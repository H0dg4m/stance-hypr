//! One phone session's transfers (docs/adr/0004): decides on the phone's
//! offers, writes what arrives, asks the user about files, offers the
//! PC's new clipboard text and queued files, and feeds outgoing data as
//! credit allows. The session loop owns the socket and the stream state
//! and calls in here; nothing here blocks on a person or a big file.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::time::{Instant, SystemTime};

use super::noise::Session;
use super::proto::{CHUNK, StreamMsg, data_frame};
use super::stream::{Decision, Out, Streams};
use super::transfer::{
    self, ClipFrom, FilesFrom, Hub, INCOMING_MAX, Part, Source, State, human_size, plain,
};
use crate::error::Result;

/// Chunks sent per loop turn (then the loop reads again).
const BURST: usize = 64;
/// Largest clipboard text either way.
const CLIP_MAX: u64 = 1 << 20;

enum In {
    Clip(Vec<u8>),
    File {
        tid: u64,
        name: String,
        size: u64,
        part: Option<Part>,
        reserved: u64,
    },
}

enum Src {
    Clip(Vec<u8>, usize),
    File(u64, Source),
}

pub struct Link {
    pub device: String,
    offered: Vec<String>,
    agreed: Vec<String>,
    incoming: HashMap<u32, In>,
    /// Deferred file offers: sid → consent id.
    consents: HashMap<u32, u64>,
    outgoing: HashMap<u32, Src>,
    /// A file being opened and hashed off the loop.
    preparing: Option<(u64, mpsc::Receiver<Result<Source>>)>,
    clip: Option<crate::clip::Clip>,
    /// Newest (updated, id) on the shelf already seen: only copies after it
    /// (a new item, or the same text copied again) go out.
    clip_floor: (u64, u64),
    clip_mtime: Option<SystemTime>,
    /// Settings, prompts, the queue and the shelf are looked at this often;
    /// data goes out every turn.
    last_slow: Option<Instant>,
}

const SLOW_EVERY: std::time::Duration = std::time::Duration::from_millis(200);

fn has(list: &[String], cap: &str) -> bool {
    list.iter().any(|c| c == cap)
}

impl Link {
    pub fn new(device: &str, offered: Vec<String>, agreed: Vec<String>) -> Self {
        // Same state dir as `stance clip` (STANCE_STATE_DIR overrides).
        let state = match std::env::var_os("STANCE_STATE_DIR").map(std::path::PathBuf::from) {
            Some(p) if p.is_absolute() => Some(p),
            _ => crate::store::state_dir().ok(),
        };
        let clip = state.and_then(|d| crate::clip::Clip::open(&d).ok());
        let floor = clip
            .as_ref()
            .and_then(|c| c.load().ok())
            .and_then(|i| i.items.iter().map(|e| (e.updated, e.id)).max())
            .unwrap_or((0, 0));
        let mtime = clip
            .as_ref()
            .and_then(|c| std::fs::metadata(&c.rev_path).ok())
            .and_then(|m| m.modified().ok());
        Self {
            device: device.into(),
            offered,
            agreed,
            incoming: HashMap::new(),
            consents: HashMap::new(),
            outgoing: HashMap::new(),
            preparing: None,
            clip,
            clip_floor: floor,
            clip_mtime: mtime,
            last_slow: None,
        }
    }

    /// Something is moving: the loop polls fast.
    pub fn busy(&self) -> bool {
        !self.outgoing.is_empty() || !self.consents.is_empty() || self.preparing.is_some()
    }

    /// Bytes are waiting for credit we have: the loop barely waits.
    pub fn can_send(&self, st: &Streams) -> bool {
        self.outgoing.keys().any(|sid| st.quota(*sid) > 0)
    }

    /// The phone's offer, already checked by the stream state (kind agreed,
    /// size within the protocol cap, limits).
    pub fn decide(
        &mut self,
        hub: &Arc<Hub>,
        sid: u32,
        kind: &str,
        size: u64,
        name: Option<&str>,
    ) -> Decision {
        let set = hub.settings();
        match kind {
            "clip.text" => {
                if set.clip_from_phone == ClipFrom::Off || size > CLIP_MAX {
                    return Decision::Decline("off".into());
                }
                self.incoming.insert(sid, In::Clip(Vec::new()));
                Decision::Accept
            }
            "file.push" => {
                if set.files_from_phone != FilesFrom::Ask {
                    return Decision::Decline("off".into());
                }
                if hub.cooling(&self.device) {
                    return Decision::Decline("cooldown".into());
                }
                if size > set.max_file() {
                    return Decision::Decline("too large".into());
                }
                let Ok(dir) = set.download_dir() else {
                    return Decision::Decline("no download folder".into());
                };
                if transfer::ensure_dir(&dir).is_err()
                    || transfer::free_space(&dir).is_none_or(|f| f < size + transfer::FREE_MARGIN)
                    || hub.incoming.load(Ordering::Relaxed) + size > INCOMING_MAX
                {
                    return Decision::Decline("no space".into());
                }
                let name = transfer::clean_name(name.unwrap_or("file"));
                let Some(cid) = hub.consent_open(&self.device, &name, size) else {
                    return Decision::Decline("busy".into());
                };
                let tid = hub.new_transfer("from_phone", &self.device, &name, size, State::Waiting);
                let (summary, body) = (
                    format!("File from {}", self.device),
                    format!(
                        "{name} ({}). Accept here, or: stance deck transfer accept {cid}",
                        human_size(size)
                    ),
                );
                // The answer goes through the hub (as does `stance deck
                // transfer accept|decline`); a stale id does nothing.
                let h = Arc::clone(hub);
                std::thread::spawn(move || {
                    if let Some(yes) = transfer::ask(&summary, &body) {
                        h.consent_answer(cid, yes);
                    }
                });
                self.consents.insert(sid, cid);
                self.incoming.insert(
                    sid,
                    In::File {
                        tid,
                        name,
                        size,
                        part: None,
                        reserved: 0,
                    },
                );
                Decision::Defer
            }
            _ => Decision::Decline("unsupported".into()),
        }
    }

    /// The phone's answers to our offers (before the stream state sees them).
    pub fn peer_msg(&mut self, hub: &Hub, m: &StreamMsg) {
        let (sid, state, detail) = match m {
            StreamMsg::Done { sid, ok: true, .. } => (*sid, State::Done, None),
            StreamMsg::Done { sid, error, .. } => (*sid, State::Failed, error.clone()),
            StreamMsg::Decline { sid, reason } => (*sid, State::Declined, Some(reason.clone())),
            StreamMsg::Cancel { sid, reason } => (*sid, State::Failed, Some(reason.clone())),
            _ => return,
        };
        if let Some(Src::File(tid, _)) = self.outgoing.remove(&sid) {
            hub.update(tid, state, detail.map(|d| plain(&d, 80)));
        } else {
            self.outgoing.remove(&sid);
        }
    }

    /// Carries out what the stream state returned.
    pub fn apply(
        &mut self,
        hub: &Hub,
        st: &mut Streams,
        s: &mut Session,
        outs: Vec<Out>,
    ) -> Result<()> {
        let mut cancel = Vec::new();
        // Every output is handled (cleanup included) even if a send fails;
        // the first send error is returned at the end.
        let mut failed = None;
        for o in outs {
            match o {
                Out::Send(m) => {
                    if failed.is_none()
                        && let Err(e) = s.send(&m)
                    {
                        failed = Some(e);
                    }
                }
                Out::Data { sid, payload } => match self.incoming.get_mut(&sid) {
                    Some(In::Clip(buf)) => buf.extend_from_slice(&payload),
                    Some(In::File { part: Some(p), .. }) => {
                        if p.write(&payload).is_err() {
                            cancel.push(sid);
                        }
                    }
                    _ => cancel.push(sid),
                },
                Out::Finished { sid, ok, .. } => self.finished(hub, sid, ok),
                Out::Withdrawn { sid } => {
                    if let Some(cid) = self.consents.remove(&sid) {
                        hub.consent_close(cid);
                    }
                    if let Some(In::File { tid, name, .. }) = self.incoming.remove(&sid) {
                        hub.update(tid, State::Declined, Some("not answered".into()));
                        hub.refused(&self.device);
                        transfer::notify(
                            &format!("File from {}", self.device),
                            &format!(
                                "{} was not accepted in time, or the phone cancelled it",
                                plain(&name, 80)
                            ),
                        );
                    }
                }
            }
        }
        if let Some(e) = failed {
            return Err(e);
        }
        for sid in cancel {
            let outs = st.cancel(sid, "write failed");
            self.apply(hub, st, s, outs)?;
        }
        Ok(())
    }

    fn finished(&mut self, hub: &Hub, sid: u32, ok: bool) {
        match self.incoming.remove(&sid) {
            Some(In::Clip(buf)) => {
                if !ok {
                    return;
                }
                let Ok(text) = String::from_utf8(buf) else {
                    eprintln!("stance deck: {}: clipboard text is not UTF-8", self.device);
                    return;
                };
                // Before wl-copy: the shelf picks it up and must not send it back.
                hub.echo_add(&transfer::sha256_hex(text.as_bytes()), &self.device);
                let device = self.device.clone();
                std::thread::spawn(move || match transfer::apply_text(text.as_bytes()) {
                    Ok(()) => transfer::notify(&format!("Clipboard from {device}"), &text),
                    Err(e) => eprintln!("stance deck: clipboard from {device}: {}", e.message),
                });
            }
            Some(In::File {
                tid,
                part,
                reserved,
                name,
                ..
            }) => {
                hub.incoming.fetch_sub(reserved, Ordering::Relaxed);
                let Some(part) = part else {
                    // Still waiting for an answer when the session ended.
                    hub.update(tid, State::Failed, Some("connection lost".into()));
                    return;
                };
                if !ok {
                    part.discard();
                    hub.update(
                        tid,
                        State::Failed,
                        Some("incomplete or checksum mismatch".into()),
                    );
                    return;
                }
                match part.finish() {
                    Ok(path) => {
                        hub.update(tid, State::Done, Some(path.display().to_string()));
                        transfer::notify(
                            &format!("File from {}", self.device),
                            &format!("Saved {}", plain(&name, 80)),
                        );
                    }
                    Err(e) => hub.update(tid, State::Failed, Some(e.message)),
                }
            }
            None => {}
        }
    }

    /// Periodic work. `Ok(false)`: the settings changed what this session
    /// offers; the caller ends it ("settings changed").
    pub fn tick(
        &mut self,
        hub: &Hub,
        st: &mut Streams,
        s: &mut Session,
        now: Instant,
    ) -> Result<bool> {
        if self
            .last_slow
            .is_some_and(|t| now.duration_since(t) < SLOW_EVERY)
        {
            self.push(hub, st, s, now)?;
            return Ok(true);
        }
        self.last_slow = Some(now);
        let set = hub.settings();
        if set.offered_caps() != self.offered {
            return Ok(false);
        }
        // Answers to file prompts.
        let answered: Vec<(u32, u64, bool)> = self
            .consents
            .iter()
            .filter_map(|(sid, cid)| match hub.consent_state(*cid) {
                Some(Some(yes)) => Some((*sid, *cid, yes)),
                None => Some((*sid, *cid, false)),
                Some(None) => None,
            })
            .collect();
        for (sid, cid, yes) in answered {
            self.consents.remove(&sid);
            hub.consent_close(cid);
            let accepted = yes && self.open_part(hub, sid, &set);
            if accepted {
                hub.accepted(&self.device);
                if let Some(o) = st.accept(sid, now) {
                    self.apply(hub, st, s, vec![o])?;
                }
            } else {
                if let Some(In::File { tid, .. }) = self.incoming.remove(&sid) {
                    hub.update(tid, State::Declined, None);
                }
                hub.refused(&self.device);
                if let Some(o) = st.decline(sid, "declined") {
                    self.apply(hub, st, s, vec![o])?;
                }
            }
        }
        // Files the user queued for this phone.
        if has(&self.agreed, "file.pull") {
            self.next_file(hub, st, s, now)?;
        }
        // New PC clipboard text.
        if set.clip_to_phone && has(&self.agreed, "clip.text") {
            self.watch_clip(hub, st, s, now)?;
        }
        self.push(hub, st, s, now)?;
        Ok(true)
    }

    fn open_part(&mut self, hub: &Hub, sid: u32, set: &transfer::Settings) -> bool {
        let Some(In::File {
            tid,
            name,
            size,
            part,
            reserved,
        }) = self.incoming.get_mut(&sid)
        else {
            return false;
        };
        let Ok(dir) = set.download_dir() else {
            return false;
        };
        match transfer::ensure_dir(&dir).and_then(|_| Part::create(&dir, name)) {
            Ok(p) => {
                *part = Some(p);
                *reserved = *size;
                hub.incoming.fetch_add(*size, Ordering::Relaxed);
                hub.update(*tid, State::Receiving, None);
                true
            }
            Err(e) => {
                hub.update(*tid, State::Failed, Some(e.message));
                false
            }
        }
    }

    fn next_file(
        &mut self,
        hub: &Hub,
        st: &mut Streams,
        s: &mut Session,
        now: Instant,
    ) -> Result<()> {
        if let Some((tid, rx)) = &self.preparing {
            let tid = *tid;
            match rx.try_recv() {
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => {
                    hub.update(tid, State::Failed, Some("could not read the file".into()));
                }
                Ok(Err(e)) => hub.update(tid, State::Failed, Some(e.message)),
                Ok(Ok(src)) => match st.offer(
                    "file.pull",
                    src.size,
                    &src.sha256,
                    Some(src.name.clone()),
                    None,
                    now,
                ) {
                    Ok(m) => {
                        let StreamMsg::Offer { sid, .. } = &m else {
                            unreachable!()
                        };
                        self.outgoing.insert(*sid, Src::File(tid, src));
                        s.send(&m)?;
                        hub.update(
                            tid,
                            State::Waiting,
                            Some(format!("offered to {}", self.device)),
                        );
                    }
                    Err(e) => hub.update(tid, State::Failed, Some(e)),
                },
            }
            self.preparing = None;
            return Ok(());
        }
        if self.outgoing.values().any(|o| matches!(o, Src::File(..))) {
            return Ok(());
        }
        if let Some(o) = hub.take_for(&self.device) {
            let max = hub.settings().max_file();
            let (tx, rx) = mpsc::channel();
            let path = o.path.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Source::open(&path, max));
            });
            hub.update(
                o.id,
                State::Queued,
                Some(format!("reading for {}", self.device)),
            );
            self.preparing = Some((o.id, rx));
        }
        Ok(())
    }

    fn watch_clip(
        &mut self,
        hub: &Hub,
        st: &mut Streams,
        s: &mut Session,
        now: Instant,
    ) -> Result<()> {
        let Some(clip) = &self.clip else {
            return Ok(());
        };
        let mtime = std::fs::metadata(&clip.rev_path)
            .ok()
            .and_then(|m| m.modified().ok());
        if mtime == self.clip_mtime {
            return Ok(());
        }
        self.clip_mtime = mtime;
        let Ok(idx) = clip.load() else { return Ok(()) };
        // Only copies made since: `updated` moves when something is copied
        // (a new item, the same text again, or `clip copy`), not on pin,
        // remove or undo, so those never send old entries.
        let floor = self.clip_floor;
        let newest = idx
            .items
            .iter()
            .filter(|e| (e.updated, e.id) > floor && e.trashed.is_none())
            .max_by_key(|e| (e.updated, e.id))
            .cloned();
        if let Some(e) = &newest {
            self.clip_floor = (e.updated, e.id);
        }
        let Some(e) = newest else { return Ok(()) };
        if !matches!(e.kind, crate::clip::Kind::Text | crate::clip::Kind::Link) || e.size > CLIP_MAX
        {
            return Ok(());
        }
        let Ok(cfg) = clip.config() else {
            return Ok(());
        };
        if !cfg.enabled || !cfg.text || cfg.paused(crate::clip::now()).is_some() {
            return Ok(());
        }
        let mut data = Vec::new();
        use std::io::Read;
        if clip
            .open_blob(&e)
            .and_then(|f| {
                f.take(CLIP_MAX)
                    .read_to_end(&mut data)
                    .map_err(|err| crate::error::Error::new("io", err.to_string()))
            })
            .is_err()
            || data.is_empty()
        {
            return Ok(());
        }
        let hash = transfer::sha256_hex(&data);
        if hub.echo_from(&hash, &self.device) {
            return Ok(());
        }
        // A newer copy replaces one still waiting to go out.
        let old: Vec<u32> = self
            .outgoing
            .iter()
            .filter(|(_, o)| matches!(o, Src::Clip(_, 0)))
            .map(|(sid, _)| *sid)
            .collect();
        for sid in old {
            self.outgoing.remove(&sid);
            let outs = st.cancel(sid, "replaced");
            self.apply(hub, st, s, outs)?;
        }
        match st.offer(
            "clip.text",
            data.len() as u64,
            &hash,
            None,
            Some("text/plain".into()),
            now,
        ) {
            Ok(m) => {
                let StreamMsg::Offer { sid, .. } = &m else {
                    unreachable!()
                };
                self.outgoing.insert(*sid, Src::Clip(data, 0));
                s.send(&m)?;
            }
            Err(e) => eprintln!("stance deck: clipboard to {}: {e}", self.device),
        }
        Ok(())
    }

    /// Sends what credit allows, a burst at a time.
    fn push(&mut self, hub: &Hub, st: &mut Streams, s: &mut Session, now: Instant) -> Result<()> {
        let sids: Vec<u32> = self.outgoing.keys().copied().collect();
        for sid in sids {
            for _ in 0..BURST {
                let quota = st.quota(sid) as usize;
                if quota == 0 {
                    break;
                }
                let n = quota.min(CHUNK);
                let chunk = match self.outgoing.get_mut(&sid) {
                    Some(Src::Clip(data, pos)) => {
                        let end = (*pos + n).min(data.len());
                        let c = data[*pos..end].to_vec();
                        *pos = end;
                        c
                    }
                    Some(Src::File(tid, src)) => match src.read(n) {
                        Ok(c) => {
                            hub.update(
                                *tid,
                                State::Sending,
                                Some(format!(
                                    "{} of {}",
                                    human_size(src.sent),
                                    human_size(src.size)
                                )),
                            );
                            c
                        }
                        Err(e) => {
                            hub.update(*tid, State::Failed, Some(e.message));
                            self.outgoing.remove(&sid);
                            let outs = st.cancel(sid, "read failed");
                            self.apply(hub, st, s, outs)?;
                            break;
                        }
                    },
                    None => break,
                };
                if chunk.is_empty() {
                    break;
                }
                s.send_plain(&data_frame(sid, &chunk))?;
                match st.sent(sid, chunk.len() as u64, now) {
                    Ok(Some(end)) => {
                        // The file must still be the one we hashed.
                        if let Some(Src::File(tid, src)) = self.outgoing.get(&sid)
                            && !src.unchanged()
                        {
                            hub.update(
                                *tid,
                                State::Failed,
                                Some("the file changed while sending".into()),
                            );
                            self.outgoing.remove(&sid);
                            let outs = st.cancel(sid, "file changed");
                            self.apply(hub, st, s, outs)?;
                            break;
                        }
                        s.send(&end)?;
                        if matches!(self.outgoing.get(&sid), Some(Src::Clip(..))) {
                            // Nothing to report for clipboard text.
                        }
                        break;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("stance deck: send: {e}");
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    /// The session ended: clean up everything still open.
    pub fn close(&mut self, hub: &Hub, outs: Vec<Out>) {
        for o in outs {
            match o {
                Out::Finished { sid, .. } => self.finished(hub, sid, false),
                Out::Withdrawn { sid } => {
                    if let Some(In::File { tid, .. }) = self.incoming.remove(&sid) {
                        hub.update(tid, State::Failed, Some("connection lost".into()));
                    }
                }
                _ => {}
            }
        }
        // Whatever the stream state no longer reports (an error mid-apply).
        let left: Vec<u32> = self.incoming.keys().copied().collect();
        for sid in left {
            self.finished(hub, sid, false);
        }
        for (_, cid) in self.consents.drain() {
            hub.consent_close(cid);
        }
        for (_, o) in self.outgoing.drain() {
            if let Src::File(tid, _) = o {
                hub.update(tid, State::Failed, Some("connection lost".into()));
            }
        }
        if let Some((tid, _)) = self.preparing.take() {
            hub.update(tid, State::Failed, Some("connection lost".into()));
        }
    }
}
