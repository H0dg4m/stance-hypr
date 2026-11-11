//! `stance events`: one JSON line per change, for UIs that want to react
//! instead of polling or watching folders themselves.
//!
//! Sources: the backend's neutral events (screens added/removed, config
//! reloaded) and the state/config files the panel cares about, through
//! inotify. Everything blocks in the kernel, so an idle stream costs
//! nothing. The process ends when stdout is closed (a poll thread notices
//! a hung-up pipe even while nothing is being written).

use std::collections::HashMap;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::backend::{self, Event, Next};
use crate::error::{Error, Result};
use crate::store;

/// Bumped on breaking changes to the lines (docs/protocol.md).
const SCHEMA: u32 = 1;

fn emit(v: &Value) {
    // `println!` writes a whole line under the stdout lock. A closed pipe
    // kills the process (SIGPIPE keeps its default action for this command).
    println!("{v}");
}

/// What a file change in one of the watched folders means.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dir {
    State,
    Config,
    Themes,
    Modes,
}

fn file_event(dir: Dir, name: &str) -> Option<&'static str> {
    if name.starts_with('.') {
        return None; // atomic-write temp files
    }
    match (dir, name) {
        (Dir::State, "pending.json") => Some("pending"),
        (Dir::State, "active.json") => Some("active"),
        (Dir::State, "clip.rev") => Some("clip"),
        (Dir::Config, "features.json") => Some("features"),
        (Dir::Config, "panel.json") => Some("panel"),
        (Dir::Config, "clip.json") => Some("clip_config"),
        (Dir::Config, "rules.toml") => Some("rules"),
        (Dir::Themes, n) if n.ends_with(".json") => Some("theme"),
        (Dir::Modes, n) if n.ends_with(".toml") => Some("modes"),
        _ => None,
    }
}

struct Inotify {
    fd: i32,
    watches: HashMap<i32, Dir>,
    /// Config dir watch id, to notice `themes/` and `modes/` appearing.
    config_wd: Option<i32>,
}

const MASK: u32 = libc::IN_CLOSE_WRITE
    | libc::IN_MOVED_TO
    | libc::IN_MOVED_FROM
    | libc::IN_DELETE
    | libc::IN_CREATE;

impl Inotify {
    fn new() -> Result<Self> {
        // SAFETY: plain syscall; the fd is owned by this struct for the
        // life of the process.
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(Error::new(
                "io",
                format!("inotify: {}", std::io::Error::last_os_error()),
            ));
        }
        Ok(Self {
            fd,
            watches: HashMap::new(),
            config_wd: None,
        })
    }

    /// Watches `path` if it exists; returns its watch id.
    fn add(&mut self, path: &Path, dir: Dir) -> Option<i32> {
        let c = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: `c` is a valid NUL-terminated path for the call.
        let wd = unsafe { libc::inotify_add_watch(self.fd, c.as_ptr(), MASK) };
        (wd >= 0).then(|| {
            self.watches.insert(wd, dir);
            wd
        })
    }

    /// Blocks until something changes; returns the event names, once each.
    fn wait(&mut self, config: &Path) -> Result<Vec<&'static str>> {
        let mut buf = [0u8; 4096];
        // SAFETY: reads into a local buffer of the given length.
        let n = unsafe { libc::read(self.fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                return Ok(vec![]);
            }
            return Err(Error::new("io", format!("inotify: {e}")));
        }
        let mut out: Vec<&'static str> = Vec::new();
        let mut push = |e: &'static str| {
            if !out.contains(&e) {
                out.push(e);
            }
        };
        let mut off = 0usize;
        let n = n as usize;
        while off + 16 <= n {
            let wd = i32::from_ne_bytes(buf[off..off + 4].try_into().unwrap_or_default());
            let mask = u32::from_ne_bytes(buf[off + 4..off + 8].try_into().unwrap_or_default());
            let len =
                u32::from_ne_bytes(buf[off + 12..off + 16].try_into().unwrap_or_default()) as usize;
            let raw = buf.get(off + 16..off + 16 + len).unwrap_or_default();
            let name = String::from_utf8_lossy(raw.split(|b| *b == 0).next().unwrap_or_default())
                .into_owned();
            off += 16 + len;
            if mask & libc::IN_Q_OVERFLOW != 0 {
                // Lost track: tell everyone to re-read.
                for e in ["pending", "active", "clip", "features", "panel", "theme"] {
                    push(e);
                }
                continue;
            }
            if mask & libc::IN_IGNORED != 0 {
                self.watches.remove(&wd);
                continue;
            }
            let Some(dir) = self.watches.get(&wd).copied() else {
                continue;
            };
            if dir == Dir::Config && mask & libc::IN_ISDIR != 0 {
                // `themes/` or `modes/` created later: start watching it.
                match name.as_str() {
                    // Files may already be inside: report it as a change.
                    "themes" => {
                        self.add(&config.join("themes"), Dir::Themes);
                        push("theme");
                    }
                    "modes" => {
                        self.add(&config.join("modes"), Dir::Modes);
                        push("modes");
                    }
                    _ => {}
                }
                continue;
            }
            if let Some(e) = file_event(dir, &name) {
                push(e);
            }
        }
        Ok(out)
    }
}

/// Ends the process when the reader of stdout goes away, even if nothing
/// is being written. A terminal never hangs up, so this costs nothing there.
fn watch_stdout() {
    std::thread::spawn(|| {
        let mut p = libc::pollfd {
            fd: 1,
            events: 0,
            revents: 0,
        };
        loop {
            // SAFETY: one valid pollfd; infinite timeout.
            let r = unsafe { libc::poll(&mut p, 1, -1) };
            if r < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if r > 0 && p.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                std::process::exit(0);
            }
            if r < 0 {
                return;
            }
        }
    });
}

fn backend_thread(name: Option<String>, state_dir: PathBuf) {
    std::thread::spawn(move || {
        let run = || -> Result<()> {
            let b = backend::connect(name.as_deref(), &state_dir)?;
            let mut src = b.events()?;
            loop {
                match src.next(None)? {
                    Next::Closed => {
                        emit(&json!({ "event": "backend_closed" }));
                        return Ok(());
                    }
                    Next::Tick => {}
                    Next::Event(e) => emit(&match e {
                        Event::MonitorAdded { name } => {
                            json!({ "event": "monitors", "change": "added", "name": name })
                        }
                        Event::MonitorRemoved { name } => {
                            json!({ "event": "monitors", "change": "removed", "name": name })
                        }
                        Event::ConfigReloaded => json!({ "event": "monitors", "change": "reload" }),
                    }),
                }
            }
        };
        if let Err(e) = run() {
            emit(&json!({ "event": "backend_error", "code": e.code, "message": e.message }));
        }
    });
}

pub fn run(backend_name: Option<&str>, state_dir: &Path) -> Result<()> {
    store::ensure_private_dir(state_dir)?;
    let config = store::config_dir()?;
    store::ensure_private_dir(&config)?;
    let mut ino = Inotify::new()?;
    ino.add(state_dir, Dir::State);
    ino.config_wd = ino.add(&config, Dir::Config);
    ino.add(&config.join("themes"), Dir::Themes);
    ino.add(&config.join("modes"), Dir::Modes);

    // What the backend can tell us (a missing compositor is not an error:
    // the file events still work).
    let (name, has_events) = match backend::connect(backend_name, state_dir) {
        Ok(b) => (Some(b.name()), b.capabilities().events),
        Err(_) => (None, false),
    };
    emit(
        &json!({ "event": "hello", "schema": SCHEMA, "backend": name, "monitor_events": has_events }),
    );
    watch_stdout();
    if has_events {
        backend_thread(backend_name.map(String::from), state_dir.to_path_buf());
    }
    loop {
        for e in ino.wait(&config)? {
            emit(&json!({ "event": e }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_files_to_events() {
        assert_eq!(file_event(Dir::State, "pending.json"), Some("pending"));
        assert_eq!(file_event(Dir::State, "clip.rev"), Some("clip"));
        assert_eq!(file_event(Dir::Config, "panel.json"), Some("panel"));
        assert_eq!(file_event(Dir::Themes, "mine.json"), Some("theme"));
        assert_eq!(file_event(Dir::Modes, "desk.toml"), Some("modes"));
        assert_eq!(file_event(Dir::Modes, ".desk.toml.tmp"), None);
        assert_eq!(file_event(Dir::State, "daemon.lock"), None);
        assert_eq!(file_event(Dir::Themes, "notes.txt"), None);
    }
}
