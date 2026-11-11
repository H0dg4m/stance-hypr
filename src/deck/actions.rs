//! Buttons to actions: maps validated buttons to `stance` argv and runs it
//! by spawning our own binary (never a shell, hard timeout), and builds the
//! state snapshot the phone sees. Display changes keep their confirm timer;
//! the phone learns about it through `State::pending`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use super::config::{self, Action, Button as Cfg, NightOp};
use super::proto::{Button, Pending, State};
use crate::error::{Error, Result};
use crate::{modes, store, tool};

const ACTION_TIMEOUT: Duration = Duration::from_secs(30);
const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
/// Snapshots newer than this are reused (many phones, one poll loop).
const SNAPSHOT_TTL: Duration = Duration::from_millis(400);

struct Snap {
    at: Instant,
    state: State,
    /// Deadline of the pending change, so a cached snapshot can count down.
    pending_deadline: Option<Instant>,
    /// An action ran since: the next snapshot must query again.
    stale: bool,
}

/// A button's command, ready to run.
pub struct Prepared {
    exe: std::path::PathBuf,
    args: Vec<String>,
    label: String,
}

impl Prepared {
    pub fn run(&self) -> Result<String> {
        let refs: Vec<&str> = self.args.iter().map(String::as_str).collect();
        let out = tool::run_tool(&self.exe, &refs, ACTION_TIMEOUT)?;
        let line = out
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("");
        Ok(if line.is_empty() {
            format!(
                "{}: done",
                if self.label.is_empty() {
                    "ok"
                } else {
                    &self.label
                }
            )
        } else {
            crate::model::sanitize(line)
        })
    }
}

pub struct Deck {
    buttons: Vec<Cfg>,
    exe: PathBuf,
    stamp: Stamp,
    snap: Mutex<Option<Snap>>,
    version: Mutex<u64>,
}

/// What `changed_on_disk` compares.
#[derive(Debug, Clone, PartialEq)]
struct Stamp {
    path: PathBuf,
    file: Option<SystemTime>,
    /// Only when the file is missing and the default set follows the modes.
    modes: Option<SystemTime>,
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

impl Deck {
    /// `$XDG_CONFIG_HOME/stance/deck.toml`; a missing file gives the default
    /// set (night toggle, keep, revert, one button per mode).
    pub fn load() -> Result<Deck> {
        let exe = std::env::current_exe()
            .map_err(|e| Error::new("io", format!("cannot find own binary: {e}")))?;
        Self::load_from(&store::config_dir()?.join("deck.toml"), exe)
    }

    pub fn load_from(path: &Path, exe: PathBuf) -> Result<Deck> {
        let (buttons, modes_stamp) = match mtime(path) {
            Some(_) => (config::parse(&store::read_checked(path)?)?, None),
            None => (default_buttons(), modes_dir_stamp()),
        };
        Ok(Deck {
            buttons,
            exe,
            stamp: Stamp {
                path: path.to_path_buf(),
                file: mtime(path),
                modes: modes_stamp,
            },
            snap: Mutex::new(None),
            version: Mutex::new(0),
        })
    }

    /// deck.toml (or, with the default set, the modes folder) changed since
    /// loading: the caller reloads.
    /// The buttons in file form, and whether they come from deck.toml
    /// (false: the default set).
    pub fn configs(&self) -> (Vec<config::ButtonCfg>, bool) {
        (
            self.buttons.iter().map(|b| b.to_cfg()).collect(),
            self.stamp.file.is_some(),
        )
    }

    pub fn changed_on_disk(&self) -> bool {
        let file = mtime(&self.stamp.path);
        let modes = if file.is_none() {
            modes_dir_stamp()
        } else {
            None
        };
        file != self.stamp.file || modes != self.stamp.modes
    }

    /// For the phone, with lit state from the last snapshot (none yet means
    /// no `on`).
    pub fn buttons(&self) -> Vec<Button> {
        let st = self
            .snap
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|s| s.state.clone()));
        self.buttons
            .iter()
            .map(|b| Button {
                id: b.id.clone(),
                label: b.label.clone(),
                icon: b.icon.clone(),
                on: st.as_ref().and_then(|s| lit(&b.action, s)),
            })
            .collect()
    }

    /// Runs the button's action; returns a short message for the phone.
    #[cfg(test)]
    pub fn execute(&self, id: &str) -> Result<String> {
        let p = self.prepare(id)?;
        let r = p.run();
        self.mark_stale();
        r
    }

    /// The command for button `id`, to run without holding the deck (the
    /// session worker runs it, so other sessions are not blocked meanwhile).
    pub fn prepare(&self, id: &str) -> Result<Prepared> {
        let b = self
            .buttons
            .iter()
            .find(|b| b.id == id)
            .ok_or_else(|| Error::new("deck_button", "no such button"))?;
        Ok(self.prepared(&b.label, &b.action))
    }

    /// `confirm` / `revert` for the proto messages of the same name,
    /// independent of the buttons in deck.toml.
    pub fn prepare_builtin(&self, what: &str) -> Result<Prepared> {
        let action = match what {
            "confirm" => Action::Confirm,
            "revert" => Action::Revert,
            _ => return Err(Error::new("deck_button", "no such action")),
        };
        Ok(self.prepared("", &action))
    }

    #[cfg(test)]
    pub fn execute_builtin(&self, what: &str) -> Result<String> {
        let p = self.prepare_builtin(what)?;
        let r = p.run();
        self.mark_stale();
        r
    }

    fn prepared(&self, label: &str, action: &Action) -> Prepared {
        Prepared {
            exe: self.exe.clone(),
            args: argv(action),
            label: label.to_string(),
        }
    }

    /// After an action: the next snapshot must see the change.
    pub fn mark_stale(&self) {
        if let Ok(mut g) = self.snap.lock()
            && let Some(s) = g.as_mut()
        {
            s.stale = true;
        }
    }

    /// Active mode, pending change and night light. Two short `stance`
    /// calls, cached for a moment. `version` grows when something other than
    /// the pending countdown changes. A failed query leaves that part empty
    /// (a missing Hyprland shows as "nothing active").
    pub fn snapshot(&self) -> State {
        if let Ok(g) = self.snap.lock()
            && let Some(s) = g.as_ref()
            && !s.stale
            && s.at.elapsed() < SNAPSHOT_TTL
        {
            return with_countdown(s);
        }
        let status = self.query(&["status", "--json"]);
        let night = self.query(&["night", "status", "--json"]);
        let mut st = build_state(status.as_ref(), night.as_ref());
        let deadline = st
            .pending
            .as_ref()
            .map(|p| Instant::now() + Duration::from_millis(p.remaining_ms));
        // Version: bump when the comparable part differs from the last one.
        let mut ver = self.version.lock().unwrap_or_else(|e| e.into_inner());
        let mut slot = self.snap.lock().unwrap_or_else(|e| e.into_inner());
        let prev = slot.as_ref().map(|s| &s.state);
        if *ver == 0 || prev.is_none_or(|p| differs(p, &st)) {
            *ver += 1;
        }
        st.version = *ver;
        let snap = Snap {
            at: Instant::now(),
            state: st.clone(),
            pending_deadline: deadline,
            stale: false,
        };
        *slot = Some(snap);
        st
    }

    fn query(&self, args: &[&str]) -> Option<Value> {
        let out = tool::run_tool(&self.exe, args, QUERY_TIMEOUT).ok()?;
        let v: Value = serde_json::from_str(&out).ok()?;
        (v.get("ok") == Some(&Value::Bool(true)))
            .then(|| v.get("data").cloned())
            .flatten()
    }
}

fn with_countdown(s: &Snap) -> State {
    let mut st = s.state.clone();
    if let (Some(p), Some(d)) = (st.pending.as_mut(), s.pending_deadline) {
        p.remaining_ms = d.saturating_duration_since(Instant::now()).as_millis() as u64;
    }
    st
}

/// Everything except the pending countdown and the version.
fn differs(a: &State, b: &State) -> bool {
    a.active_mode != b.active_mode
        || a.night != b.night
        || a.pending.as_ref().map(|p| &p.label) != b.pending.as_ref().map(|p| &p.label)
}

/// Pure part of `snapshot`: `status --json` and `night status --json` data.
fn build_state(status: Option<&Value>, night: Option<&Value>) -> State {
    let mut st = State::default();
    if let Some(s) = status {
        st.active_mode = s
            .get("active_mode")
            .and_then(Value::as_str)
            .map(crate::model::sanitize);
        if let Some(p) = s.get("pending").filter(|p| p.is_object()) {
            st.pending = Some(Pending {
                label: crate::model::sanitize(
                    p.get("label").and_then(Value::as_str).unwrap_or("change"),
                ),
                remaining_ms: p.get("remaining_ms").and_then(Value::as_u64).unwrap_or(0),
            });
        }
    }
    if let Some(n) = night
        && n.get("on") == Some(&Value::Bool(true))
    {
        st.night = n
            .get("temperature")
            .and_then(Value::as_u64)
            .map(|k| k as u32);
    }
    st
}

/// Lit state for toggles: night light and the active mode.
fn lit(a: &Action, s: &State) -> Option<bool> {
    match a {
        Action::Night(NightOp::Toggle | NightOp::On(_) | NightOp::Off) => Some(s.night.is_some()),
        Action::Mode(n) => Some(s.active_mode.as_deref() == Some(n.as_str())),
        _ => None,
    }
}

/// The only place an action becomes arguments to our own binary.
fn argv(a: &Action) -> Vec<String> {
    let s = |x: &str| x.to_string();
    match a {
        Action::Mode(n) => vec![s("mode"), s("apply"), n.clone()],
        Action::Night(op) => match op {
            NightOp::Toggle => vec![s("night"), s("toggle")],
            NightOp::Off => vec![s("night"), s("off")],
            NightOp::On(None) => vec![s("night"), s("on")],
            NightOp::On(Some(k)) => vec![s("night"), s("on"), k.to_string()],
            NightOp::Set(k) => vec![s("night"), s("set"), k.to_string()],
        },
        Action::Brightness { monitor, value } => {
            vec![s("brightness"), monitor.clone(), value.clone()]
        }
        Action::Audio(sink) => vec![s("audio"), s("set"), sink.clone()],
        Action::Layout {
            kind,
            monitor,
            reference,
        } => {
            let mut v = vec![s("layout"), kind.clone(), monitor.clone()];
            v.extend(reference.clone());
            v
        }
        Action::Confirm => vec![s("confirm")],
        Action::Revert => vec![s("revert")],
    }
}

fn modes_dir_stamp() -> Option<SystemTime> {
    modes::modes_dir().ok().and_then(|d| mtime(&d))
}

fn default_buttons() -> Vec<Cfg> {
    let mk = |id: &str, label: &str, icon: &str, action| Cfg {
        id: id.into(),
        label: label.into(),
        icon: Some(icon.into()),
        action,
    };
    let mut v = vec![
        mk(
            "night",
            "Night light",
            "bedtime",
            Action::Night(NightOp::Toggle),
        ),
        mk("keep", "Keep", "check", Action::Confirm),
        mk("undo", "Revert", "undo", Action::Revert),
    ];
    for m in modes::list().unwrap_or_default() {
        let id = format!("mode-{}", m.name);
        if m.error.is_some()
            || id.len() > super::proto::MAX_ID
            || v.len() >= super::proto::MAX_BUTTONS
        {
            continue;
        }
        let label: String = m.name.chars().take(super::proto::MAX_LABEL).collect();
        v.push(mk(&id, &label, "tune", Action::Mode(m.name)));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("stance-deck-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        store::ensure_private_dir(&d).unwrap();
        d
    }

    /// A fake `stance` that records its argv and answers status/night JSON.
    fn fake_exe(dir: &Path, pending: bool) -> PathBuf {
        let p = dir.join("fake-stance");
        let status = if pending {
            r#"{"ok":true,"data":{"pending":{"label":"layout left-of","remaining_ms":9000},"active_mode":"work"}}"#
        } else {
            r#"{"ok":true,"data":{"pending":null,"active_mode":"work"}}"#
        };
        let script = format!(
            "#!/bin/sh\necho \"$@\" >> {log}\ncase \"$1 $2\" in\n\"status --json\") echo '{status}';;\n\"night status\") echo '{{\"ok\":true,\"data\":{{\"on\":true,\"temperature\":3500}}}}';;\n\"sleep \"*) sleep 5;;\n*) echo \"done: $*\";;\nesac\n",
            log = dir.join("log").display()
        );
        std::fs::write(&p, script).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn write_cfg(dir: &Path, body: &str) -> PathBuf {
        let p = dir.join("deck.toml");
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        p
    }

    const CFG: &str = "[[button]]\nid=\"night\"\nlabel=\"Night\"\naction=\"night\"\n\
        [[button]]\nid=\"work\"\nlabel=\"Work\"\naction=\"mode\"\nname=\"work\"\n\
        [[button]]\nid=\"lay\"\nlabel=\"L\"\naction=\"layout\"\nkind=\"left-of\"\nmonitor=\"DP 1\"\nreference=\"eDP-1\"\n\
        [[button]]\nid=\"keep\"\nlabel=\"Keep\"\naction=\"confirm\"\n";

    #[test]
    fn argv_mapping() {
        assert_eq!(argv(&Action::Mode("w".into())), ["mode", "apply", "w"]);
        assert_eq!(
            argv(&Action::Night(NightOp::On(Some(3000)))),
            ["night", "on", "3000"]
        );
        assert_eq!(
            argv(&Action::Brightness {
                monitor: "DP-1".into(),
                value: "-10".into()
            }),
            ["brightness", "DP-1", "-10"]
        );
        assert_eq!(
            argv(&Action::Audio("hdmi out".into())),
            ["audio", "set", "hdmi out"]
        );
        assert_eq!(
            argv(&Action::Layout {
                kind: "right-of".into(),
                monitor: "A".into(),
                reference: Some("B".into())
            }),
            ["layout", "right-of", "A", "B"]
        );
        assert_eq!(argv(&Action::Confirm), ["confirm"]);
    }

    #[test]
    fn executes_without_a_shell() {
        let d = tmp("exec");
        let exe = fake_exe(&d, false);
        // an argument with shell metacharacters stays one argv element
        let cfg = write_cfg(&d, CFG);
        let deck = Deck::load_from(&cfg, exe).unwrap();
        let msg = deck.execute("lay").unwrap();
        assert!(msg.starts_with("done: layout left-of"), "{msg}");
        let log = std::fs::read_to_string(d.join("log")).unwrap();
        assert!(log.contains("layout left-of DP 1 eDP-1"));
        assert_eq!(deck.execute("nope").unwrap_err().code, "deck_button");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn builtins_run_confirm_and_revert_only() {
        let d = tmp("bi");
        let exe = fake_exe(&d, false);
        let deck = Deck::load_from(&write_cfg(&d, CFG), exe).unwrap();
        assert!(deck.execute_builtin("confirm").is_ok());
        assert!(deck.execute_builtin("revert").is_ok());
        assert!(deck.execute_builtin("night").is_err());
        let log = std::fs::read_to_string(d.join("log")).unwrap();
        assert!(log.contains("confirm") && log.contains("revert"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn action_timeout_kills() {
        let d = tmp("to");
        let exe = fake_exe(&d, false);
        // run_tool directly with a short timeout (ACTION_TIMEOUT is long)
        let r = tool::run_tool(&exe, &["sleep", "x"], Duration::from_millis(150));
        assert_eq!(r.unwrap_err().code, "tool_timeout");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn snapshot_version_and_lit_buttons() {
        let d = tmp("snap");
        let exe = fake_exe(&d, true);
        let deck = Deck::load_from(&write_cfg(&d, CFG), exe).unwrap();
        assert!(deck.buttons().iter().all(|b| b.on.is_none()));
        let a = deck.snapshot();
        assert_eq!(a.version, 1);
        assert_eq!(a.active_mode.as_deref(), Some("work"));
        assert_eq!(a.night, Some(3500));
        assert_eq!(a.pending.as_ref().unwrap().label, "layout left-of");
        let b = deck.buttons();
        assert_eq!(b[0].on, Some(true)); // night
        assert_eq!(b[1].on, Some(true)); // active mode work
        assert_eq!(b[3].on, None); // confirm
        // unchanged (only the countdown moves): same version, also past the cache
        std::thread::sleep(SNAPSHOT_TTL + Duration::from_millis(50));
        let c = deck.snapshot();
        assert_eq!(c.version, 1);
        assert!(c.pending.unwrap().remaining_ms <= 9000);
        // execute drops the cache; same content keeps the version
        deck.execute("keep").unwrap();
        assert_eq!(deck.snapshot().version, 1);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn build_state_handles_missing_data() {
        let s = build_state(None, None);
        assert_eq!(s, State::default());
        let st = build_state(
            Some(&json!({"pending":null,"active_mode":null})),
            Some(&json!({"on":false,"temperature":null})),
        );
        assert!(st.pending.is_none() && st.night.is_none() && st.active_mode.is_none());
    }

    #[test]
    fn reload_detection() {
        let d = tmp("mt");
        let exe = fake_exe(&d, false);
        let cfg = write_cfg(&d, CFG);
        let deck = Deck::load_from(&cfg, exe).unwrap();
        assert!(!deck.changed_on_disk());
        std::thread::sleep(Duration::from_millis(1100));
        write_cfg(&d, &format!("{CFG}\n"));
        assert!(deck.changed_on_disk());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn bad_file_is_an_error() {
        let d = tmp("bad");
        let cfg = write_cfg(&d, "[[button]]\nid=\"a\"\nlabel=\"A\"\naction=\"shell\"\n");
        assert!(Deck::load_from(&cfg, PathBuf::from("/bin/true")).is_err());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn missing_file_gives_defaults() {
        let d = tmp("def");
        let deck = Deck::load_from(&d.join("deck.toml"), PathBuf::from("/bin/true")).unwrap();
        let ids: Vec<_> = deck.buttons().into_iter().map(|b| b.id).collect();
        assert!(ids.starts_with(&["night".into(), "keep".into(), "undo".into()]));
        std::fs::remove_dir_all(&d).ok();
    }
}
