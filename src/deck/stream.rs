//! Streams inside one deck session (protocol v2, docs/adr/0003): offers,
//! credit-based flow control, size and hash checks. Pure state: the session
//! loop feeds it what arrived and sends what it returns, so every rule is
//! unit-tested without sockets. An `Err` is a protocol error: the session
//! ends (and `close_all` reports every open stream as failed).

// The sending side (offer/quota/sent) and accepting offers are used by the
// 0.7 transfers; 0.6 tests them here and declines every offer.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::proto::{CAP_NAMES, PAYLOAD_MAX, StreamMsg};

/// Largest credit a receiver grants at once.
pub const WINDOW_MAX: u64 = 256 * 1024;
/// A stream with no data or credit for this long is cancelled.
pub const IDLE: Duration = Duration::from_secs(30);
/// Open streams per direction.
pub const OPEN_MAX: usize = 2;
/// Offers accepted per `OFFER_PERIOD` (more are declined, not errors).
pub const OFFERS_MAX: usize = 4;
pub const OFFER_PERIOD: Duration = Duration::from_secs(10);
/// Closed sids remembered: frames for them are dropped, not errors (a
/// `cancel` may cross data in flight).
pub const RECENT_MAX: usize = 8;
pub const NAME_MAX: usize = 255;
pub const MIME_MAX: usize = 127;

/// Size cap per kind; `None` for names outside the protocol.
pub fn size_cap(kind: &str) -> Option<u64> {
    match kind {
        "clip.text" => Some(1 << 20),
        "clip.image" => Some(32 << 20),
        "file.push" | "file.pull" => Some(512 << 20),
        _ => None,
    }
}

/// The receiving side's answer to a valid offer.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Accept,
    Decline(String),
}

/// What the session does next.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// Send this stream message to the peer.
    Send(StreamMsg),
    /// Payload of an accepted incoming stream, in order: write it to the
    /// stream's temp file.
    Data { sid: u32, payload: Vec<u8> },
    /// An incoming stream is over. `ok`: all bytes arrived and the SHA-256
    /// matched; otherwise its temp data must be deleted.
    Finished { sid: u32, kind: String, ok: bool },
}

#[derive(Debug)]
struct Inbound {
    kind: String,
    size: u64,
    sha256: String,
    received: u64,
    granted: u64,
    hasher: Sha256,
    last: Instant,
}

#[derive(Debug)]
struct Outbound {
    size: u64,
    sent: u64,
    /// Total credit the peer granted (accept + windows).
    granted: u64,
    accepted: bool,
    ended: bool,
    last: Instant,
}

#[derive(Debug)]
pub struct Streams {
    agreed: Vec<String>,
    /// The peer's sids are odd (the peer is the phone) or even (the PC).
    peer_odd: bool,
    inbound: HashMap<u32, Inbound>,
    outbound: HashMap<u32, Outbound>,
    recent: VecDeque<u32>,
    offers: VecDeque<Instant>,
    /// Highest sid the peer used: sids only grow, so none is reused.
    peer_last: u32,
    next_own: u32,
}

fn err<T>(m: &str) -> Result<T, String> {
    Err(m.to_string())
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl Streams {
    /// `agreed`: the negotiated caps. `peer_odd`: true on the PC (the phone
    /// uses odd sids), false on a client.
    pub fn new(agreed: Vec<String>, peer_odd: bool) -> Self {
        Self {
            agreed: agreed
                .into_iter()
                .filter(|c| CAP_NAMES.contains(&c.as_str()))
                .collect(),
            peer_odd,
            inbound: HashMap::new(),
            outbound: HashMap::new(),
            recent: VecDeque::new(),
            offers: VecDeque::new(),
            peer_last: 0,
            next_own: if peer_odd { 2 } else { 1 },
        }
    }

    pub fn open(&self) -> usize {
        self.inbound.len() + self.outbound.len()
    }

    fn close(&mut self, sid: u32) {
        self.recent.push_back(sid);
        while self.recent.len() > RECENT_MAX {
            self.recent.pop_front();
        }
    }

    fn recent(&self, sid: u32) -> bool {
        self.recent.contains(&sid)
    }

    /// A stream message from the peer. `decide` is asked only for a valid
    /// offer of a negotiated kind within the limits.
    pub fn on_msg(
        &mut self,
        msg: StreamMsg,
        now: Instant,
        decide: impl FnOnce(&str, u64, Option<&str>) -> Decision,
    ) -> Result<Vec<Out>, String> {
        match msg {
            StreamMsg::Offer {
                sid,
                kind,
                size,
                sha256,
                name,
                mime,
            } => {
                if sid == 0 || (sid % 2 == 1) != self.peer_odd || sid <= self.peer_last {
                    return err("bad stream id");
                }
                self.peer_last = sid;
                let decline = |r: &str| {
                    Ok(vec![Out::Send(StreamMsg::Decline {
                        sid,
                        reason: r.into(),
                    })])
                };
                while self
                    .offers
                    .front()
                    .is_some_and(|t| now.duration_since(*t) > OFFER_PERIOD)
                {
                    self.offers.pop_front();
                }
                if !self.agreed.contains(&kind) {
                    return decline("unsupported");
                }
                if self.offers.len() >= OFFERS_MAX {
                    return decline("too fast");
                }
                self.offers.push_back(now);
                let cap = size_cap(&kind).unwrap_or(0);
                if size == 0 || size > cap {
                    return decline("too large");
                }
                if !is_hex64(&sha256)
                    || name.as_ref().is_some_and(|n| n.len() > NAME_MAX)
                    || mime.as_ref().is_some_and(|m| m.len() > MIME_MAX)
                {
                    return decline("bad offer");
                }
                if self.inbound.len() >= OPEN_MAX {
                    return decline("busy");
                }
                match decide(&kind, size, name.as_deref()) {
                    Decision::Decline(r) => decline(&r),
                    Decision::Accept => {
                        let window = size.min(WINDOW_MAX);
                        self.inbound.insert(
                            sid,
                            Inbound {
                                kind,
                                size,
                                sha256,
                                received: 0,
                                granted: window,
                                hasher: Sha256::new(),
                                last: now,
                            },
                        );
                        Ok(vec![Out::Send(StreamMsg::Accept { sid, window })])
                    }
                }
            }
            StreamMsg::End { sid } => {
                let Some(s) = self.inbound.get(&sid) else {
                    return if self.recent(sid) {
                        Ok(vec![])
                    } else {
                        err("unknown stream")
                    };
                };
                if s.received != s.size {
                    return err("end before all data");
                }
                let s = self.inbound.remove(&sid).expect("checked");
                self.close(sid);
                let ok = hex(&s.hasher.finalize()) == s.sha256;
                Ok(vec![
                    Out::Send(StreamMsg::Done {
                        sid,
                        ok,
                        error: (!ok).then(|| "checksum mismatch".to_string()),
                    }),
                    Out::Finished {
                        sid,
                        kind: s.kind,
                        ok,
                    },
                ])
            }
            StreamMsg::Cancel { sid, .. } => {
                if let Some(s) = self.inbound.remove(&sid) {
                    self.close(sid);
                    return Ok(vec![Out::Finished {
                        sid,
                        kind: s.kind,
                        ok: false,
                    }]);
                }
                if self.outbound.remove(&sid).is_some() {
                    self.close(sid);
                    return Ok(vec![]);
                }
                if self.recent(sid) {
                    Ok(vec![])
                } else {
                    err("unknown stream")
                }
            }
            StreamMsg::Accept { sid, window } => {
                let Some(o) = self.outbound.get_mut(&sid) else {
                    return if self.recent(sid) {
                        Ok(vec![])
                    } else {
                        err("unknown stream")
                    };
                };
                if o.accepted || window == 0 || window > WINDOW_MAX || window > o.size {
                    return err("bad accept");
                }
                o.accepted = true;
                o.granted = window;
                o.last = now;
                Ok(vec![])
            }
            StreamMsg::Window { sid, bytes } => {
                let Some(o) = self.outbound.get_mut(&sid) else {
                    return if self.recent(sid) {
                        Ok(vec![])
                    } else {
                        err("unknown stream")
                    };
                };
                let total = o.granted.checked_add(bytes);
                if !o.accepted
                    || bytes == 0
                    || bytes > WINDOW_MAX
                    || total.is_none_or(|t| t > o.size)
                {
                    return err("bad window");
                }
                o.granted = total.expect("checked");
                o.last = now;
                Ok(vec![])
            }
            StreamMsg::Decline { sid, .. } => {
                match self.outbound.get(&sid) {
                    Some(o) if !o.accepted => {}
                    Some(_) => return err("decline after accept"),
                    None => {
                        return if self.recent(sid) {
                            Ok(vec![])
                        } else {
                            err("unknown stream")
                        };
                    }
                }
                self.outbound.remove(&sid);
                self.close(sid);
                Ok(vec![])
            }
            StreamMsg::Done { sid, .. } => {
                match self.outbound.get(&sid) {
                    Some(o) if o.ended => {}
                    Some(_) => return err("done before end"),
                    None => {
                        return if self.recent(sid) {
                            Ok(vec![])
                        } else {
                            err("unknown stream")
                        };
                    }
                }
                self.outbound.remove(&sid);
                self.close(sid);
                Ok(vec![])
            }
        }
    }

    /// A data frame from the peer.
    pub fn on_data(
        &mut self,
        sid: u32,
        payload: Vec<u8>,
        now: Instant,
    ) -> Result<Vec<Out>, String> {
        let Some(s) = self.inbound.get_mut(&sid) else {
            return if self.recent(sid) {
                Ok(vec![])
            } else {
                err("data for unknown stream")
            };
        };
        let n = payload.len() as u64;
        if payload.is_empty() || payload.len() > PAYLOAD_MAX {
            return err("bad data frame length");
        }
        let received = s.received + n;
        if received > s.granted || received > s.size {
            return err("data beyond credit");
        }
        s.received = received;
        s.hasher.update(&payload);
        s.last = now;
        let mut out = vec![Out::Data { sid, payload }];
        // Keep at least half a window in flight while bytes remain.
        let outstanding = s.granted - s.received;
        if outstanding < WINDOW_MAX / 2 && s.granted < s.size {
            let bytes = (s.size - s.granted).min(WINDOW_MAX - outstanding);
            s.granted += bytes;
            out.push(Out::Send(StreamMsg::Window { sid, bytes }));
        }
        Ok(out)
    }

    /// Our own offer. The caller sends the returned message, then data as
    /// credit allows (`quota`, `sent`) and `end` when `sent` says so.
    pub fn offer(
        &mut self,
        kind: &str,
        size: u64,
        sha256: &str,
        name: Option<String>,
        mime: Option<String>,
        now: Instant,
    ) -> Result<StreamMsg, String> {
        if !self.agreed.iter().any(|c| c == kind) {
            return err("not negotiated");
        }
        if self.outbound.len() >= OPEN_MAX {
            return err("busy");
        }
        if size == 0 || size > size_cap(kind).unwrap_or(0) || !is_hex64(sha256) {
            return err("bad offer");
        }
        let sid = self.next_own;
        self.next_own = sid.checked_add(2).ok_or("out of stream ids")?;
        self.outbound.insert(
            sid,
            Outbound {
                size,
                sent: 0,
                granted: 0,
                accepted: false,
                ended: false,
                last: now,
            },
        );
        Ok(StreamMsg::Offer {
            sid,
            kind: kind.into(),
            size,
            sha256: sha256.into(),
            name,
            mime,
        })
    }

    /// Bytes we may send on our stream now.
    pub fn quota(&self, sid: u32) -> u64 {
        self.outbound
            .get(&sid)
            .filter(|o| o.accepted && !o.ended)
            .map_or(0, |o| o.granted - o.sent)
    }

    /// Record `n` bytes sent; returns `end` once all `size` bytes are out.
    pub fn sent(&mut self, sid: u32, n: u64, now: Instant) -> Result<Option<StreamMsg>, String> {
        let o = self.outbound.get_mut(&sid).ok_or("unknown stream")?;
        if !o.accepted || o.ended || n > o.granted - o.sent {
            return err("send beyond credit");
        }
        o.sent += n;
        o.last = now;
        if o.sent == o.size {
            o.ended = true;
            return Ok(Some(StreamMsg::End { sid }));
        }
        Ok(None)
    }

    /// Cancels streams idle for `IDLE`.
    pub fn tick(&mut self, now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        let idle_in: Vec<u32> = self
            .inbound
            .iter()
            .filter(|(_, s)| now.duration_since(s.last) >= IDLE)
            .map(|(k, _)| *k)
            .collect();
        for sid in idle_in {
            let s = self.inbound.remove(&sid).expect("listed");
            self.close(sid);
            out.push(Out::Send(StreamMsg::Cancel {
                sid,
                reason: "idle".into(),
            }));
            out.push(Out::Finished {
                sid,
                kind: s.kind,
                ok: false,
            });
        }
        let idle_out: Vec<u32> = self
            .outbound
            .iter()
            .filter(|(_, o)| now.duration_since(o.last) >= IDLE)
            .map(|(k, _)| *k)
            .collect();
        for sid in idle_out {
            self.outbound.remove(&sid);
            self.close(sid);
            out.push(Out::Send(StreamMsg::Cancel {
                sid,
                reason: "idle".into(),
            }));
        }
        out
    }

    /// The session ended: every open incoming stream failed.
    pub fn close_all(&mut self) -> Vec<Out> {
        let mut out: Vec<Out> = self
            .inbound
            .drain()
            .map(|(sid, s)| Out::Finished {
                sid,
                kind: s.kind,
                ok: false,
            })
            .collect();
        out.sort_by_key(|o| match o {
            Out::Finished { sid, .. } => *sid,
            _ => 0,
        });
        self.outbound.clear();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(b: &[u8]) -> String {
        hex(&Sha256::digest(b))
    }
    fn caps() -> Vec<String> {
        vec!["file.push".into(), "clip.text".into()]
    }
    fn accept(_: &str, _: u64, _: Option<&str>) -> Decision {
        Decision::Accept
    }

    /// Phone (odd sids) sends `data` to the PC through both state machines.
    fn transfer(data: &[u8], hash: &str) -> (Vec<u8>, Vec<Out>) {
        let t = Instant::now();
        let mut phone = Streams::new(caps(), false);
        let mut pc = Streams::new(caps(), true);
        let offer = phone
            .offer(
                "file.push",
                data.len() as u64,
                hash,
                Some("a.txt".into()),
                None,
                t,
            )
            .unwrap();
        let mut to_phone: VecDeque<StreamMsg> = VecDeque::new();
        let mut got = Vec::new();
        let mut finished = Vec::new();
        let handle =
            |outs: Vec<Out>, q: &mut VecDeque<StreamMsg>, got: &mut Vec<u8>, fin: &mut Vec<Out>| {
                for o in outs {
                    match o {
                        Out::Send(m) => q.push_back(m),
                        Out::Data { payload, .. } => got.extend(payload),
                        f @ Out::Finished { .. } => fin.push(f),
                    }
                }
            };
        let outs = pc.on_msg(offer, t, accept).unwrap();
        handle(outs, &mut to_phone, &mut got, &mut finished);
        let sid = 1;
        let mut off = 0usize;
        loop {
            while let Some(m) = to_phone.pop_front() {
                assert!(phone.on_msg(m, t, accept).unwrap().is_empty());
            }
            let q = phone.quota(sid) as usize;
            if q == 0 {
                break;
            }
            let n = q.min(crate::deck::proto::CHUNK).min(data.len() - off);
            let outs = pc.on_data(sid, data[off..off + n].to_vec(), t).unwrap();
            handle(outs, &mut to_phone, &mut got, &mut finished);
            off += n;
            if let Some(end) = phone.sent(sid, n as u64, t).unwrap() {
                let outs = pc.on_msg(end, t, accept).unwrap();
                handle(outs, &mut to_phone, &mut got, &mut finished);
                while let Some(m) = to_phone.pop_front() {
                    phone.on_msg(m, t, accept).unwrap();
                }
                break;
            }
        }
        assert_eq!(phone.open() + pc.open(), 0);
        (got, finished)
    }

    #[test]
    fn a_transfer_crosses_many_windows() {
        let data: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect();
        let (got, fin) = transfer(&data, &sha(&data));
        assert_eq!(got, data);
        assert_eq!(
            fin,
            vec![Out::Finished {
                sid: 1,
                kind: "file.push".into(),
                ok: true
            }]
        );
    }

    #[test]
    fn a_wrong_hash_fails_the_stream_not_the_session() {
        let data = vec![7u8; 1000];
        let (_, fin) = transfer(&data, &sha(b"other"));
        assert_eq!(
            fin,
            vec![Out::Finished {
                sid: 1,
                kind: "file.push".into(),
                ok: false
            }]
        );
    }

    fn offer(sid: u32, kind: &str, size: u64) -> StreamMsg {
        StreamMsg::Offer {
            sid,
            kind: kind.into(),
            size,
            sha256: sha(b"x"),
            name: None,
            mime: None,
        }
    }
    fn declined(outs: Vec<Out>) -> String {
        match &outs[..] {
            [Out::Send(StreamMsg::Decline { reason, .. })] => reason.clone(),
            o => panic!("not declined: {o:?}"),
        }
    }

    #[test]
    fn offers_are_checked() {
        let t = Instant::now();
        let mut pc = Streams::new(caps(), true);
        assert_eq!(
            declined(pc.on_msg(offer(1, "clip.image", 10), t, accept).unwrap()),
            "unsupported"
        );
        assert_eq!(
            declined(
                pc.on_msg(offer(3, "clip.text", 2 << 20), t, accept)
                    .unwrap()
            ),
            "too large"
        );
        assert_eq!(
            declined(pc.on_msg(offer(5, "clip.text", 0), t, accept).unwrap()),
            "too large"
        );
        let bad = StreamMsg::Offer {
            sid: 7,
            kind: "clip.text".into(),
            size: 3,
            sha256: "ABC".into(),
            name: None,
            mime: None,
        };
        assert_eq!(declined(pc.on_msg(bad, t, accept).unwrap()), "bad offer");
        // The consent answer is passed on.
        let no = |_: &str, _: u64, _: Option<&str>| Decision::Decline("refused".into());
        assert_eq!(
            declined(
                pc.on_msg(offer(9, "clip.text", 3), t + OFFER_PERIOD * 2, no)
                    .unwrap()
            ),
            "refused"
        );
        // Parity, reuse and order are protocol errors.
        assert!(pc.on_msg(offer(10, "clip.text", 3), t, accept).is_err());
        assert!(pc.on_msg(offer(9, "clip.text", 3), t, accept).is_err());
        assert!(pc.on_msg(offer(0, "clip.text", 3), t, accept).is_err());
    }

    #[test]
    fn offers_are_rate_limited_and_open_streams_capped() {
        let t = Instant::now();
        let mut pc = Streams::new(caps(), true);
        assert!(matches!(
            pc.on_msg(offer(1, "clip.text", 3), t, accept).unwrap()[0],
            Out::Send(StreamMsg::Accept { .. })
        ));
        assert!(matches!(
            pc.on_msg(offer(3, "clip.text", 3), t, accept).unwrap()[0],
            Out::Send(StreamMsg::Accept { .. })
        ));
        assert_eq!(
            declined(pc.on_msg(offer(5, "clip.text", 3), t, accept).unwrap()),
            "busy"
        );
        assert_eq!(
            declined(pc.on_msg(offer(7, "clip.text", 3), t, accept).unwrap()),
            "busy"
        );
        assert_eq!(
            declined(pc.on_msg(offer(9, "clip.text", 3), t, accept).unwrap()),
            "too fast"
        );
        // The window slides.
        pc.on_msg(
            StreamMsg::Cancel {
                sid: 1,
                reason: "x".into(),
            },
            t,
            accept,
        )
        .unwrap();
        let later = t + OFFER_PERIOD + Duration::from_secs(1);
        assert!(matches!(
            pc.on_msg(offer(11, "clip.text", 3), later, accept).unwrap()[0],
            Out::Send(StreamMsg::Accept { .. })
        ));
    }

    #[test]
    fn data_rules() {
        let t = Instant::now();
        let mut pc = Streams::new(caps(), true);
        // Data before any stream (or for an unknown one) ends the session.
        assert!(pc.on_data(1, vec![1], t).is_err());
        pc.on_msg(offer(1, "file.push", 600_000), t, accept)
            .unwrap();
        assert!(pc.on_data(1, vec![], t).is_err());
        assert!(pc.on_data(1, vec![0; PAYLOAD_MAX + 1], t).is_err());
        // End before all bytes is an error.
        assert!(pc.on_msg(StreamMsg::End { sid: 1 }, t, accept).is_err());
        // Beyond the granted window.
        let mut pc = Streams::new(caps(), true);
        pc.on_msg(offer(1, "file.push", 600_000), t, accept)
            .unwrap();
        // A sender that never waits for `window`: 16 chunks fit, then credit
        // is only topped up as data arrives, never past what was granted.
        let mut sent = 0u64;
        let mut granted = WINDOW_MAX;
        let r = loop {
            match pc.on_data(1, vec![0; 16_000], t) {
                Ok(outs) => {
                    sent += 16_000;
                    for o in outs {
                        if let Out::Send(StreamMsg::Window { bytes, .. }) = o {
                            granted += bytes;
                        }
                    }
                    assert!(sent <= granted);
                }
                Err(e) => break e,
            }
        };
        assert!(r.contains("credit"));
    }

    #[test]
    fn cancelled_sids_tolerate_frames_in_flight() {
        let t = Instant::now();
        let mut pc = Streams::new(caps(), true);
        pc.on_msg(offer(1, "file.push", 100_000), t, accept)
            .unwrap();
        pc.on_data(1, vec![1; 1000], t).unwrap();
        let fin = pc
            .on_msg(
                StreamMsg::Cancel {
                    sid: 1,
                    reason: "user".into(),
                },
                t,
                accept,
            )
            .unwrap();
        assert_eq!(
            fin,
            vec![Out::Finished {
                sid: 1,
                kind: "file.push".into(),
                ok: false
            }]
        );
        assert_eq!(pc.on_data(1, vec![1; 1000], t).unwrap(), vec![]);
        assert_eq!(
            pc.on_msg(StreamMsg::End { sid: 1 }, t, accept).unwrap(),
            vec![]
        );
        // Only recent ones: an old sid falls out of the list.
        for i in 0..RECENT_MAX as u32 {
            let sid = 3 + 2 * i;
            pc.on_msg(
                offer(sid, "clip.text", 1),
                t + OFFER_PERIOD * (i + 1),
                accept,
            )
            .unwrap();
            pc.on_msg(
                StreamMsg::Cancel {
                    sid,
                    reason: "x".into(),
                },
                t,
                accept,
            )
            .unwrap();
        }
        assert!(pc.on_data(1, vec![1], t).is_err());
    }

    #[test]
    fn idle_streams_are_cancelled_and_session_end_fails_open_ones() {
        let t = Instant::now();
        let mut pc = Streams::new(caps(), true);
        pc.on_msg(offer(1, "file.push", 100_000), t, accept)
            .unwrap();
        pc.on_msg(offer(3, "clip.text", 10), t, accept).unwrap();
        pc.on_data(3, vec![1; 5], t + IDLE / 2).unwrap();
        let outs = pc.tick(t + IDLE);
        assert_eq!(
            outs,
            vec![
                Out::Send(StreamMsg::Cancel {
                    sid: 1,
                    reason: "idle".into()
                }),
                Out::Finished {
                    sid: 1,
                    kind: "file.push".into(),
                    ok: false
                }
            ]
        );
        assert_eq!(
            pc.close_all(),
            vec![Out::Finished {
                sid: 3,
                kind: "clip.text".into(),
                ok: false
            }]
        );
    }

    #[test]
    fn sender_side_rules() {
        let t = Instant::now();
        let mut phone = Streams::new(caps(), false);
        assert!(
            phone
                .offer("clip.image", 5, &sha(b"x"), None, None, t)
                .is_err()
        );
        let StreamMsg::Offer { sid, .. } = phone
            .offer("clip.text", 5, &sha(b"x"), None, None, t)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(sid, 1);
        assert_eq!(phone.quota(sid), 0);
        assert!(phone.sent(sid, 1, t).is_err());
        // An accept larger than the stream, a second accept, too much window.
        assert!(
            phone
                .clone_for_test()
                .on_msg(StreamMsg::Accept { sid, window: 6 }, t, accept)
                .is_err()
        );
        phone
            .on_msg(StreamMsg::Accept { sid, window: 3 }, t, accept)
            .unwrap();
        assert!(
            phone
                .clone_for_test()
                .on_msg(StreamMsg::Accept { sid, window: 3 }, t, accept)
                .is_err()
        );
        assert!(
            phone
                .clone_for_test()
                .on_msg(StreamMsg::Window { sid, bytes: 3 }, t, accept)
                .is_err()
        );
        assert!(
            phone
                .clone_for_test()
                .on_msg(
                    StreamMsg::Done {
                        sid,
                        ok: true,
                        error: None
                    },
                    t,
                    accept
                )
                .is_err()
        );
        assert_eq!(phone.quota(sid), 3);
        assert_eq!(phone.sent(sid, 3, t).unwrap(), None);
        phone
            .on_msg(StreamMsg::Window { sid, bytes: 2 }, t, accept)
            .unwrap();
        assert_eq!(phone.sent(sid, 2, t).unwrap(), Some(StreamMsg::End { sid }));
        assert!(
            phone
                .on_msg(
                    StreamMsg::Decline {
                        sid,
                        reason: "x".into()
                    },
                    t,
                    accept
                )
                .is_err()
        );
        phone
            .on_msg(
                StreamMsg::Done {
                    sid,
                    ok: true,
                    error: None,
                },
                t,
                accept,
            )
            .unwrap();
        assert_eq!(phone.open(), 0);
        // Next own sid is the next odd one.
        let StreamMsg::Offer { sid, .. } = phone
            .offer("clip.text", 5, &sha(b"x"), None, None, t)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(sid, 3);
    }

    impl Streams {
        fn clone_for_test(&self) -> Streams {
            Streams {
                agreed: self.agreed.clone(),
                peer_odd: self.peer_odd,
                inbound: HashMap::new(),
                outbound: self
                    .outbound
                    .iter()
                    .map(|(k, o)| {
                        (
                            *k,
                            Outbound {
                                size: o.size,
                                sent: o.sent,
                                granted: o.granted,
                                accepted: o.accepted,
                                ended: o.ended,
                                last: o.last,
                            },
                        )
                    })
                    .collect(),
                recent: self.recent.clone(),
                offers: self.offers.clone(),
                peer_last: self.peer_last,
                next_own: self.next_own,
            }
        }
    }
}
