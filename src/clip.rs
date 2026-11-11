//! Clipboard shelf: images, texts and links copied on Wayland, kept in a
//! private store (see docs/clip.md). `wl-paste --watch stance clip ingest`
//! feeds it; the panel reads `stance clip list --json`.
//!
//! Layout of the clip dir (0700): `index.json` (metadata + 200-char
//! previews), `blobs/<sha256>.<ext>` (0600, full content), `lock`.
//! Every writer holds the lock for load → mutate → atomic write; readers
//! rely on the atomic rename. `<state dir>/clip.rev` holds the index
//! revision so the panel can watch one tiny file.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::store;

pub const MAX_TEXT: u64 = 256 * 1024;
pub const MAX_IMAGE: u64 = 32 * 1024 * 1024;
const PREVIEW_CHARS: usize = 200;
/// Search looks at this much of each text blob beyond the preview.
const SEARCH_BYTES: u64 = 4096;
const TRASH_SECS: u64 = 24 * 3600;
pub const RETENTION_DAYS: &[u32] = &[1, 3, 7, 15, 30];
pub const MAX_PAUSE: u64 = 24 * 3600;
const MAX_SIDE: u32 = 65_535;
/// Live, unpinned items beyond these limits are dropped oldest first.
pub const MAX_ITEMS: usize = 5000;
pub const MAX_BYTES: u64 = 1 << 30;
const MAX_INDEX: u64 = 64 * 1024 * 1024;
/// Prefix of what a damaged index is moved aside to (with its blobs).
pub const CORRUPT: &str = "corrupt-";

/// `30s`, `15m`, `1h` (or plain seconds) → seconds.
pub fn parse_duration(s: &str) -> Result<u64> {
    let bad = || Error::new("usage", format!("`{s}` is not a duration (30s, 15m, 1h)"));
    let (num, mul) = match s.as_bytes().last() {
        Some(b's') => (&s[..s.len() - 1], 1),
        Some(b'm') => (&s[..s.len() - 1], 60),
        Some(b'h') => (&s[..s.len() - 1], 3600),
        _ => (s, 1),
    };
    if num.is_empty() || num.len() > 6 || !num.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let n: u64 = num.parse().map_err(|_| bad())?;
    Ok(n * mul)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    Link,
    Image,
}

impl Kind {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "text" => Ok(Self::Text),
            "link" => Ok(Self::Link),
            "image" => Ok(Self::Image),
            _ => Err(Error::new(
                "usage",
                format!("unknown kind `{s}` (text, link, image)"),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// File format version (src/migrate.rs); missing = 1.
    pub schema: u32,
    /// Nothing is stored unless this is on.
    pub enabled: bool,
    pub text: bool,
    pub image: bool,
    pub retention_days: u32,
    /// Capture is paused until this unix time (set by `clip pause`, not by
    /// `config put`). Kept on disk so a restart does not resume early.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused_until: Option<u64>,
}

impl Config {
    /// The pause deadline if it is still in the future.
    pub fn paused(&self, at: u64) -> Option<u64> {
        self.paused_until.filter(|t| *t > at)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema: crate::migrate::CURRENT,
            enabled: false,
            text: true,
            image: true,
            retention_days: 30,
            paused_until: None,
        }
    }
}

impl Config {
    fn validate(&self) -> Result<()> {
        if !RETENTION_DAYS.contains(&self.retention_days) {
            return Err(Error::new(
                "invalid",
                "retention_days must be 1, 3, 7, 15 or 30",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub kind: Kind,
    pub created: u64,
    pub updated: u64,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub preview: String,
    #[serde(default)]
    pub host: Option<String>,
    pub size: u64,
    pub mime: String,
    pub sha256: String,
    pub ext: String,
    #[serde(default)]
    pub w: Option<u32>,
    #[serde(default)]
    pub h: Option<u32>,
    #[serde(default)]
    pub trashed: Option<u64>,
}

impl Entry {
    /// sha256 is 64 hex characters and ext one we write ourselves.
    pub fn sane(&self) -> bool {
        self.sha256.len() == 64
            && self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            && matches!(
                self.ext.as_str(),
                "txt" | "png" | "jpg" | "webp" | "gif" | "bmp"
            )
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Index {
    pub schema: u32,
    pub rev: u64,
    pub next_id: u64,
    pub items: Vec<Entry>,
    /// Ids removed by the last `rm`/`clear` (for `undo`).
    #[serde(default)]
    pub last_removed: Vec<u64>,
}

/// One item as the panel sees it (docs/clip.md, tests/fixtures/clip-list.json).
#[derive(Debug, Serialize)]
pub struct Item {
    pub id: String,
    pub kind: Kind,
    pub created: u64,
    pub updated: u64,
    pub pinned: bool,
    pub preview: String,
    pub host: Option<String>,
    pub size: u64,
    pub mime: String,
    pub path: Option<String>,
    pub w: Option<u32>,
    pub h: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct Listing {
    pub rev: u64,
    pub enabled: bool,
    pub paused_until: Option<u64>,
    pub items: Vec<Item>,
}

pub struct Clip {
    pub dir: PathBuf,
    pub config_path: PathBuf,
    pub rev_path: PathBuf,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn io_err(what: &str) -> impl Fn(std::io::Error) -> Error + '_ {
    move |e| Error::new("io", format!("{what}: {e}"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// Ids are decimal numbers; anything else is refused before use.
pub fn parse_id(s: &str) -> Result<u64> {
    if s.is_empty() || s.len() > 19 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::new("usage", format!("`{s}` is not a clip id")));
    }
    s.parse()
        .map_err(|_| Error::new("usage", format!("`{s}` is not a clip id")))
}

/// Sniffs an image type from magic bytes: (mime, extension).
pub fn sniff_image(data: &[u8]) -> Option<(&'static str, &'static str)> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "png"))
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(("image/jpeg", "jpg"))
    } else if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some(("image/gif", "gif"))
    } else if data.starts_with(b"BM") && data.len() > 26 {
        Some(("image/bmp", "bmp"))
    } else {
        None
    }
}

/// Width and height from a PNG IHDR chunk.
pub fn png_size(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 24 || &data[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(data[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(data[20..24].try_into().ok()?);
    // Header values are untrusted; only plausible sizes are reported.
    ((1..=MAX_SIDE).contains(&w) && (1..=MAX_SIDE).contains(&h)).then_some((w, h))
}

/// The host of a standalone http(s) URL, or None when `text` is not exactly
/// one such URL. Non-ASCII hosts (homograph risk) stay plain text.
pub fn link_host(text: &str) -> Option<String> {
    let t = text.trim();
    // A backslash makes browsers and URL parsers disagree on the host
    // (`https://evil.com\@trusted.com`), so such text stays plain text.
    if t.is_empty() || t.len() > 4096 || t.chars().any(|c| c.is_whitespace() || c == '\\') {
        return None;
    }
    let rest = t
        .strip_prefix("https://")
        .or_else(|| t.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let hostport = authority.rsplit('@').next()?;
    let host = if let Some(v6) = hostport.strip_prefix('[') {
        v6.split(']').next()?
    } else {
        hostport.split(':').next()?
    };
    if host.is_empty()
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
    {
        return None;
    }
    Some(host.to_lowercase())
}

/// True when every non-empty line is a `file://` URI (a file-manager copy).
fn is_file_copy(text: &str) -> bool {
    let mut any = false;
    for l in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !l.starts_with("file://") {
            return false;
        }
        any = true;
    }
    any
}

/// First PREVIEW_CHARS characters with control characters as spaces.
pub fn preview(text: &str) -> String {
    text.trim()
        .chars()
        .filter(|c| !is_invisible(*c))
        .take(PREVIEW_CHARS)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Bidi overrides/isolates, zero-width and other invisible format
/// characters that could make a preview lie about its content.
pub fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{206F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

/// Reads at most `max` bytes; None if the input is larger (the rest is
/// drained so the writer does not get EPIPE).
fn read_capped(input: &mut impl Read, max: u64) -> std::io::Result<Option<Vec<u8>>> {
    let mut buf = Vec::new();
    input.take(max + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > max {
        std::io::copy(input, &mut std::io::sink())?;
        return Ok(None);
    }
    Ok(Some(buf))
}

/// What one copy becomes before it is stored.
#[derive(Debug, PartialEq)]
pub struct Parsed {
    pub kind: Kind,
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub ext: &'static str,
    pub preview: String,
    pub host: Option<String>,
    pub w: Option<u32>,
    pub h: Option<u32>,
}

/// Pure classification of one clipboard payload. None = do not store.
pub fn classify(data: Vec<u8>, image: bool) -> Option<Parsed> {
    if image {
        let (mime, ext) = sniff_image(&data)?;
        let (w, h) = match png_size(&data) {
            Some((w, h)) => (Some(w), Some(h)),
            None => (None, None),
        };
        return Some(Parsed {
            kind: Kind::Image,
            bytes: data,
            mime,
            ext,
            preview: String::new(),
            host: None,
            w,
            h,
        });
    }
    let text = String::from_utf8(data).ok()?;
    if text.trim().is_empty() || text.contains('\0') || is_file_copy(&text) {
        return None;
    }
    let host = link_host(&text);
    let kind = if host.is_some() {
        Kind::Link
    } else {
        Kind::Text
    };
    Some(Parsed {
        kind,
        preview: preview(&text),
        bytes: text.into_bytes(),
        mime: "text/plain",
        ext: "txt",
        host,
        w: None,
        h: None,
    })
}

impl Clip {
    pub fn open(state_dir: &Path) -> Result<Self> {
        let dir = match std::env::var_os("STANCE_CLIP_DIR").map(PathBuf::from) {
            Some(p) if p.is_absolute() => p,
            _ => data_dir()?,
        };
        let config_path = match std::env::var_os("STANCE_CLIP_CONFIG").map(PathBuf::from) {
            Some(p) if p.is_absolute() => p,
            _ => store::config_dir()?.join("clip.json"),
        };
        Ok(Self {
            dir,
            config_path,
            rev_path: state_dir.join("clip.rev"),
        })
    }

    fn blobs(&self) -> PathBuf {
        self.dir.join("blobs")
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("index.json")
    }

    pub fn blob_path(&self, e: &Entry) -> PathBuf {
        // Entries are validated on load (`Entry::sane`), so this cannot leave
        // blobs/; the fallback name just never exists.
        if !e.sane() {
            return self.blobs().join("invalid");
        }
        self.blobs().join(format!("{}.{}", e.sha256, e.ext))
    }

    pub fn config(&self) -> Result<Config> {
        if !self.config_path.exists() {
            return Ok(Config::default());
        }
        let s = store::read_checked(&self.config_path)?;
        let bad = |e: serde_json::Error| Error::new("invalid", format!("clip.json: {e}"));
        let mut v: serde_json::Value = serde_json::from_str(&s).map_err(bad)?;
        let found = crate::migrate::upgrade_json("clip.json", &mut v)?;
        if found > crate::migrate::CURRENT
            && let Some(o) = v.as_object_mut()
        {
            // A newer file: read the settings we know, ignore the rest.
            // Writing is refused by `save_config`.
            o.retain(|k, _| {
                matches!(
                    k.as_str(),
                    "schema" | "enabled" | "text" | "image" | "retention_days" | "paused_until"
                )
            });
        }
        let c: Config = serde_json::from_value(v).map_err(bad)?;
        c.validate()?;
        Ok(c)
    }

    /// Merges a JSON object into the current config and saves it.
    pub fn put_config(&self, json: &str) -> Result<Config> {
        let patch: serde_json::Value = serde_json::from_str(json)
            .map_err(|e| Error::new("invalid", format!("--data: {e}")))?;
        let serde_json::Value::Object(patch) = patch else {
            return Err(Error::new("invalid", "--data must be a JSON object"));
        };
        let c = {
            let _l = store::lock_named(&self.dir, "config.lock")?;
            let mut cur = serde_json::to_value(self.config()?).unwrap_or_default();
            for (k, v) in patch {
                if !matches!(k.as_str(), "enabled" | "text" | "image" | "retention_days") {
                    return Err(Error::new("invalid", format!("unknown clip setting `{k}`")));
                }
                cur[k] = v;
            }
            let c: Config = serde_json::from_value(cur)
                .map_err(|e| Error::new("invalid", format!("--data: {e}")))?;
            c.validate()?;
            self.save_config(&c)?;
            c
        };
        // A shorter retention takes effect now, not at the next copy.
        if self.index_path().exists() {
            self.prune()?;
        }
        Ok(c)
    }

    /// Pauses capture for `secs` (at most a day) or, with 0, resumes it.
    /// Returns the deadline.
    pub fn pause(&self, secs: u64) -> Result<Option<u64>> {
        if secs > MAX_PAUSE {
            return Err(Error::new("usage", "a pause is at most 24h"));
        }
        let c = {
            let _l = store::lock_named(&self.dir, "config.lock")?;
            let mut c = self.config()?;
            c.paused_until = (secs > 0).then(|| now() + secs);
            self.save_config(&c)?;
            c
        };
        if let Ok(idx) = self.load() {
            // Lets the panel notice the change through clip.rev.
            self.write_rev(idx.rev);
        }
        Ok(c.paused_until)
    }

    fn save_config(&self, c: &Config) -> Result<()> {
        let parent = self
            .config_path
            .parent()
            .ok_or_else(|| Error::new("io", "config path has no parent"))?;
        crate::migrate::guard_json(&self.config_path, "clip.json")?;
        store::ensure_private_dir(parent)?;
        let mut c = c.clone();
        c.schema = crate::migrate::CURRENT;
        let body = serde_json::to_string_pretty(&c).unwrap_or_default();
        store::atomic_write(&self.config_path, body.as_bytes(), false)
    }

    /// Index as last written (no lock; rename is atomic). Missing = empty.
    pub fn load(&self) -> Result<Index> {
        let p = self.index_path();
        let mut f = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&p)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Index {
                    schema: 1,
                    next_id: 1,
                    ..Default::default()
                });
            }
            Err(e) => return Err(Error::new("io", format!("{}: {e}", p.display()))),
        };
        let mut s = String::new();
        (&mut f)
            .take(MAX_INDEX + 1)
            .read_to_string(&mut s)
            .map_err(io_err("clip index"))?;
        if s.len() as u64 > MAX_INDEX {
            return Err(Error::new("invalid", "clip index: larger than 64 MiB"));
        }
        let mut idx: Index = serde_json::from_str(&s)
            .map_err(|e| Error::new("invalid", format!("clip index: {e}")))?;
        if idx.schema > 1 {
            return Err(Error::new(
                "newer_schema",
                "clip index was written by a newer stance; update stance (nothing was changed)",
            ));
        }
        // Tampered entries must never become paths.
        idx.items.retain(Entry::sane);
        Ok(idx)
    }

    /// Runs `f` on the index under the lock and saves it with a new rev.
    pub fn mutate<T>(&self, f: impl FnOnce(&mut Index, &Self) -> Result<T>) -> Result<(T, u64)> {
        let _lock = store::lock_state_in(&self.dir)?;
        let mut idx = match self.load() {
            Err(e) if e.code == "invalid" => {
                // A damaged index must not wedge the shelf, nor cost the
                // content: index and blobs move aside together (doctor
                // reports it; delete the folder once checked) and the shelf
                // starts empty.
                let aside = self.dir.join(format!("{CORRUPT}{}", now()));
                let _ = store::ensure_private_dir(&aside);
                let _ = fs::rename(self.index_path(), aside.join("index.json"));
                let _ = fs::rename(self.blobs(), aside.join("blobs"));
                eprintln!(
                    "stance clip: {e}; moved index and content to {}",
                    aside.display()
                );
                Index {
                    schema: 1,
                    next_id: 1,
                    ..Default::default()
                }
            }
            r => r?,
        };
        let out = f(&mut idx, self)?;
        idx.schema = 1;
        idx.rev += 1;
        idx.next_id = idx.next_id.max(1);
        let body = serde_json::to_vec(&idx).unwrap_or_default();
        store::atomic_write(&self.index_path(), &body, false)?;
        self.write_rev(idx.rev);
        Ok((out, idx.rev))
    }

    fn write_rev(&self, rev: u64) {
        if let Some(parent) = self.rev_path.parent()
            && store::ensure_private_dir(parent).is_ok()
        {
            let _ = store::atomic_write(&self.rev_path, rev.to_string().as_bytes(), false);
        }
    }

    /// Writes the blob unless an intact one is there (content-addressed;
    /// temp file + rename so a crash never leaves a truncated blob; 0600, no
    /// symlink follow). Called under the lock, so `sweep` cannot race it.
    fn write_blob(&self, sha: &str, ext: &str, data: &[u8]) -> Result<()> {
        store::ensure_private_dir(&self.blobs())?;
        let p = self.blobs().join(format!("{sha}.{ext}"));
        if let Ok(m) = fs::symlink_metadata(&p)
            && m.is_file()
            && m.len() == data.len() as u64
        {
            return Ok(());
        }
        let tmp = self
            .blobs()
            .join(format!(".{sha}.{}.tmp", std::process::id()));
        let _ = fs::remove_file(&tmp);
        let r = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)
            .and_then(|mut f| f.write_all(data).and_then(|_| f.sync_all()))
            .and_then(|_| fs::rename(&tmp, &p));
        if let Err(e) = r {
            let _ = fs::remove_file(&tmp);
            return Err(Error::new("io", format!("clip blob: {e}")));
        }
        Ok(())
    }

    /// Stores one payload. Returns the entry id and whether it was new.
    pub fn add(&self, p: Parsed, at: u64) -> Result<(u64, bool)> {
        let sha = sha256_hex(&p.bytes);
        let retention = self.config()?.retention_days;
        let (r, _) = self.mutate(|idx, c| {
            // Blob before index (both under the lock): a crash leaves an
            // orphan blob, never a dangling reference.
            c.write_blob(&sha, p.ext, &p.bytes)?;
            let r = if let Some(e) = idx.items.iter_mut().find(|e| e.sha256 == sha) {
                e.updated = at;
                e.trashed = None;
                (e.id, false)
            } else {
                let id = idx.next_id.max(1);
                idx.next_id = id + 1;
                idx.items.push(Entry {
                    id,
                    kind: p.kind,
                    created: at,
                    updated: at,
                    pinned: false,
                    preview: p.preview,
                    host: p.host,
                    size: p.bytes.len() as u64,
                    mime: p.mime.into(),
                    sha256: sha.clone(),
                    ext: p.ext.into(),
                    w: p.w,
                    h: p.h,
                    trashed: None,
                });
                (id, true)
            };
            prune_index(idx, retention, at);
            c.sweep(idx);
            Ok(r)
        })?;
        Ok(r)
    }

    /// Deletes blobs that no entry (trashed ones included) refers to.
    /// Does nothing while a damaged index is kept aside.
    fn sweep(&self, idx: &Index) {
        let keep: HashSet<String> = idx
            .items
            .iter()
            .map(|e| format!("{}.{}", e.sha256, e.ext))
            .collect();
        let Ok(rd) = fs::read_dir(self.blobs()) else {
            return;
        };
        for d in rd.flatten() {
            let name = d.file_name().to_string_lossy().into_owned();
            let stale_tmp = name.starts_with('.')
                && d.metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age.as_secs() > 600);
            if (!name.starts_with('.') && !keep.contains(&name)) || stale_tmp {
                let _ = fs::remove_file(d.path());
            }
        }
    }

    pub fn prune(&self) -> Result<u64> {
        let retention = self.config()?.retention_days;
        let at = now();
        Ok(self
            .mutate(|idx, c| {
                prune_index(idx, retention, at);
                c.sweep(idx);
                Ok(())
            })?
            .1)
    }

    /// Live items, pinned first then newest; at most `limit` of each kind.
    pub fn list(&self, kind: Option<Kind>, query: Option<&str>, limit: usize) -> Result<Listing> {
        let idx = self.load()?;
        let cfg = self.config().unwrap_or_default();
        let enabled = cfg.enabled;
        let needle = query
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(str::to_lowercase);
        let mut v: Vec<&Entry> = idx
            .items
            .iter()
            .filter(|e| e.trashed.is_none())
            .filter(|e| kind.is_none_or(|k| e.kind == k))
            .filter(|e| needle.as_deref().is_none_or(|n| self.matches(e, n)))
            .collect();
        v.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated.cmp(&a.updated)));
        // The limit is per kind, so many texts never hide older images.
        let mut seen = [0usize; 3];
        let items = v
            .into_iter()
            .filter(|e| {
                let n = &mut seen[e.kind as usize];
                *n += 1;
                *n <= limit
            })
            .map(|e| Item {
                id: e.id.to_string(),
                kind: e.kind,
                created: e.created,
                updated: e.updated,
                pinned: e.pinned,
                preview: e.preview.clone(),
                host: e.host.clone(),
                size: e.size,
                mime: e.mime.clone(),
                path: (e.kind == Kind::Image)
                    .then(|| self.blob_path(e).to_string_lossy().into_owned()),
                w: e.w,
                h: e.h,
            })
            .collect();
        Ok(Listing {
            rev: idx.rev,
            enabled,
            paused_until: cfg.paused(now()),
            items,
        })
    }

    fn matches(&self, e: &Entry, needle: &str) -> bool {
        if e.preview.to_lowercase().contains(needle)
            || e.host.as_deref().is_some_and(|h| h.contains(needle))
        {
            return true;
        }
        if e.kind == Kind::Image {
            return false;
        }
        let Ok(f) = self.open_blob(e) else {
            return false;
        };
        let mut buf = Vec::new();
        if f.take(SEARCH_BYTES).read_to_end(&mut buf).is_err() {
            return false;
        }
        String::from_utf8_lossy(&buf)
            .to_lowercase()
            .contains(needle)
    }

    pub fn open_blob(&self, e: &Entry) -> Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.blob_path(e))
            .map_err(io_err("clip blob"))
    }

    pub fn entry(&self, id: u64) -> Result<Entry> {
        self.load()?
            .items
            .into_iter()
            .find(|e| e.id == id && e.trashed.is_none())
            .ok_or_else(|| Error::new("not_found", format!("no clip item {id}")))
    }

    /// Puts the item back on the clipboard and moves it to the top.
    pub fn copy(&self, id: u64) -> Result<u64> {
        let e = self.entry(id)?;
        let wl_copy = crate::tool::find("wl-copy")
            .ok_or_else(|| Error::new("missing_tool", "wl-copy not found (wl-clipboard)"))?;
        let blob = self.open_blob(&e)?;
        // wl-copy forks a server and the parent exits once it has the data.
        let mut cmd = Command::new(wl_copy);
        cmd.args(["--type", &e.mime])
            .stdin(Stdio::from(blob))
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if !crate::tool::status_timeout(cmd, std::time::Duration::from_secs(10)) {
            return Err(Error::new("tool", "wl-copy failed"));
        }
        let at = now();
        Ok(self
            .mutate(|idx, _| {
                if let Some(x) = idx.items.iter_mut().find(|x| x.id == id) {
                    x.updated = at;
                }
                Ok(())
            })?
            .1)
    }

    pub fn set_pinned(&self, ids: &[u64], on: bool) -> Result<u64> {
        Ok(self
            .mutate(|idx, _| {
                for id in ids {
                    live_mut(idx, *id)?.pinned = on;
                }
                Ok(())
            })?
            .1)
    }

    pub fn remove(&self, ids: &[u64]) -> Result<u64> {
        let at = now();
        Ok(self
            .mutate(|idx, _| {
                for id in ids {
                    live_mut(idx, *id)?.trashed = Some(at);
                }
                idx.last_removed = ids.to_vec();
                Ok(())
            })?
            .1)
    }

    /// Restores the last removal. Returns (restored count, rev).
    pub fn undo(&self) -> Result<(usize, u64)> {
        self.mutate(|idx, _| {
            let ids = std::mem::take(&mut idx.last_removed);
            let mut n = 0;
            for e in idx.items.iter_mut().filter(|e| ids.contains(&e.id)) {
                if e.trashed.take().is_some() {
                    n += 1;
                }
            }
            Ok(n)
        })
    }

    /// Deletes removed (trashed) items and their content now instead of
    /// after a day. Returns how many went.
    pub fn purge(&self) -> Result<(usize, u64)> {
        self.mutate(|idx, c| {
            let before = idx.items.len();
            idx.items.retain(|e| e.trashed.is_none());
            idx.last_removed.clear();
            c.sweep(idx);
            Ok(before - idx.items.len())
        })
    }

    /// Removes unpinned items (recoverable with undo), or with `all` wipes
    /// the whole store at once.
    pub fn clear(&self, all: bool) -> Result<u64> {
        let at = now();
        Ok(self
            .mutate(|idx, c| {
                if all {
                    idx.items.clear();
                    idx.last_removed.clear();
                    c.sweep(idx);
                } else {
                    let mut ids = Vec::new();
                    for e in idx.items.iter_mut() {
                        if !e.pinned && e.trashed.is_none() {
                            e.trashed = Some(at);
                            ids.push(e.id);
                        }
                    }
                    idx.last_removed = ids;
                }
                Ok(())
            })?
            .1)
    }
}

fn live_mut(idx: &mut Index, id: u64) -> Result<&mut Entry> {
    idx.items
        .iter_mut()
        .find(|e| e.id == id && e.trashed.is_none())
        .ok_or_else(|| Error::new("not_found", format!("no clip item {id}")))
}

/// Drops expired unpinned items and trash older than a day.
pub fn prune_index(idx: &mut Index, retention_days: u32, at: u64) {
    let keep_secs = u64::from(retention_days) * 86400;
    idx.items.retain(|e| match e.trashed {
        Some(t) => at.saturating_sub(t) < TRASH_SECS,
        None => e.pinned || at.saturating_sub(e.updated) < keep_secs,
    });
    // Size and count caps: oldest unpinned live items go first.
    let mut unpinned: Vec<(u64, u64, u64)> = idx
        .items
        .iter()
        .filter(|e| !e.pinned && e.trashed.is_none())
        .map(|e| (e.updated, e.id, e.size))
        .collect();
    unpinned.sort();
    let mut count = unpinned.len();
    let mut bytes: u64 = idx.items.iter().map(|e| e.size).sum();
    let mut drop = HashSet::new();
    for (_, id, size) in unpinned {
        if count <= MAX_ITEMS && bytes <= MAX_BYTES {
            break;
        }
        drop.insert(id);
        count -= 1;
        bytes = bytes.saturating_sub(size);
    }
    idx.items.retain(|e| !drop.contains(&e.id));
    let live: HashSet<u64> = idx.items.iter().map(|e| e.id).collect();
    idx.last_removed.retain(|id| live.contains(id));
}

fn data_dir() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        Some(p) if p.is_absolute() => p,
        _ => std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or_else(|| Error::new("env", "HOME is not set"))?
            .join(".local/share"),
    };
    Ok(base.join("stance").join("clip"))
}

/// `stance clip ingest`: called by `wl-paste --watch` once per copy with the
/// data on stdin and CLIPBOARD_STATE in the environment. Never fails loudly:
/// a broken store must not break the watcher loop, so errors go to stderr.
pub fn ingest(clip: &Clip, image: bool, input: &mut impl Read) -> Result<Option<u64>> {
    let state = std::env::var("CLIPBOARD_STATE").unwrap_or_default();
    let cfg = clip.config().unwrap_or_default();
    let wanted = state == "data"
        && cfg.enabled
        && cfg.paused(now()).is_none()
        && if image { cfg.image } else { cfg.text };
    if !wanted {
        // Sensitive, empty, disabled or unknown: read nothing into memory.
        let _ = std::io::copy(input, &mut std::io::sink());
        return Ok(None);
    }
    let max = if image { MAX_IMAGE } else { MAX_TEXT };
    let Some(data) = read_capped(input, max).map_err(io_err("stdin"))? else {
        return Ok(None);
    };
    let Some(p) = classify(data, image) else {
        return Ok(None);
    };
    Ok(Some(clip.add(p, now())?.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> Clip {
        let d = std::env::temp_dir().join(format!("stance-clip-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        store::ensure_private_dir(&d).unwrap();
        Clip {
            dir: d.join("clip"),
            config_path: d.join("cfg/clip.json"),
            rev_path: d.join("state/clip.rev"),
        }
    }

    #[test]
    fn newer_config_reads_but_is_never_written() {
        let c = tmp("newer-cfg");
        let p = c.config_path.clone();
        store::ensure_private_dir(p.parent().unwrap()).unwrap();
        fs::write(
            &p,
            r#"{"schema":2,"enabled":true,"retention_days":7,"future_knob":1}"#,
        )
        .unwrap();
        let cfg = c.config().unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.retention_days, 7);
        let before = fs::read(&p).unwrap();
        assert_eq!(
            c.put_config(r#"{"enabled":false}"#).unwrap_err().code,
            "newer_schema"
        );
        assert_eq!(c.pause(60).unwrap_err().code, "newer_schema");
        assert_eq!(fs::read(&p).unwrap(), before);
    }

    #[test]
    fn written_config_carries_schema() {
        let c = tmp("schema-cfg");
        c.put_config(r#"{"enabled":true}"#).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&c.config_path).unwrap()).unwrap();
        assert_eq!(v["schema"], 1);
    }

    #[test]
    fn fixture_configs_load() {
        let c = tmp("fx-0.3.0");
        let src =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/config-0.3.0/clip.json");
        store::ensure_private_dir(c.config_path.parent().unwrap()).unwrap();
        fs::copy(src, &c.config_path).unwrap();
        assert_eq!(c.config().unwrap().retention_days, 15);
    }

    fn text(s: &str) -> Parsed {
        classify(s.as_bytes().to_vec(), false).unwrap()
    }

    #[test]
    fn links() {
        assert_eq!(
            link_host("https://GitHub.com/a/b?x#y").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            link_host(" http://u:p@h.io:8080/ \n").as_deref(),
            Some("h.io")
        );
        assert_eq!(link_host("http://[::1]:80/").as_deref(), Some("::1"));
        assert_eq!(link_host("see https://a.b"), None);
        assert_eq!(link_host("https://a.b\nhttps://c.d"), None);
        assert_eq!(link_host("ftp://a.b"), None);
        assert_eq!(link_host("https://"), None);
        assert_eq!(link_host("javascript:alert(1)"), None);
        assert_eq!(text("https://x.org").kind, Kind::Link);
        assert_eq!(text("a https://x.org b").kind, Kind::Text);
    }

    #[test]
    fn classify_rules() {
        assert!(classify(b"   \n".to_vec(), false).is_none());
        assert!(classify(vec![0xff, 0xfe], false).is_none());
        assert!(classify(b"file:///home/a\nfile:///home/b\n".to_vec(), false).is_none());
        assert!(classify(b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec(), true).is_none());
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&1920u32.to_be_bytes());
        png.extend_from_slice(&1080u32.to_be_bytes());
        let p = classify(png, true).unwrap();
        assert_eq!(
            (p.mime, p.ext, p.w, p.h),
            ("image/png", "png", Some(1920), Some(1080))
        );
        assert_eq!(
            classify(vec![0xff, 0xd8, 0xff, 0xe0], true).unwrap().ext,
            "jpg"
        );
        assert_eq!(preview("a\tb\x1bc"), "a b c");
        assert_eq!(preview(&"é".repeat(500)).chars().count(), PREVIEW_CHARS);
    }

    #[test]
    fn capped_read() {
        let mut big = std::io::Cursor::new(vec![b'a'; 11]);
        assert!(read_capped(&mut big, 10).unwrap().is_none());
        assert_eq!(big.position(), 11);
        let mut ok = std::io::Cursor::new(vec![b'a'; 10]);
        assert_eq!(read_capped(&mut ok, 10).unwrap().unwrap().len(), 10);
    }

    #[test]
    fn invisible_and_homographs() {
        assert_eq!(preview("safe\u{202E}gpj.exe"), "safegpj.exe");
        assert_eq!(preview("a\u{200B}b\u{FEFF}"), "ab");
        assert_eq!(link_host("https://gіthub.com/x"), None, "Cyrillic i");
        assert_eq!(text("https://gіthub.com/x").kind, Kind::Text);
    }

    #[test]
    fn blob_repair_and_corrupt_index() {
        let c = tmp("repair");
        let (id, _) = c.add(text("intact"), 1).unwrap();
        let e = c.entry(id).unwrap();
        fs::write(c.blob_path(&e), "int").unwrap(); // truncated by a crash
        c.add(text("intact"), 2).unwrap();
        assert_eq!(fs::read_to_string(c.blob_path(&e)).unwrap(), "intact");
        fs::write(c.dir.join("index.json"), "{not json").unwrap();
        assert!(c.list(None, None, 10).is_err());
        c.add(text("after"), 3).unwrap();
        assert_eq!(c.list(None, None, 10).unwrap().items.len(), 1);
        assert!(
            fs::read_dir(&c.dir)
                .unwrap()
                .flatten()
                .any(|d| d.file_name().to_string_lossy().starts_with(CORRUPT))
        );
        fs::remove_dir_all(c.dir.parent().unwrap()).ok();
    }

    #[test]
    fn limit_is_per_kind() {
        let c = tmp("limit");
        c.add(text("https://a.org"), 1).unwrap();
        for i in 0..5 {
            c.add(text(&format!("t{i}")), 10 + i).unwrap();
        }
        let l = c.list(None, None, 2).unwrap();
        assert_eq!(l.items.len(), 3);
        assert!(l.items.iter().any(|i| i.kind == Kind::Link));
        fs::remove_dir_all(c.dir.parent().unwrap()).ok();
    }

    #[test]
    fn pause_blocks_ingest_and_survives_config_put() {
        let c = tmp("pause");
        c.put_config(r#"{"enabled":true}"#).unwrap();
        assert_eq!(parse_duration("15m").unwrap(), 900);
        assert_eq!(parse_duration("1h").unwrap(), 3600);
        assert_eq!(parse_duration("45").unwrap(), 45);
        for bad in ["", "m", "-1m", "1d", "9999999h"] {
            assert!(parse_duration(bad).is_err(), "{bad}");
        }
        assert!(c.pause(MAX_PAUSE + 1).is_err());
        let until = c.pause(600).unwrap().unwrap();
        assert!(until > now());
        c.put_config(r#"{"retention_days":7}"#).unwrap();
        assert_eq!(
            c.config().unwrap().paused_until,
            Some(until),
            "config put keeps the pause"
        );
        assert!(
            c.put_config(r#"{"paused_until":1}"#).is_err(),
            "only clip pause sets it"
        );
        assert_eq!(c.list(None, None, 5).unwrap().paused_until, Some(until));
        // Simulated ingest gate (CLIPBOARD_STATE is owned by another test).
        assert!(c.config().unwrap().paused(now()).is_some());
        assert_eq!(c.pause(0).unwrap(), None);
        assert!(c.list(None, None, 5).unwrap().paused_until.is_none());
        let mut cfg = c.config().unwrap();
        cfg.paused_until = Some(1);
        assert!(
            cfg.paused(now()).is_none(),
            "past deadline means not paused"
        );
        fs::remove_dir_all(c.dir.parent().unwrap()).ok();
    }

    #[test]
    fn scan_fixes() {
        // S-04, S-12
        assert_eq!(link_host("https://evil.com\\@trusted.com/x"), None);
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&u32::MAX.to_be_bytes());
        png.extend_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(png_size(&png), None);
        // S-06
        let c = tmp("scan");
        fs::create_dir_all(c.config_path.parent().unwrap()).unwrap();
        fs::write(&c.config_path, r#"{"retenton_days":1}"#).unwrap();
        assert!(c.config().is_err());
        fs::remove_file(&c.config_path).unwrap();
        c.put_config(r#"{"enabled":true}"#).unwrap();
        // S-03: corrupt index keeps blobs
        let (a, _) = c.add(text("keep me"), 1).unwrap();
        let blob = c.blob_path(&c.entry(a).unwrap());
        fs::write(c.dir.join("index.json"), "{broken").unwrap();
        c.add(text("new"), 2).unwrap();
        assert!(!blob.exists(), "moved, not deleted");
        let aside: Vec<_> = fs::read_dir(&c.dir)
            .unwrap()
            .flatten()
            .filter(|d| d.file_name().to_string_lossy().starts_with(CORRUPT))
            .collect();
        assert_eq!(aside.len(), 1);
        assert_eq!(
            fs::read_dir(aside[0].path().join("blobs")).unwrap().count(),
            1
        );
        assert_eq!(
            c.list(None, None, 10).unwrap().items.len(),
            1,
            "fresh shelf works"
        );
        // newer schema is refused, not overwritten
        fs::write(
            c.dir.join("index.json"),
            r#"{"schema":2,"rev":1,"next_id":1,"items":[]}"#,
        )
        .unwrap();
        assert_eq!(c.add(text("x"), 3).unwrap_err().code, "newer_schema");
        fs::remove_file(c.dir.join("index.json")).unwrap();
        // S-08: tampered entry dropped on load
        let (b, _) = c.add(text("y"), 4).unwrap();
        let mut v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(c.dir.join("index.json")).unwrap()).unwrap();
        v["items"][0]["sha256"] = "../../etc/passwd".into();
        fs::write(c.dir.join("index.json"), v.to_string()).unwrap();
        assert!(c.entry(b).is_err());
        // S-18
        let (d, _) = c.add(text("gone"), 5).unwrap();
        c.remove(&[d]).unwrap();
        assert_eq!(c.purge().unwrap().0, 1);
        assert!(c.undo().unwrap().0 == 0);
        fs::remove_dir_all(c.dir.parent().unwrap()).ok();
    }

    #[test]
    fn caps_drop_oldest_unpinned() {
        let mut idx = Index::default();
        for id in 0..(MAX_ITEMS as u64 + 3) {
            idx.items.push(Entry {
                id,
                kind: Kind::Text,
                created: id,
                updated: id,
                pinned: id == 0,
                preview: String::new(),
                host: None,
                size: 1,
                mime: "text/plain".into(),
                sha256: format!("{id:064x}"),
                ext: "txt".into(),
                w: None,
                h: None,
                trashed: None,
            });
        }
        prune_index(&mut idx, 30, 10);
        assert_eq!(idx.items.iter().filter(|e| !e.pinned).count(), MAX_ITEMS);
        assert!(idx.items.iter().any(|e| e.id == 0), "pinned kept");
        assert!(
            !idx.items.iter().any(|e| e.id == 1),
            "oldest unpinned dropped"
        );
    }

    #[test]
    fn ids() {
        assert_eq!(parse_id("417").unwrap(), 417);
        for bad in ["", "-1", "1a", "../x", "99999999999999999999"] {
            assert!(parse_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn store_dedupe_pin_rm_undo() {
        let c = tmp("store");
        let (a, new) = c.add(text("hello"), 100).unwrap();
        assert!(new);
        let (b, _) = c.add(text("https://x.org"), 200).unwrap();
        let (a2, new2) = c.add(text("hello"), 300).unwrap();
        assert_eq!((a2, new2), (a, false));
        let l = c.list(None, None, 10).unwrap();
        assert_eq!(l.items.len(), 2);
        assert_eq!(l.items[0].id, a.to_string());
        assert_eq!(l.items[1].host.as_deref(), Some("x.org"));

        c.set_pinned(&[b], true).unwrap();
        assert_eq!(c.list(None, None, 10).unwrap().items[0].id, b.to_string());
        assert_eq!(c.list(Some(Kind::Text), None, 10).unwrap().items.len(), 1);
        assert_eq!(c.list(None, Some("HELL"), 10).unwrap().items.len(), 1);
        assert_eq!(c.list(None, Some("x.org"), 10).unwrap().items.len(), 1);

        c.remove(&[a]).unwrap();
        assert_eq!(c.list(None, None, 10).unwrap().items.len(), 1);
        assert!(c.set_pinned(&[a], true).is_err());
        assert_eq!(c.undo().unwrap().0, 1);
        assert_eq!(c.list(None, None, 10).unwrap().items.len(), 2);
        assert_eq!(c.undo().unwrap().0, 0);

        let rev = c.clear(false).unwrap();
        assert_eq!(fs::read_to_string(&c.rev_path).unwrap(), rev.to_string());
        let l = c.list(None, None, 10).unwrap();
        assert_eq!(l.items.len(), 1, "pinned survives clear");
        c.clear(true).unwrap();
        assert!(c.list(None, None, 10).unwrap().items.is_empty());
        assert_eq!(fs::read_dir(c.dir.join("blobs")).unwrap().count(), 0);
        fs::remove_dir_all(c.dir.parent().unwrap()).ok();
    }

    #[test]
    fn retention_and_sweep() {
        let c = tmp("ret");
        let day = 86400;
        let (old, _) = c.add(text("old"), 0).unwrap();
        let (pin, _) = c.add(text("pinned"), 0).unwrap();
        c.set_pinned(&[pin], true).unwrap();
        // 30 days later the next copy prunes the old unpinned one.
        c.add(text("new"), 31 * day).unwrap();
        let ids: Vec<String> = c
            .list(None, None, 10)
            .unwrap()
            .items
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert!(!ids.contains(&old.to_string()));
        assert!(ids.contains(&pin.to_string()));
        assert_eq!(fs::read_dir(c.dir.join("blobs")).unwrap().count(), 2);
        fs::remove_dir_all(c.dir.parent().unwrap()).ok();
    }

    #[test]
    fn trash_purged_after_a_day() {
        let mut idx = Index::default();
        let e = |id, trashed| Entry {
            id,
            kind: Kind::Text,
            created: 0,
            updated: 1000,
            pinned: false,
            preview: String::new(),
            host: None,
            size: 1,
            mime: "text/plain".into(),
            sha256: format!("{id}"),
            ext: "txt".into(),
            w: None,
            h: None,
            trashed,
        };
        idx.items = vec![e(1, Some(1000)), e(2, Some(1000 + TRASH_SECS)), e(3, None)];
        idx.last_removed = vec![1, 2];
        prune_index(&mut idx, 30, 1001 + TRASH_SECS);
        assert_eq!(
            idx.items.iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(idx.last_removed, vec![2]);
    }

    #[test]
    fn config_rules() {
        let c = tmp("cfg");
        assert!(!c.config().unwrap().enabled);
        let n = c
            .put_config(r#"{"enabled":true,"retention_days":7}"#)
            .unwrap();
        assert!(n.enabled && n.text && n.retention_days == 7);
        assert!(c.put_config(r#"{"retention_days":5}"#).is_err());
        assert!(c.put_config(r#"{"command":"rm -rf"}"#).is_err());
        assert!(c.put_config("[]").is_err());
        assert_eq!(c.config().unwrap().retention_days, 7);
        fs::remove_dir_all(c.dir.parent().unwrap()).ok();
    }

    #[test]
    fn ingest_skips_unless_data_and_enabled() {
        let c = tmp("ing");
        // SAFETY: tests touching CLIPBOARD_STATE run only here.
        unsafe { std::env::set_var("CLIPBOARD_STATE", "data") };
        assert!(
            ingest(&c, false, &mut "x".as_bytes()).unwrap().is_none(),
            "disabled"
        );
        c.put_config(r#"{"enabled":true}"#).unwrap();
        for state in ["sensitive", "nil", "clear", "", "weird"] {
            unsafe { std::env::set_var("CLIPBOARD_STATE", state) };
            assert!(
                ingest(&c, false, &mut "pw".as_bytes()).unwrap().is_none(),
                "{state}"
            );
        }
        unsafe { std::env::set_var("CLIPBOARD_STATE", "data") };
        assert!(ingest(&c, false, &mut "kept".as_bytes()).unwrap().is_some());
        c.put_config(r#"{"text":false}"#).unwrap();
        assert!(ingest(&c, false, &mut "off".as_bytes()).unwrap().is_none());
        unsafe { std::env::remove_var("CLIPBOARD_STATE") };
        assert_eq!(c.list(None, None, 10).unwrap().items.len(), 1);
        fs::remove_dir_all(c.dir.parent().unwrap()).ok();
    }
}
