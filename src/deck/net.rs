//! `stance deck serve`: the TCP listener for phones, a 0600 unix control
//! socket for the local CLI / panel (`pair`, `status`), and one thread per
//! session. No async runtime; everything is bounded.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde_json::json;

use super::actions::Deck;
use super::noise::{self, Incoming, Keypair, Session};
use super::pair::{self, QrPayload};
use super::proto::{ClientMsg, SCHEMA, ServerMsg, State};
use crate::error::{Error, Result};
use crate::store;

const MAX_SESSIONS: usize = 8;
const PER_IP_PER_MIN: usize = 20;
const GLOBAL_PER_MIN: usize = 60;
const PRESSES_PER_S: usize = 10;
const PAIR_MSG_TIMEOUT: Duration = Duration::from_secs(10);
const TICK: Duration = Duration::from_millis(500);

fn log(msg: &str) {
    eprintln!("stance deck: {msg}");
}

pub fn runtime_dir() -> Result<PathBuf> {
    Ok(crate::hypr::runtime_dir()?.join("stance"))
}

pub fn control_path() -> Result<PathBuf> {
    Ok(runtime_dir()?.join("deck.sock"))
}

pub fn qr_svg_path() -> Result<PathBuf> {
    Ok(runtime_dir()?.join("deck-pair.svg"))
}

fn random32() -> Result<[u8; 32]> {
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|e| Error::new("deck", format!("no randomness: {e}")))?;
    Ok(b)
}

struct Pairing {
    psk: [u8; 32],
    expires: Instant,
}

struct Shared {
    key: Keypair,
    port: u16,
    loopback: bool,
    pairing: Mutex<Option<Pairing>>,
    devices: Mutex<(Option<SystemTime>, pair::Devices)>,
    connected: Mutex<Vec<String>>,
    sessions: AtomicUsize,
    /// Bumped when deck.toml is reloaded: sessions resend their hello.
    deck_gen: AtomicUsize,
    state: Mutex<State>,
    deck: Mutex<Deck>,
}

impl Shared {
    /// Re-reads deck-devices.toml when it changed on disk (unpair from the
    /// CLI takes effect within a tick).
    fn refresh_devices(&self) {
        let mtime = pair::devices_path()
            .ok()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());
        let mut d = self.devices.lock().unwrap_or_else(|e| e.into_inner());
        if d.0 != mtime {
            match pair::load_devices() {
                Ok(list) => *d = (mtime, list),
                Err(e) => log(&format!("devices: {}", e.message)),
            }
        }
    }

    fn device_name(&self, key: &str) -> Option<String> {
        let d = self.devices.lock().unwrap_or_else(|e| e.into_inner());
        d.1.devices
            .iter()
            .find(|x| x.key == key)
            .map(|x| x.name.clone())
    }

    /// One attempt per window: taking the PSK burns it.
    fn take_psk(&self) -> Option<[u8; 32]> {
        let p = self
            .pairing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()?;
        let _ = qr_svg_path().map(std::fs::remove_file);
        (p.expires > Instant::now()).then_some(p.psk)
    }

    fn hosts(&self) -> Vec<String> {
        if self.loopback {
            vec!["127.0.0.1".into()]
        } else {
            let mut h = pair::local_hosts();
            if h.is_empty() {
                h.push("127.0.0.1".into());
            }
            h
        }
    }

    fn hello(&self) -> ServerMsg {
        let deck = self.deck.lock().unwrap_or_else(|e| e.into_inner());
        ServerMsg::Hello {
            schema: SCHEMA,
            name: pair::pc_name(),
            hosts: self.hosts(),
            port: self.port,
            buttons: deck.buttons(),
            state: self.state.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        }
    }

    fn open_pairing(&self) -> Result<(String, u64)> {
        let psk = random32()?;
        let q = QrPayload {
            hosts: self.hosts(),
            port: self.port,
            pc_public: self.key.public.clone(),
            psk,
            name: pair::pc_name(),
        };
        let text = pair::format(&q);
        store::ensure_private_dir(&runtime_dir()?)?;
        store::atomic_write(&qr_svg_path()?, pair::render_svg(&text)?.as_bytes(), false)?;
        *self.pairing.lock().unwrap_or_else(|e| e.into_inner()) = Some(Pairing {
            psk,
            expires: Instant::now() + Duration::from_secs(pair::PAIR_WINDOW_S),
        });
        Ok((text, pair::PAIR_WINDOW_S * 1000))
    }
}

/// Sliding one-minute windows per peer IP and overall.
#[derive(Default)]
struct RateLimit {
    per_ip: HashMap<IpAddr, VecDeque<Instant>>,
    all: VecDeque<Instant>,
}

impl RateLimit {
    fn allow(&mut self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let minute = Duration::from_secs(60);
        let prune = |q: &mut VecDeque<Instant>| {
            while q.front().is_some_and(|t| now.duration_since(*t) > minute) {
                q.pop_front();
            }
        };
        prune(&mut self.all);
        self.per_ip.retain(|_, q| {
            prune(q);
            !q.is_empty()
        });
        let q = self.per_ip.entry(ip).or_default();
        if q.len() >= PER_IP_PER_MIN || self.all.len() >= GLOBAL_PER_MIN {
            return false;
        }
        q.push_back(now);
        self.all.push_back(now);
        true
    }
}

pub struct ServeOpts {
    pub port: u16,
    /// Listen on 127.0.0.1 only (USB through `adb reverse`).
    pub loopback: bool,
}

pub fn serve(opts: ServeOpts) -> Result<()> {
    let state_dir = crate::store::state_dir()?;
    store::ensure_private_dir(&state_dir)?;
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(state_dir.join("deck.lock"))
        .map_err(|e| Error::new("io", format!("deck lock: {e}")))?;
    if lock.try_lock().is_err() {
        return Err(Error::new(
            "running",
            "`stance deck serve` is already running",
        ));
    }

    let addr: SocketAddr = if opts.loopback {
        ([127, 0, 0, 1], opts.port).into()
    } else {
        ([0, 0, 0, 0], opts.port).into()
    };
    let listener =
        TcpListener::bind(addr).map_err(|e| Error::new("deck", format!("listen {addr}: {e}")))?;
    let shared = Arc::new(Shared {
        key: pair::pc_keypair()?,
        port: listener.local_addr().map(|a| a.port()).unwrap_or(opts.port),
        loopback: opts.loopback,
        pairing: Mutex::new(None),
        devices: Mutex::new((None, pair::Devices::default())),
        connected: Mutex::new(Vec::new()),
        sessions: AtomicUsize::new(0),
        deck_gen: AtomicUsize::new(0),
        state: Mutex::new(State::default()),
        deck: Mutex::new(Deck::load()?),
    });
    shared.refresh_devices();
    log(&format!(
        "listening on {addr} (fingerprint {}), {} paired device(s)",
        noise::fingerprint(&shared.key.public),
        shared
            .devices
            .lock()
            .map(|d| d.1.devices.len())
            .unwrap_or(0)
    ));

    // State: one snapshot for every session, refreshed on a tick.
    {
        let sh = shared.clone();
        std::thread::spawn(move || {
            loop {
                sh.refresh_devices();
                {
                    let mut deck = sh.deck.lock().unwrap_or_else(|e| e.into_inner());
                    if deck.changed_on_disk() {
                        match Deck::load() {
                            Ok(d) => {
                                *deck = d;
                                sh.deck_gen.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(e) => log(&format!("deck.toml: {}", e.message)),
                        }
                    }
                }
                // Only poll when someone is listening.
                if sh.sessions.load(Ordering::Relaxed) > 0 {
                    let snap = sh.deck.lock().unwrap_or_else(|e| e.into_inner()).snapshot();
                    *sh.state.lock().unwrap_or_else(|e| e.into_inner()) = snap;
                }
                let expired = {
                    let mut p = sh.pairing.lock().unwrap_or_else(|e| e.into_inner());
                    let gone = p.as_ref().is_some_and(|x| x.expires <= Instant::now());
                    if gone {
                        *p = None;
                    }
                    gone
                };
                if expired {
                    let _ = qr_svg_path().map(std::fs::remove_file);
                }
                std::thread::sleep(TICK);
            }
        });
    }

    // Local control socket.
    {
        let sh = shared.clone();
        let path = control_path()?;
        store::ensure_private_dir(&runtime_dir()?)?;
        if let Ok(m) = std::fs::symlink_metadata(&path) {
            if !m.file_type().is_symlink() {
                let _ = std::fs::remove_file(&path);
            } else {
                return Err(Error::new("unsafe_path", "deck.sock is a symlink"));
            }
        }
        let ul = UnixListener::bind(&path)
            .map_err(|e| Error::new("deck", format!("control socket: {e}")))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| Error::new("io", e.to_string()))?;
        std::thread::spawn(move || {
            for c in ul.incoming().flatten() {
                if let Err(e) = control(&sh, c) {
                    log(&format!("control: {}", e.message));
                }
            }
        });
    }

    let limit = Mutex::new(RateLimit::default());
    for conn in listener.incoming() {
        let Ok(stream) = conn else { continue };
        let Ok(peer) = stream.peer_addr() else {
            continue;
        };
        if shared.sessions.load(Ordering::Relaxed) >= MAX_SESSIONS
            || !limit
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .allow(peer.ip())
        {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            continue;
        }
        let sh = shared.clone();
        sh.sessions.fetch_add(1, Ordering::Relaxed);
        std::thread::spawn(move || {
            if let Err(e) = connection(&sh, stream)
                && e.code != "deck_closed"
            {
                log(&format!("{}: {}", peer.ip(), e.message));
            }
            sh.sessions.fetch_sub(1, Ordering::Relaxed);
        });
    }
    Ok(())
}

fn control(sh: &Shared, c: UnixStream) -> Result<()> {
    // Only our own user may talk to it (the dir is 0700 too).
    let uid = crate::hypr::current_uid()?;
    let peer_uid = peer_uid(&c)?;
    if peer_uid != uid {
        return Err(Error::new("deck", "control connection from another user"));
    }
    c.set_read_timeout(Some(Duration::from_secs(2))).ok();
    let mut line = String::new();
    BufReader::new(c.try_clone().map_err(|e| Error::new("io", e.to_string()))?)
        .take(4096)
        .read_line(&mut line)
        .map_err(|e| Error::new("io", e.to_string()))?;
    let req: serde_json::Value =
        serde_json::from_str(&line).map_err(|_| Error::new("deck", "bad control request"))?;
    let reply = match req.get("cmd").and_then(|v| v.as_str()) {
        Some("pair") => match sh.open_pairing() {
            Ok((payload, ms)) => json!({
                "ok": true,
                "payload": payload,
                "expires_ms": ms,
                "svg": qr_svg_path().ok(),
                "fingerprint": noise::fingerprint(&sh.key.public),
            }),
            Err(e) => json!({ "ok": false, "error": e.message }),
        },
        Some("status") => {
            let connected = sh
                .connected
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let devices: Vec<_> = sh
                .devices
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .1
                .devices
                .iter()
                .map(|d| json!({ "name": d.name, "paired_at": d.paired_at, "connected": connected.contains(&d.name) }))
                .collect();
            json!({
                "ok": true,
                "port": sh.port,
                "loopback": sh.loopback,
                "hosts": sh.hosts(),
                "fingerprint": noise::fingerprint(&sh.key.public),
                "pairing": sh.pairing.lock().map(|p| p.is_some()).unwrap_or(false),
                "devices": devices,
            })
        }
        _ => json!({ "ok": false, "error": "unknown command" }),
    };
    let mut c = c;
    writeln!(c, "{reply}").map_err(|e| Error::new("io", e.to_string()))
}

fn peer_uid(c: &UnixStream) -> Result<u32> {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: valid fd, out-pointers to a correctly sized ucred.
    let r = unsafe {
        libc::getsockopt(
            c.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if r != 0 {
        return Err(Error::new("deck", "cannot read peer credentials"));
    }
    Ok(cred.uid)
}

/// Asks the running `serve` over the control socket.
pub fn ask(cmd: &str) -> Result<serde_json::Value> {
    let path = control_path()?;
    let mut c = UnixStream::connect(&path).map_err(|_| {
        Error::new(
            "deck_not_running",
            "`stance deck serve` is not running (start it, or enable stance-deck.service)",
        )
    })?;
    c.set_read_timeout(Some(Duration::from_secs(3))).ok();
    writeln!(c, "{}", json!({ "cmd": cmd })).map_err(|e| Error::new("io", e.to_string()))?;
    let mut line = String::new();
    BufReader::new(c)
        .take(64 * 1024)
        .read_line(&mut line)
        .map_err(|e| Error::new("io", e.to_string()))?;
    serde_json::from_str(&line).map_err(|_| Error::new("deck", "bad reply from deck serve"))
}

fn connection(sh: &Shared, stream: TcpStream) -> Result<()> {
    stream.set_nodelay(true).ok();
    let inc = noise::accept(stream, &sh.key, &|k| sh.device_name(k).is_some(), &|| {
        sh.take_psk()
    })?;
    let (mut s, name, key) = match inc {
        Incoming::Device(s, key) => {
            let name = sh.device_name(&key).unwrap_or_default();
            (s, name, key)
        }
        Incoming::Paired(mut s, key) => {
            let name = match s.recv::<ClientMsg>(PAIR_MSG_TIMEOUT)? {
                Some(ClientMsg::Pair { name }) => name,
                _ => return Err(Error::new("deck_proto", "expected a pair message")),
            };
            let dev = pair::add_device(&name, &key)?;
            sh.refresh_devices();
            log(&format!("paired `{}`", dev.name));
            (s, dev.name, key)
        }
    };
    sh.connected
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(name.clone());
    let res = session(sh, &mut s, &key);
    s.shutdown();
    sh.connected
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|n| n != &name);
    res
}

fn session(sh: &Shared, s: &mut Session, key: &str) -> Result<()> {
    // First snapshot right away instead of waiting for the tick.
    {
        let snap = sh.deck.lock().unwrap_or_else(|e| e.into_inner()).snapshot();
        *sh.state.lock().unwrap_or_else(|e| e.into_inner()) = snap;
    }
    s.send(&sh.hello())?;
    let mut sent_version = sh.state.lock().map(|st| st.version).unwrap_or(0);
    let mut deck_gen = sh.deck_gen.load(Ordering::Relaxed);
    let mut presses: VecDeque<Instant> = VecDeque::new();
    loop {
        if sh.device_name(key).is_none() {
            let _ = s.send(&ServerMsg::Bye {
                reason: "unpaired".into(),
            });
            return Ok(());
        }
        let msg = s.recv::<ClientMsg>(TICK)?;
        match msg {
            None => {}
            Some(ClientMsg::Bye) => return Ok(()),
            Some(ClientMsg::Ping { req }) => s.send(&ServerMsg::Pong { req })?,
            Some(ClientMsg::Pair { .. }) => return Err(Error::new("deck_proto", "already paired")),
            Some(
                m
                @ (ClientMsg::Press { .. } | ClientMsg::Confirm { .. } | ClientMsg::Revert { .. }),
            ) => {
                let now = Instant::now();
                while presses
                    .front()
                    .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(1))
                {
                    presses.pop_front();
                }
                let req = match &m {
                    ClientMsg::Press { req, .. }
                    | ClientMsg::Confirm { req }
                    | ClientMsg::Revert { req } => *req,
                    _ => 0,
                };
                if presses.len() >= PRESSES_PER_S {
                    s.send(&ServerMsg::Ack {
                        req,
                        ok: false,
                        error: Some("too fast".into()),
                    })?;
                    continue;
                }
                presses.push_back(now);
                s.send(&ServerMsg::Ack {
                    req,
                    ok: true,
                    error: None,
                })?;
                let res = {
                    let deck = sh.deck.lock().unwrap_or_else(|e| e.into_inner());
                    match &m {
                        ClientMsg::Press { id, .. } => deck.execute(id),
                        ClientMsg::Confirm { .. } => deck.execute_builtin("confirm"),
                        _ => deck.execute_builtin("revert"),
                    }
                };
                let reply = match res {
                    Ok(message) => ServerMsg::Result {
                        req,
                        ok: true,
                        message: Some(message),
                        error: None,
                    },
                    Err(e) => ServerMsg::Result {
                        req,
                        ok: false,
                        message: None,
                        error: Some(e.message),
                    },
                };
                s.send(&reply)?;
                // Reflect the change without waiting for the tick.
                let snap = sh.deck.lock().unwrap_or_else(|e| e.into_inner()).snapshot();
                *sh.state.lock().unwrap_or_else(|e| e.into_inner()) = snap;
            }
        }
        let g = sh.deck_gen.load(Ordering::Relaxed);
        if g != deck_gen {
            deck_gen = g;
            s.send(&sh.hello())?;
        }
        let st = sh.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if st.version != sent_version {
            sent_version = st.version;
            s.send(&ServerMsg::State { state: st })?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_windows() {
        let mut r = RateLimit::default();
        let ip: IpAddr = [10, 0, 0, 1].into();
        for _ in 0..PER_IP_PER_MIN {
            assert!(r.allow(ip));
        }
        assert!(!r.allow(ip));
        assert!(r.allow([10, 0, 0, 2].into()));
    }
}
