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
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime};

use serde_json::json;

use super::actions::Deck;
use super::link::Link;
use super::noise::{self, Incoming, Keypair, Session};
use super::pair::{self, QrPayload};
use super::proto::{self, ClientMsg, PROTO, Plain, SCHEMA, ServerMsg, State};
use super::stream::Streams;
use super::transfer::{self, Hub};
use crate::error::{Error, Result};
use crate::store;

const MAX_SESSIONS: usize = 8;
/// Connections still in their handshake; kept apart from sessions so idle
/// strangers cannot take the slots of paired phones.
const MAX_HANDSHAKES: usize = 4;
/// Failed handshakes per minute: per IP, and overall (the overall limit
/// only blocks IPs that failed themselves, so a flood from elsewhere never
/// locks out a paired phone).
const FAILS_PER_IP: usize = 10;
const FAILS_GLOBAL: usize = 40;
const PRESSES_PER_S: usize = 10;
const PAIR_MSG_TIMEOUT: Duration = Duration::from_secs(10);
const TICK: Duration = Duration::from_millis(500);

fn log(msg: &str) {
    eprintln!("stance deck: {msg}");
}

/// `$XDG_RUNTIME_DIR/stance`, or `STANCE_DECK_DIR` (absolute) so a second
/// server (tests) can run next to the real one.
pub fn runtime_dir() -> Result<PathBuf> {
    match std::env::var_os("STANCE_DECK_DIR").map(PathBuf::from) {
        Some(p) if p.is_absolute() => Ok(p),
        _ => Ok(crate::hypr::runtime_dir()?.join("stance")),
    }
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
    handshakes: AtomicUsize,
    limit: Mutex<RateLimit>,
    /// Device name → last connection (unix seconds), kept in the state dir.
    seen: Mutex<HashMap<String, u64>>,
    /// Bumped when deck.toml is reloaded: sessions resend their hello.
    deck_gen: AtomicUsize,
    state: Mutex<State>,
    deck: Mutex<Deck>,
    /// Transfers (docs/adr/0004): settings, consent, queue, history.
    hub: Arc<Hub>,
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
            proto: PROTO,
            caps: self.hub.settings().offered_caps(),
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

/// Sliding one-minute windows of failed handshakes, per peer IP and overall.
#[derive(Default)]
struct RateLimit {
    per_ip: HashMap<IpAddr, VecDeque<Instant>>,
    all: VecDeque<Instant>,
}

impl RateLimit {
    fn prune(&mut self) {
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
    }

    fn allow(&mut self, ip: IpAddr) -> bool {
        self.prune();
        let mine = self.per_ip.get(&ip).map_or(0, VecDeque::len);
        mine < FAILS_PER_IP && (mine == 0 || self.all.len() < FAILS_GLOBAL)
    }

    fn fail(&mut self, ip: IpAddr) {
        let now = Instant::now();
        self.per_ip.entry(ip).or_default().push_back(now);
        self.all.push_back(now);
        self.prune();
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
        handshakes: AtomicUsize::new(0),
        limit: Mutex::new(RateLimit::default()),
        seen: Mutex::new(load_seen()),
        deck_gen: AtomicUsize::new(0),
        state: Mutex::new(State::default()),
        deck: Mutex::new(Deck::load()?),
        hub: Arc::new(Hub::new()),
    });
    shared.refresh_devices();
    // Temp files a crash left in the download folder.
    if let Ok(dir) = shared.hub.settings().download_dir()
        && dir.is_dir()
    {
        transfer::sweep(&dir);
    }
    log(&format!(
        "listening on {} (fingerprint {}), {} paired device(s)",
        listener
            .local_addr()
            .map_or_else(|_| addr.to_string(), |a| a.to_string()),
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
                {
                    let connected = sh.connected.lock().map(|c| c.clone()).unwrap_or_default();
                    sh.hub.expire_queue(&connected, QUEUE_WAIT_S);
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
        // sockaddr_un holds 107 bytes of path.
        if path.as_os_str().len() > 100 {
            return Err(Error::new(
                "deck",
                format!(
                    "control socket path is too long ({}): use a shorter XDG_RUNTIME_DIR",
                    path.display()
                ),
            ));
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

    for conn in listener.incoming() {
        let Ok(stream) = conn else { continue };
        let Ok(peer) = stream.peer_addr() else {
            continue;
        };
        if shared.handshakes.load(Ordering::Relaxed) >= MAX_HANDSHAKES
            || !shared
                .limit
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .allow(peer.ip())
        {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            continue;
        }
        let sh = shared.clone();
        sh.handshakes.fetch_add(1, Ordering::Relaxed);
        std::thread::spawn(move || {
            if let Err(e) = connection(&sh, stream, peer.ip())
                && e.code != "deck_closed"
            {
                log(&format!("{}: {}", peer.ip(), e.message));
            }
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
                .map(|d| {
                    let last_seen = sh.seen.lock().ok().and_then(|m| m.get(&d.name).copied());
                    json!({ "name": d.name, "paired_at": d.paired_at, "connected": connected.contains(&d.name), "last_seen": last_seen })
                })
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
        Some("transfers") => {
            let connected = sh.connected.lock().map(|c| c.clone()).unwrap_or_default();
            sh.hub.expire_queue(&connected, QUEUE_WAIT_S);
            json!({ "ok": true, "transfers": sh.hub.list(), "pending": sh.hub.consent_pending().map(|c| json!({
                "id": c.id, "device": c.device, "name": c.name, "size": c.size,
            })) })
        }
        Some("answer") => {
            let id = req.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
            let yes = req.get("accept").and_then(|v| v.as_bool()).unwrap_or(false);
            if sh.hub.consent_answer(id, yes) {
                json!({ "ok": true })
            } else {
                json!({ "ok": false, "error": "no such pending transfer" })
            }
        }
        Some("send") => {
            let path = req.get("path").and_then(|v| v.as_str()).map(PathBuf::from);
            let to = req.get("to").and_then(|v| v.as_str()).map(str::to_string);
            let size = req.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            match path {
                Some(p) if p.is_absolute() && p.as_os_str().len() < 4096 => {
                    let name = transfer::clean_name(
                        &p.file_name()
                            .map(|n| n.to_string_lossy())
                            .unwrap_or_default(),
                    );
                    let dev = to.clone().unwrap_or_else(|| "any phone".into());
                    let id =
                        sh.hub
                            .new_transfer("to_phone", &dev, &name, size, transfer::State::Queued);
                    sh.hub.queue(transfer::Outgoing { id, to, path: p });
                    json!({ "ok": true, "id": id })
                }
                _ => json!({ "ok": false, "error": "send needs an absolute path" }),
            }
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
    ask_with(json!({ "cmd": cmd }))
}

/// A control request with arguments (`{"cmd": ..., ...}`).
pub fn ask_with(req: serde_json::Value) -> Result<serde_json::Value> {
    let path = control_path()?;
    let mut c = UnixStream::connect(&path).map_err(|_| {
        Error::new(
            "deck_not_running",
            "`stance deck serve` is not running (start it, or enable stance-deck.service)",
        )
    })?;
    c.set_read_timeout(Some(Duration::from_secs(3))).ok();
    writeln!(c, "{req}").map_err(|e| Error::new("io", e.to_string()))?;
    let mut line = String::new();
    BufReader::new(c)
        .take(64 * 1024)
        .read_line(&mut line)
        .map_err(|e| Error::new("io", e.to_string()))?;
    serde_json::from_str(&line).map_err(|_| Error::new("deck", "bad reply from deck serve"))
}

fn connection(sh: &Shared, stream: TcpStream, ip: IpAddr) -> Result<()> {
    stream.set_nodelay(true).ok();
    let inc = noise::accept(stream, &sh.key, &|k| sh.device_name(k).is_some(), &|| {
        sh.take_psk()
    });
    sh.handshakes.fetch_sub(1, Ordering::Relaxed);
    let inc = match inc {
        Ok(i) => i,
        Err(e) => {
            sh.limit.lock().unwrap_or_else(|e| e.into_inner()).fail(ip);
            return Err(e);
        }
    };
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
            notify_paired(&dev.name, &key);
            (s, dev.name, key)
        }
    };
    if sh.sessions.fetch_add(1, Ordering::Relaxed) >= MAX_SESSIONS {
        sh.sessions.fetch_sub(1, Ordering::Relaxed);
        let _ = s.send(&ServerMsg::Bye {
            reason: "too many phones connected".into(),
        });
        return Ok(());
    }
    sh.mark_seen(&name);
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
    sh.mark_seen(&name);
    sh.sessions.fetch_sub(1, Ordering::Relaxed);
    res
}

/// A desktop notice for every new pairing, so a pairing nobody expected
/// (a photographed QR) does not go unnoticed.
fn notify_paired(name: &str, key: &str) {
    let Some(tool) = crate::tool::find("notify-send") else {
        return;
    };
    let body = format!(
        "{name} (key {}). Not you? Unpair it in the panel (Settings, Phone) or with: stance deck unpair \"{name}\"",
        noise::fingerprint(key)
    );
    std::thread::spawn(move || {
        let _ = crate::tool::run_tool(
            &tool,
            &["-a", "stance", "stance: phone paired", &body],
            Duration::from_secs(3),
        );
    });
}

fn seen_path() -> Option<PathBuf> {
    crate::store::state_dir()
        .ok()
        .map(|d| d.join("deck-seen.json"))
}

fn load_seen() -> HashMap<String, u64> {
    seen_path()
        .and_then(|p| store::read_checked(&p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

impl Shared {
    fn mark_seen(&self, name: &str) {
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        seen.insert(name.to_string(), crate::apply::now_ms() / 1000);
        // Forget devices that were unpaired.
        let known: Vec<String> = self
            .devices
            .lock()
            .map(|d| d.1.devices.iter().map(|x| x.name.clone()).collect())
            .unwrap_or_default();
        seen.retain(|n, _| known.contains(n));
        if let (Some(p), Ok(data)) = (seen_path(), serde_json::to_vec(&*seen)) {
            let _ = store::atomic_write(&p, &data, false);
        }
    }
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
    // Protocol v2 (docs/adr/0003): set once the client sent `caps`. Before
    // that, stream messages and data frames end the session, and the PC
    // never sends any (an old app treats them as fatal).
    let mut v2: Option<(Streams, Link)> = None;
    // Button actions run on a worker (one at a time, in order), so the loop
    // keeps reading while an action or its subprocess takes its time.
    let r = std::thread::scope(|sc| {
        let (job_tx, job_rx) = mpsc::channel::<(u32, Job)>();
        let (res_tx, res_rx) = mpsc::channel::<(u32, Result<String>)>();
        sc.spawn(move || {
            for (req, job) in job_rx {
                // The deck lock covers looking the command up, not running
                // it: other sessions keep going during a slow action.
                let prepared = {
                    let deck = sh.deck.lock().unwrap_or_else(|e| e.into_inner());
                    match &job {
                        Job::Press(id) => deck.prepare(id),
                        Job::Builtin(b) => deck.prepare_builtin(b),
                    }
                };
                let res = prepared.and_then(|p| p.run());
                sh.deck
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .mark_stale();
                if res_tx.send((req, res)).is_err() {
                    break;
                }
            }
        });
        let mut jobs = Jobs {
            tx: job_tx,
            rx: res_rx,
            open: 0,
        };
        session_loop(
            sh,
            s,
            key,
            &mut v2,
            &mut presses,
            &mut sent_version,
            &mut deck_gen,
            &mut jobs,
        )
        // Dropping `jobs` ends the worker after its current action.
    });
    // Every open stream failed with the session: temp files go, prompts close.
    if let Some((st, link)) = v2.as_mut() {
        let outs = st.close_all();
        link.close(&sh.hub, outs);
    }
    r
}

enum Job {
    Press(String),
    Builtin(&'static str),
}

/// The session's side of the action worker.
struct Jobs {
    tx: mpsc::Sender<(u32, Job)>,
    rx: mpsc::Receiver<(u32, Result<String>)>,
    /// Sent and not answered yet.
    open: usize,
}

/// A file queued for a phone that is not connected fails after this long.
const QUEUE_WAIT_S: u64 = 120;

/// Poll quickly while something is in flight.
const BUSY_TICK: Duration = Duration::from_millis(100);
/// Barely wait while data is ready to go out.
const SEND_TICK: Duration = Duration::from_millis(1);

#[allow(clippy::too_many_arguments)]
fn session_loop(
    sh: &Shared,
    s: &mut Session,
    key: &str,
    v2: &mut Option<(Streams, Link)>,
    presses: &mut VecDeque<Instant>,
    sent_version: &mut u64,
    deck_gen: &mut usize,
    jobs: &mut Jobs,
) -> Result<()> {
    loop {
        if sh.device_name(key).is_none() {
            let _ = s.send(&ServerMsg::Bye {
                reason: "unpaired".into(),
            });
            return Ok(());
        }
        let now = Instant::now();
        if let Some((st, link)) = v2.as_mut() {
            let outs = st.tick(now);
            link.apply(&sh.hub, st, s, outs)?;
            if !link.tick(&sh.hub, st, s, now)? {
                let outs = st.close_all();
                link.close(&sh.hub, outs);
                let _ = s.send(&ServerMsg::Bye {
                    reason: "settings changed".into(),
                });
                return Ok(());
            }
        }
        // Finished actions.
        while let Ok((req, res)) = jobs.rx.try_recv() {
            jobs.open = jobs.open.saturating_sub(1);
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
        let busy = jobs.open > 0 || v2.as_ref().is_some_and(|(st, l)| st.open() > 0 || l.busy());
        let sending = v2.as_ref().is_some_and(|(st, l)| l.can_send(st));
        let wait = if sending {
            SEND_TICK
        } else if busy {
            BUSY_TICK
        } else {
            TICK
        };
        let msg = match s.recv_any::<ClientMsg>(wait)? {
            None => None,
            Some(Plain::Msg(m)) => Some(m),
            Some(Plain::Stream(m)) => {
                let (st, link) = v2
                    .as_mut()
                    .ok_or_else(|| Error::new("deck_proto", "stream message before caps"))?;
                link.peer_msg(&sh.hub, &m);
                let sid = match &m {
                    proto::StreamMsg::Offer { sid, .. } => *sid,
                    _ => 0,
                };
                let outs = st
                    .on_msg(m, now, |kind, size, name| {
                        link.decide(&sh.hub, sid, kind, size, name)
                    })
                    .map_err(|e| Error::new("deck_proto", e))?;
                link.apply(&sh.hub, st, s, outs)?;
                None
            }
            Some(Plain::Data { sid, payload }) => {
                let (st, link) = v2
                    .as_mut()
                    .ok_or_else(|| Error::new("deck_proto", "data frame before caps"))?;
                let outs = st
                    .on_data(sid, payload, now)
                    .map_err(|e| Error::new("deck_proto", e))?;
                link.apply(&sh.hub, st, s, outs)?;
                None
            }
        };
        match msg {
            None => {}
            Some(ClientMsg::Caps { proto: p, caps }) => {
                if v2.is_some() || p < 2 {
                    return Err(Error::new("deck_proto", "caps sent twice or for proto 1"));
                }
                let offered = sh.hub.settings().offered_caps();
                let agreed = proto::agree_caps(&offered, &caps);
                s.send(&ServerMsg::Caps {
                    proto: PROTO,
                    caps: agreed.clone(),
                })?;
                let device = sh.device_name(key).unwrap_or_default();
                *v2 = Some((
                    Streams::new(agreed.clone(), true),
                    Link::new(&device, offered, agreed),
                ));
            }
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
                let job = match m {
                    ClientMsg::Press { id, .. } => Job::Press(id),
                    ClientMsg::Confirm { .. } => Job::Builtin("confirm"),
                    _ => Job::Builtin("revert"),
                };
                jobs.tx
                    .send((req, job))
                    .map_err(|_| Error::new("deck", "action worker stopped"))?;
                jobs.open += 1;
            }
        }
        let g = sh.deck_gen.load(Ordering::Relaxed);
        if g != *deck_gen {
            *deck_gen = g;
            s.send(&sh.hello())?;
        }
        let st = sh.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if st.version != *sent_version {
            *sent_version = st.version;
            s.send(&ServerMsg::State { state: st })?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_counts_failures_only() {
        let mut r = RateLimit::default();
        let bad: IpAddr = [10, 0, 0, 1].into();
        let phone: IpAddr = [10, 0, 0, 9].into();
        for _ in 0..FAILS_PER_IP {
            assert!(r.allow(bad));
            r.fail(bad);
        }
        assert!(!r.allow(bad));
        // A global flood of failures from many IPs...
        for i in 0..FAILS_GLOBAL {
            r.fail([10, 1, (i / 200) as u8, (i % 200) as u8].into());
        }
        // ...never locks out an IP that has not failed.
        assert!(r.allow(phone));
        r.fail(phone);
        assert!(!r.allow(phone));
    }
}
