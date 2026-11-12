//! Clipboard text and files between the PC and paired phones (protocol v2,
//! docs/adr/0004-phone-transfers.md): settings (`transfer.toml`), file name
//! cleaning, incoming files (temp file next to the target, linked into
//! place, never overwriting), the server-wide state every session shares
//! (`Hub`: settings snapshot, echo guard, the single pending consent, the
//! transfer list, files queued for phones) and the external tools
//! (`wl-copy`, `notify-send`), always by argv with a timeout.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::store;

/// Longest cleaned file name, in bytes.
pub const NAME_BYTES: usize = 120;
/// Longest extension kept when a name is cut.
const EXT_BYTES: usize = 16;
/// Free space left on the download filesystem after a file.
pub const FREE_MARGIN: u64 = 64 << 20;
/// Incoming file data in flight, server-wide.
pub const INCOMING_MAX: u64 = 1 << 30;
/// Hashes of text that came from a phone are not sent back for this long.
const ECHO_TTL: Duration = Duration::from_secs(30);
/// Declined or unanswered file offers before a device has to wait.
const COOLDOWN_AFTER: u32 = 3;
const COOLDOWN: Duration = Duration::from_secs(300);
/// Transfers remembered for `stance deck transfers`.
const HISTORY: usize = 20;
const PART_PREFIX: &str = ".stance-part-";

// ---- settings -------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClipFrom {
    #[default]
    Off,
    /// Applied without a prompt, with a notification showing a preview.
    Notify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FilesFrom {
    #[default]
    Off,
    /// Every file asks (notification or `stance deck transfer accept`).
    Ask,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(default = "schema_one")]
    pub schema: u32,
    #[serde(default)]
    pub clip_to_phone: bool,
    #[serde(default)]
    pub clip_from_phone: ClipFrom,
    #[serde(default)]
    pub files_from_phone: FilesFrom,
    /// Absolute, or starting with `~/`. Empty = `~/Downloads/stance`.
    #[serde(default)]
    pub download_dir: String,
    #[serde(default = "max_mb_default")]
    pub max_file_mb: u64,
    /// Clipboard images too (PNG, JPEG, WebP, GIF; up to 32 MiB), both ways
    /// as the two clip switches allow.
    #[serde(default)]
    pub clip_images: bool,
    /// Phones whose files are saved without asking (still announced).
    #[serde(default)]
    pub trusted_devices: Vec<String>,
}

fn schema_one() -> u32 {
    1
}
fn max_mb_default() -> u64 {
    512
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: 1,
            clip_to_phone: false,
            clip_from_phone: ClipFrom::Off,
            files_from_phone: FilesFrom::Off,
            download_dir: String::new(),
            max_file_mb: max_mb_default(),
            clip_images: false,
            trusted_devices: Vec::new(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        if !(1..=512).contains(&self.max_file_mb) {
            return Err(Error::new("usage", "max_file_mb must be 1..512"));
        }
        if self.trusted_devices.len() > 16
            || self
                .trusted_devices
                .iter()
                .any(|d| super::proto::clean_name(d).as_deref() != Some(d.as_str()))
        {
            return Err(Error::new(
                "usage",
                "trusted_devices: at most 16 device names as paired",
            ));
        }
        let d = &self.download_dir;
        if !d.is_empty() && !(d.starts_with('/') || d.starts_with("~/")) {
            return Err(Error::new(
                "usage",
                "download_dir must be absolute or start with ~/",
            ));
        }
        if d.len() > 4096 || d.chars().any(char::is_control) || d.split('/').any(|c| c == "..") {
            return Err(Error::new("usage", "download_dir is not a clean path"));
        }
        Ok(())
    }

    pub fn download_dir(&self) -> Result<PathBuf> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| Error::new("io", "HOME is not set"))?;
        Ok(match self.download_dir.as_str() {
            "" => home.join("Downloads/stance"),
            d if d.starts_with("~/") => home.join(&d[2..]),
            d => PathBuf::from(d),
        })
    }

    pub fn max_file(&self) -> u64 {
        self.max_file_mb << 20
    }

    /// Capabilities offered to a phone with these settings.
    pub fn offered_caps(&self) -> Vec<String> {
        let mut c = Vec::new();
        if self.clip_to_phone || self.clip_from_phone != ClipFrom::Off {
            c.push("clip.text".to_string());
            if self.clip_images {
                c.push("clip.image".into());
            }
        }
        if self.files_from_phone != FilesFrom::Off {
            c.push("file.push".into());
        }
        c.push("file.pull".into());
        c
    }
}

pub fn settings_path() -> Result<PathBuf> {
    Ok(store::config_dir()?.join("transfer.toml"))
}

/// Missing file = defaults (all off). Anything wrong = all off, with the
/// reason (the server logs it).
pub fn load_settings() -> (Settings, Option<String>) {
    let Ok(path) = settings_path() else {
        return (Settings::default(), Some("no config dir".into()));
    };
    match fs::read_to_string(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Settings::default(), None),
        Err(e) => (Settings::default(), Some(format!("transfer.toml: {e}"))),
        Ok(text) => match parse_settings(&text) {
            Ok(s) => (s, None),
            Err(e) => (
                Settings::default(),
                Some(format!("transfer.toml: {}", e.message)),
            ),
        },
    }
}

pub fn parse_settings(text: &str) -> Result<Settings> {
    let s: Settings = toml::from_str(text).map_err(|e| Error::new("usage", e.to_string()))?;
    if s.schema > crate::migrate::CURRENT {
        return Err(crate::migrate::newer("transfer.toml", s.schema));
    }
    s.validate()?;
    Ok(s)
}

/// Drops `device` from `trusted_devices` (unpaired, or paired again with
/// another key). Leaves the file alone when it is not there.
pub fn untrust(device: &str) {
    let (s, warn) = load_settings();
    if warn.is_some() || !s.trusted_devices.iter().any(|d| d == device) {
        return;
    }
    let mut s = s;
    s.trusted_devices.retain(|d| d != device);
    if let Ok(json) = serde_json::to_string(&s)
        && let Err(e) = put_settings(&json)
    {
        eprintln!("stance: transfer.toml: {}", e.message);
    }
}

/// Validates JSON from the panel / CLI and writes `transfer.toml`.
pub fn put_settings(json: &str) -> Result<Settings> {
    let mut s: Settings = serde_json::from_str(json)
        .map_err(|e| Error::new("usage", format!("bad settings: {e}")))?;
    s.validate()?;
    let path = settings_path()?;
    if let Ok(old) = fs::read_to_string(&path)
        && crate::migrate::toml_schema(&old) > crate::migrate::CURRENT
    {
        return Err(crate::migrate::newer(
            "transfer.toml",
            crate::migrate::toml_schema(&old),
        ));
    }
    s.schema = crate::migrate::CURRENT;
    let text = toml::to_string(&s).map_err(|e| Error::new("io", e.to_string()))?;
    store::atomic_write(&path, text.as_bytes(), false)?;
    Ok(s)
}

// ---- names and files ------------------------------------------------------

/// A file name from a phone, safe to create in the download folder.
pub fn clean_name(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let kept: String = last
        .chars()
        .filter(|c| !c.is_control() && !crate::clip::is_invisible(*c))
        .collect();
    let t = kept
        .trim_start_matches(['.', '-', ' '])
        .trim_end_matches(['.', ' '])
        .to_string();
    let t = if t.is_empty() { "file".to_string() } else { t };
    if t.len() <= NAME_BYTES {
        return t;
    }
    // Keep a short extension, cut the stem on a char boundary.
    let (stem, ext) = match t.rfind('.') {
        Some(i) if i > 0 && t.len() - i <= EXT_BYTES + 1 => (&t[..i], &t[i..]),
        _ => (t.as_str(), ""),
    };
    let mut cut = NAME_BYTES - ext.len();
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{}", stem[..cut].trim_end_matches(['.', ' ']), ext)
}

/// Markup-free, one line, short: for notification bodies.
pub fn plain(s: &str, max: usize) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| !matches!(c, '<' | '>' | '&') && !crate::clip::is_invisible(*c))
        .collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= max {
        t
    } else {
        format!("{}…", t.chars().take(max).collect::<String>())
    }
}

pub fn human_size(n: u64) -> String {
    match n {
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.0} KB", n as f64 / 1024.0),
        n => format!("{n} B"),
    }
}

/// The download folder: created (0700) if missing; must be a directory
/// we own, not a symlink.
pub fn ensure_dir(dir: &Path) -> Result<()> {
    if fs::symlink_metadata(dir).is_err() {
        fs::create_dir_all(dir).map_err(|e| Error::new("io", format!("{}: {e}", dir.display())))?;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    let m = fs::symlink_metadata(dir).map_err(|e| Error::new("io", e.to_string()))?;
    if !m.is_dir() {
        return Err(Error::new(
            "io",
            format!("{} is not a directory", dir.display()),
        ));
    }
    if m.uid() != crate::hypr::current_uid()? {
        return Err(Error::new("io", format!("{} is not ours", dir.display())));
    }
    Ok(())
}

/// Deletes leftover temp files (a crash mid-transfer).
pub fn sweep(dir: &Path) {
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(PART_PREFIX) {
                let _ = fs::remove_file(e.path());
            }
        }
    }
}

pub fn free_space(dir: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: valid C string and a zeroed out-struct.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    Some(st.f_bavail as u64 * st.f_frsize as u64)
}

fn random_hex() -> String {
    let mut b = [0u8; 8];
    let _ = File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b));
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// An incoming file: a temp file in the target folder (same filesystem),
/// linked to its final name only after size and hash were checked.
#[derive(Debug)]
pub struct Part {
    file: File,
    tmp: PathBuf,
    dir: PathBuf,
    name: String,
}

impl Drop for Part {
    /// Unfinished or failed: the temp file goes (`finish` links it first).
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.tmp);
    }
}

impl Part {
    pub fn create(dir: &Path, name: &str) -> Result<Self> {
        let tmp = dir.join(format!("{PART_PREFIX}{}", random_hex()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)
            .map_err(|e| Error::new("io", format!("{}: {e}", tmp.display())))?;
        Ok(Self {
            file,
            tmp,
            dir: dir.to_path_buf(),
            name: clean_name(name),
        })
    }

    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        self.file
            .write_all(data)
            .map_err(|e| Error::new("io", e.to_string()))
    }

    /// Links the file under its name (` (2)`, ` (3)`… when taken; never
    /// overwrites) and removes the temp name. Returns the final path.
    pub fn finish(self) -> Result<PathBuf> {
        let r = (|| {
            self.file
                .sync_all()
                .map_err(|e| Error::new("io", e.to_string()))?;
            fs::set_permissions(&self.tmp, fs::Permissions::from_mode(0o644))
                .map_err(|e| Error::new("io", e.to_string()))?;
            let (stem, ext) = match self.name.rfind('.') {
                Some(i) if i > 0 => (&self.name[..i], &self.name[i..]),
                _ => (self.name.as_str(), ""),
            };
            for n in 1..1000 {
                let name = if n == 1 {
                    self.name.clone()
                } else {
                    format!("{stem} ({n}){ext}")
                };
                let target = self.dir.join(name);
                match fs::hard_link(&self.tmp, &target) {
                    Ok(()) => return Ok(target),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    // FAT, exFAT, some FUSE: no hard links; rename, never overwriting.
                    Err(_) => match rename_noreplace(&self.tmp, &target) {
                        Ok(()) => return Ok(target),
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                        // No RENAME_NOREPLACE either (old kernel, some FUSE): copy into
                        // a newly created file, still never overwriting.
                        Err(e)
                            if matches!(
                                e.raw_os_error(),
                                Some(libc::EINVAL | libc::ENOSYS | libc::EOPNOTSUPP)
                            ) =>
                        {
                            match copy_noreplace(&self.tmp, &target) {
                                Ok(()) => return Ok(target),
                                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                                Err(e) => return Err(Error::new("io", e.to_string())),
                            }
                        }
                        Err(e) => return Err(Error::new("io", e.to_string())),
                    },
                }
            }
            Err(Error::new("io", "no free name"))
        })();
        // Drop removes the temp name (the file lives on under its new one).
        r
    }

    /// Drop removes the temp file.
    pub fn discard(self) {}
}

fn copy_noreplace(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut src = File::open(from)?;
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(to)?;
    let r = std::io::copy(&mut src, &mut out).and_then(|_| out.sync_all());
    if r.is_err() {
        let _ = fs::remove_file(to);
    }
    r
}

fn rename_noreplace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let (a, b) = (
        CString::new(from.as_os_str().as_bytes())?,
        CString::new(to.as_os_str().as_bytes())?,
    );
    // SAFETY: valid C strings, AT_FDCWD. A raw syscall: musl has no
    // renameat2 wrapper.
    let r = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            a.as_ptr(),
            libc::AT_FDCWD,
            b.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if r == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// A file going to a phone: opened once, checked on the open descriptor,
/// read exactly `size` bytes.
#[derive(Debug)]
pub struct Source {
    pub file: File,
    pub size: u64,
    pub mtime: i64,
    pub sha256: String,
    pub name: String,
    pub sent: u64,
}

impl Source {
    pub fn open(path: &Path, max: u64) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
            .map_err(|e| Error::new("io", format!("{}: {e}", path.display())))?;
        let m = file
            .metadata()
            .map_err(|e| Error::new("io", e.to_string()))?;
        if !m.is_file() {
            return Err(Error::new("usage", "not a regular file"));
        }
        if m.len() == 0 || m.len() > max {
            return Err(Error::new(
                "usage",
                format!("size must be 1 byte..{}", human_size(max)),
            ));
        }
        let mut h = Sha256::new();
        let mut r = (&file).take(m.len());
        let mut buf = vec![0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            let n = r
                .read(&mut buf)
                .map_err(|e| Error::new("io", e.to_string()))?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            total += n as u64;
        }
        if total != m.len() {
            return Err(Error::new("io", "the file changed while it was read"));
        }
        use std::io::Seek;
        (&file)
            .seek(std::io::SeekFrom::Start(0))
            .map_err(|e| Error::new("io", e.to_string()))?;
        Ok(Self {
            size: m.len(),
            mtime: m.mtime(),
            sha256: hex(&h.finalize()),
            name: clean_name(
                &path
                    .file_name()
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_default(),
            ),
            sent: 0,
            file,
        })
    }

    /// The next at most `n` bytes.
    pub fn read(&mut self, n: usize) -> Result<Vec<u8>> {
        let want = n.min((self.size - self.sent) as usize);
        let mut buf = vec![0u8; want];
        self.file
            .read_exact(&mut buf)
            .map_err(|e| Error::new("io", format!("read: {e}")))?;
        self.sent += want as u64;
        Ok(buf)
    }

    /// Still the file we hashed (size and mtime).
    pub fn unchanged(&self) -> bool {
        self.file
            .metadata()
            .is_ok_and(|m| m.len() == self.size && m.mtime() == self.mtime)
    }
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn sha256_hex(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

// ---- tools -----------------------------------------------------------------

/// `STANCE_TEST_TOOLS` (a directory) replaces the tools in tests.
fn tool(name: &str) -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("STANCE_TEST_TOOLS") {
        let p = PathBuf::from(d).join(name);
        return p.is_file().then_some(p);
    }
    crate::tool::find(name)
}

/// Sets the PC clipboard to `text` (wl-copy; it forks and the parent exits
/// once it has the data).
pub fn apply_text(text: &[u8]) -> Result<()> {
    apply_clip(text, "text/plain;charset=utf-8")
}

/// Sets the PC clipboard to `data` of type `mime`.
pub fn apply_clip(data: &[u8], mime: &str) -> Result<()> {
    let text = data;
    if std::env::var_os("WAYLAND_DISPLAY").is_none()
        && std::env::var_os("STANCE_TEST_TOOLS").is_none()
    {
        return Err(Error::new("missing_tool", "no Wayland session for wl-copy"));
    }
    let wl = tool("wl-copy").ok_or_else(|| Error::new("missing_tool", "wl-copy not found"))?;
    let mut child = Command::new(wl)
        .args(["--type", mime])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::new("tool", e.to_string()))?;
    let mut stdin = child.stdin.take().expect("piped");
    let data = text.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let ok = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let _ = writer.join();
    if ok {
        Ok(())
    } else {
        Err(Error::new("tool", "wl-copy failed"))
    }
}

/// A plain notification, in the background.
pub fn notify(summary: &str, body: &str) {
    let Some(n) = tool("notify-send") else {
        return;
    };
    let (summary, body) = (plain(summary, 80), plain(body, 200));
    std::thread::spawn(move || {
        // `--`: text from a phone may start with a dash.
        if let Err(e) = crate::tool::run_tool(
            &n,
            &["-a", "stance", "--", &summary, &body],
            Duration::from_secs(3),
        ) {
            eprintln!("stance deck: notify-send: {}", e.message);
        }
    });
}

/// Asks with Accept / Decline buttons; `Some(true)` = accepted. Blocks up
/// to `CONSENT_WAIT` (run it on its own thread). No answer, no action
/// support or no tool = `None`.
pub fn ask(summary: &str, body: &str) -> Option<bool> {
    let n = tool("notify-send")?;
    let (summary, body) = (plain(summary, 80), plain(body, 200));
    let out = crate::tool::run_tool(
        &n,
        &[
            "-a",
            "stance",
            "--wait",
            "-t",
            "60000",
            "-A",
            "accept=Accept",
            "-A",
            "decline=Decline",
            "--",
            &summary,
            &body,
        ],
        super::stream::CONSENT_WAIT,
    )
    .ok()?;
    match out.trim() {
        "accept" => Some(true),
        "decline" => Some(false),
        _ => None,
    }
}

// ---- server-wide state -------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Queued,
    Waiting,
    Sending,
    Receiving,
    Done,
    Declined,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Transfer {
    pub id: u64,
    /// "to_phone" | "from_phone"
    pub direction: &'static str,
    pub device: String,
    pub name: String,
    pub size: u64,
    pub state: State,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub at: u64,
}

/// The one offer waiting for a person, server-wide.
#[derive(Debug, Clone)]
pub struct Consent {
    pub id: u64,
    pub device: String,
    pub name: String,
    pub size: u64,
    pub answer: Option<bool>,
}

/// A file the user asked to send (`stance deck send`), waiting for a session.
#[derive(Debug)]
pub struct Outgoing {
    pub id: u64,
    pub to: Option<String>,
    pub path: PathBuf,
}

pub struct Hub {
    settings: Mutex<(Option<SystemTime>, Settings)>,
    echo: Mutex<Vec<(String, String, Instant)>>,
    consent: Mutex<Option<Consent>>,
    transfers: Mutex<VecDeque<Transfer>>,
    outbox: Mutex<Vec<Outgoing>>,
    /// device, refusals, last refusal, cooling until.
    #[allow(clippy::type_complexity)]
    cooldown: Mutex<Vec<(String, u32, Instant, Option<Instant>)>>,
    pub incoming: AtomicU64,
    next_id: AtomicU64,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Self {
        Self {
            settings: Mutex::new((None, Settings::default())),
            echo: Mutex::new(Vec::new()),
            consent: Mutex::new(None),
            transfers: Mutex::new(VecDeque::new()),
            outbox: Mutex::new(Vec::new()),
            cooldown: Mutex::new(Vec::new()),
            incoming: AtomicU64::new(0),
            next_id: AtomicU64::new(1),
        }
    }

    /// The settings, re-read when transfer.toml changed on disk.
    pub fn settings(&self) -> Settings {
        let mtime = settings_path()
            .ok()
            .and_then(|p| fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());
        let mut s = self.settings.lock().unwrap_or_else(|e| e.into_inner());
        if s.0 != mtime || mtime.is_none() && s.1 != Settings::default() {
            let (new, warn) = load_settings();
            if let Some(w) = warn {
                eprintln!("stance deck: {w} (transfers off)");
            }
            *s = (mtime, new);
        }
        s.1.clone()
    }

    pub fn id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    // -- echo guard: text a phone sent is not sent back to it ---------------
    pub fn echo_add(&self, hash: &str, device: &str) {
        let mut e = self.echo.lock().unwrap_or_else(|e| e.into_inner());
        e.retain(|(_, _, t)| t.elapsed() < ECHO_TTL);
        e.push((hash.into(), device.into(), Instant::now()));
    }
    pub fn echo_from(&self, hash: &str, device: &str) -> bool {
        let e = self.echo.lock().unwrap_or_else(|e| e.into_inner());
        e.iter()
            .any(|(h, d, t)| h == hash && d == device && t.elapsed() < ECHO_TTL)
    }

    // -- consent -----------------------------------------------------------
    /// Takes the single consent slot; `None` when another offer waits.
    pub fn consent_open(&self, device: &str, name: &str, size: u64) -> Option<u64> {
        let mut c = self.consent.lock().unwrap_or_else(|e| e.into_inner());
        if c.is_some() {
            return None;
        }
        let id = self.id();
        *c = Some(Consent {
            id,
            device: device.into(),
            name: name.into(),
            size,
            answer: None,
        });
        Some(id)
    }
    /// Answers the consent `id` (a stale id does nothing). True if it matched.
    pub fn consent_answer(&self, id: u64, yes: bool) -> bool {
        let mut c = self.consent.lock().unwrap_or_else(|e| e.into_inner());
        match c.as_mut() {
            Some(x) if x.id == id && x.answer.is_none() => {
                x.answer = Some(yes);
                true
            }
            _ => false,
        }
    }
    pub fn consent_state(&self, id: u64) -> Option<Option<bool>> {
        let c = self.consent.lock().unwrap_or_else(|e| e.into_inner());
        c.as_ref().filter(|x| x.id == id).map(|x| x.answer)
    }
    pub fn consent_close(&self, id: u64) {
        let mut c = self.consent.lock().unwrap_or_else(|e| e.into_inner());
        if c.as_ref().is_some_and(|x| x.id == id) {
            *c = None;
        }
    }
    pub fn consent_pending(&self) -> Option<Consent> {
        self.consent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .filter(|c| c.answer.is_none())
    }

    // -- cooldown after declined / unanswered file offers ---------------------
    pub fn cooling(&self, device: &str) -> bool {
        let mut c = self.cooldown.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        // Refusals older than the cooldown no longer count.
        c.retain(|(_, _, last, until)| {
            until.map_or(now.duration_since(*last) < COOLDOWN, |u| u > now)
        });
        c.iter()
            .any(|(d, _, _, until)| d == device && until.is_some())
    }
    pub fn refused(&self, device: &str) {
        let mut c = self.cooldown.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        match c.iter_mut().find(|x| x.0 == device) {
            Some(x) => {
                x.1 += 1;
                x.2 = now;
                if x.1 >= COOLDOWN_AFTER {
                    x.3 = Some(now + COOLDOWN);
                }
            }
            None => c.push((device.into(), 1, now, None)),
        }
    }
    pub fn accepted(&self, device: &str) {
        let mut c = self.cooldown.lock().unwrap_or_else(|e| e.into_inner());
        c.retain(|x| x.0 != device);
    }

    // -- transfer list ---------------------------------------------------------
    pub fn record(&self, t: Transfer) {
        let mut l = self.transfers.lock().unwrap_or_else(|e| e.into_inner());
        l.retain(|x| x.id != t.id);
        l.push_back(t);
        while l.len() > HISTORY {
            l.pop_front();
        }
    }
    pub fn update(&self, id: u64, state: State, detail: Option<String>) {
        let mut l = self.transfers.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = l.iter_mut().find(|t| t.id == id) {
            t.state = state;
            t.detail = detail;
            t.at = unix_now();
        }
    }
    pub fn list(&self) -> Vec<Transfer> {
        self.transfers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect()
    }
    pub fn new_transfer(
        &self,
        direction: &'static str,
        device: &str,
        name: &str,
        size: u64,
        state: State,
    ) -> u64 {
        let id = self.id();
        self.record(Transfer {
            id,
            direction,
            device: device.into(),
            name: name.into(),
            size,
            state,
            detail: None,
            at: unix_now(),
        });
        id
    }

    // -- files queued for phones -----------------------------------------------
    pub fn queue(&self, o: Outgoing) {
        self.outbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(o);
    }
    /// The next queued file for `device` (or for any device).
    pub fn take_for(&self, device: &str) -> Option<Outgoing> {
        let mut q = self.outbox.lock().unwrap_or_else(|e| e.into_inner());
        let i = q
            .iter()
            .position(|o| o.to.as_deref().is_none_or(|t| t == device))?;
        Some(q.remove(i))
    }
    /// Queued files whose device is not connected: failed after a while.
    pub fn expire_queue(&self, connected: &[String], older_than: u64) {
        let mut q = self.outbox.lock().unwrap_or_else(|e| e.into_inner());
        let now = unix_now();
        let list = self
            .transfers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        q.retain(|o| {
            let fresh = list
                .iter()
                .any(|t| t.id == o.id && now.saturating_sub(t.at) < older_than);
            let reachable = match &o.to {
                Some(d) => connected.contains(d),
                None => !connected.is_empty(),
            };
            // Connected but never taken (an app without file support): give up too.
            let stuck = list
                .iter()
                .any(|t| t.id == o.id && now.saturating_sub(t.at) >= older_than * 5);
            (fresh || reachable) && !stuck
        });
        for t in list {
            if t.state == State::Queued && !q.iter().any(|o| o.id == t.id) {
                self.update(t.id, State::Failed, Some("no phone connected".into()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name("C:\\x\\a.txt"), "a.txt");
        assert_eq!(clean_name(".bashrc"), "bashrc");
        assert_eq!(clean_name("-rf .txt"), "rf .txt");
        assert_eq!(clean_name("..."), "file");
        assert_eq!(clean_name(""), "file");
        assert_eq!(clean_name("a\0b\nc.pdf"), "abc.pdf");
        assert_eq!(clean_name("invoice\u{202E}fdp.exe"), "invoicefdp.exe");
        assert_eq!(clean_name("name. . "), "name");
        let long = format!("{}.tar.gz", "ş".repeat(100));
        let c = clean_name(&long);
        assert!(c.len() <= NAME_BYTES && c.ends_with(".gz"), "{c}");
        let weird = format!("a.{}", "x".repeat(200));
        assert!(clean_name(&weird).len() <= NAME_BYTES);
    }

    #[test]
    fn notification_text_has_no_markup() {
        assert_eq!(plain("<b>hi</b> & \n bye", 50), "bhi/b bye");
        assert_eq!(plain("abcdef", 3), "abc…");
    }

    #[test]
    fn settings_rules() {
        let s = parse_settings("").unwrap();
        assert_eq!(s, Settings::default());
        assert_eq!(s.offered_caps(), vec!["file.pull"]);
        let s = parse_settings("clip_to_phone = true\nfiles_from_phone = \"ask\"\n").unwrap();
        assert_eq!(
            s.offered_caps(),
            vec!["clip.text", "file.push", "file.pull"]
        );
        assert!(parse_settings("max_file_mb = 0").is_err());
        assert!(parse_settings("max_file_mb = 513").is_err());
        assert!(parse_settings("download_dir = \"rel/dir\"").is_err());
        assert!(parse_settings("download_dir = \"/a/../b\"").is_err());
        assert!(parse_settings("files_from_phone = \"always\"").is_err());
        assert!(parse_settings("surprise = 1").is_err());
        assert!(parse_settings("schema = 9").is_err());
        let s = parse_settings(
            "clip_to_phone = true\nclip_images = true\ntrusted_devices = [\"Pixel 7\"]",
        )
        .unwrap();
        assert_eq!(
            s.offered_caps(),
            vec!["clip.text", "clip.image", "file.pull"]
        );
        assert!(parse_settings("trusted_devices = [\"bad\\nname\"]").is_err());
        assert!(parse_settings("clip_images = true").unwrap().offered_caps() == vec!["file.pull"]);
    }

    #[test]
    fn parts_never_overwrite_and_clean_up() {
        let dir = std::env::temp_dir().join(format!("stance-part-test-{}", random_hex()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.txt"), b"old").unwrap();
        let mut p = Part::create(&dir, "../a.txt").unwrap();
        p.write(b"new").unwrap();
        let got = p.finish().unwrap();
        assert_eq!(got, dir.join("a (2).txt"));
        assert_eq!(fs::read(dir.join("a.txt")).unwrap(), b"old");
        assert_eq!(fs::read(&got).unwrap(), b"new");
        let p = Part::create(&dir, "b").unwrap();
        p.discard();
        let left: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left.len(), 2, "{left:?}");
        fs::write(dir.join(".stance-part-dead"), b"x").unwrap();
        sweep(&dir);
        assert!(!dir.join(".stance-part-dead").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rename_fallback_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("stance-rn-test-{}", random_hex()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a"), b"a").unwrap();
        fs::write(dir.join("b"), b"b").unwrap();
        let e = rename_noreplace(&dir.join("a"), &dir.join("b")).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(dir.join("b")).unwrap(), b"b");
        rename_noreplace(&dir.join("a"), &dir.join("c")).unwrap();
        assert_eq!(fs::read(dir.join("c")).unwrap(), b"a");
        // Last resort (no RENAME_NOREPLACE): copy, also never overwriting.
        let e = copy_noreplace(&dir.join("c"), &dir.join("b")).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(dir.join("b")).unwrap(), b"b");
        copy_noreplace(&dir.join("c"), &dir.join("d")).unwrap();
        assert_eq!(fs::read(dir.join("d")).unwrap(), b"a");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sources_check_the_open_file() {
        let dir = std::env::temp_dir().join(format!("stance-src-test-{}", random_hex()));
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x.bin");
        fs::write(&f, b"hello").unwrap();
        let mut s = Source::open(&f, 1 << 20).unwrap();
        assert_eq!(s.sha256, sha256_hex(b"hello"));
        assert_eq!(s.read(3).unwrap(), b"hel");
        assert_eq!(s.read(100).unwrap(), b"lo");
        assert!(s.unchanged());
        assert!(Source::open(&dir, 1 << 20).is_err());
        assert!(Source::open(&f, 4).is_err());
        fs::write(dir.join("empty"), b"").unwrap();
        assert!(Source::open(&dir.join("empty"), 10).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn consent_is_single_and_ids_are_checked() {
        let h = Hub::new();
        let id = h.consent_open("p", "a", 1).unwrap();
        assert!(h.consent_open("q", "b", 1).is_none());
        assert!(!h.consent_answer(id + 100, true));
        assert!(h.consent_answer(id, true));
        assert!(!h.consent_answer(id, false));
        assert_eq!(h.consent_state(id), Some(Some(true)));
        h.consent_close(id);
        assert!(h.consent_open("q", "b", 1).is_some());
    }

    #[test]
    fn cooldown_and_echo() {
        let h = Hub::new();
        for _ in 0..COOLDOWN_AFTER {
            assert!(!h.cooling("p"));
            h.refused("p");
        }
        assert!(h.cooling("p"));
        assert!(!h.cooling("q"));
        h.echo_add("abc", "p");
        assert!(h.echo_from("abc", "p"));
        assert!(!h.echo_from("abc", "q"));
    }
}
