//! Apply → verify → (confirm | revert).
//!
//! Every user-started change snapshots what it touches, applies, re-reads
//! the compositor state to verify, and arms a confirmation deadline. If
//! nobody confirms, the guardian process restores the snapshot. Automatic
//! changes (daemon) skip the timer.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::backend::{Backend, Caps, Notice, OptKey, OptValue};
use crate::error::{Error, Result};
use crate::model::Monitor;
use crate::power::PowerProfile;
use crate::spec::MonitorSpec;
use crate::store;

pub const DEFAULT_TIMEOUT: u64 = 12;
const VERIFY_FOR: Duration = Duration::from_millis(3000);
const POLL: Duration = Duration::from_millis(100);
/// A guardian should act on time; after this grace any stance call does it.
const SWEEP_GRACE_MS: u64 = 2000;

/// Everything one change may touch. Monitors not listed stay as they are.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    #[serde(default)]
    pub monitors: Vec<MonitorSpec>,
    #[serde(default)]
    pub options: BTreeMap<OptKey, OptValue>,
    #[serde(default)]
    pub power: Option<PowerProfile>,
    /// (workspace id, connector).
    #[serde(default)]
    pub workspaces: Vec<(u32, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    pub session: String,
    pub generation: u64,
    pub deadline_ms: u64,
    /// Length of the current confirmation window (for progress bars).
    #[serde(default)]
    pub timeout_ms: u64,
    pub label: String,
    /// State before the first unconfirmed change (oldest wins).
    pub snapshot: State,
    pub active_mode_before: Option<ActiveMode>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Active {
    pub session: String,
    pub mode: Option<String>,
    /// Whether the mode's Hyprland options / power profile were applied.
    #[serde(default)]
    pub extras: bool,
}

/// (mode name or none, extras applied).
pub type ActiveMode = (Option<String>, bool);

pub struct Ctx<'a> {
    pub backend: &'a dyn Backend,
    pub state_dir: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub label: String,
    pub changed: Vec<String>,
    pub pending: Option<PendingInfo>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PendingInfo {
    pub label: String,
    pub generation: u64,
    pub deadline_ms: u64,
    pub remaining_ms: u64,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Ctx<'_> {
    fn pending_path(&self) -> PathBuf {
        self.state_dir.join("pending.json")
    }

    fn active_path(&self) -> PathBuf {
        self.state_dir.join("active.json")
    }

    pub fn read_pending(&self) -> Option<Pending> {
        read_json(&self.pending_path())
    }

    fn write_pending(&self, p: &Pending) -> Result<()> {
        write_json(&self.pending_path(), p)
    }

    fn clear_pending(&self) {
        let _ = fs::remove_file(self.pending_path());
    }

    /// Active mode name and whether its extras were applied.
    pub fn active(&self) -> Option<(String, bool)> {
        let a: Active = read_json(&self.active_path())?;
        if a.session != self.backend.session_id() {
            return None;
        }
        Some((a.mode?, a.extras))
    }

    pub fn active_mode(&self) -> Option<String> {
        self.active().map(|(m, _)| m)
    }

    pub fn set_active(&self, (mode, extras): ActiveMode) -> Result<()> {
        let a = Active {
            session: self.backend.session_id().to_string(),
            mode,
            extras,
        };
        write_json(&self.active_path(), &a)
    }

    pub fn lock(&self) -> Result<store::StateLock> {
        store::lock_state_in(&self.state_dir)
    }

    pub fn pending_info(&self) -> Option<PendingInfo> {
        let p = self.read_pending()?;
        if p.session != self.backend.session_id() {
            return None;
        }
        Some(PendingInfo {
            label: p.label,
            generation: p.generation,
            deadline_ms: p.deadline_ms,
            remaining_ms: p.deadline_ms.saturating_sub(now_ms()),
        })
    }

    /// Handles leftovers: pending state from another compositor session is
    /// dropped (never applied to a new session); an expired one from this
    /// session is reverted once. Caller holds the lock.
    pub fn sweep(&self) -> Vec<String> {
        let Some(p) = self.read_pending() else {
            return vec![];
        };
        if p.session != self.backend.session_id() {
            self.clear_pending();
            return vec!["dropped pending change from a previous Hyprland session".into()];
        }
        if now_ms() > p.deadline_ms + SWEEP_GRACE_MS {
            let r = self.restore(&p);
            return vec![match r {
                Ok(()) => "an unconfirmed change had expired and was reverted".into(),
                Err(e) => format!("expired change could not be fully reverted: {}", e.message),
            }];
        }
        vec![]
    }

    /// Refuses changes the backend cannot make (code `unsupported`).
    fn check_caps(&self, t: &State) -> Result<()> {
        let c = self.backend.capabilities();
        let b = self.backend.name();
        Caps::require(c.displays || t.monitors.is_empty(), b, "changing displays")?;
        for m in &t.monitors {
            Caps::require(c.mirror || m.mirror.is_none(), b, "mirroring")?;
            Caps::require(c.vrr || m.vrr == 0, b, "variable refresh rate")?;
            Caps::require(c.transform || m.transform == 0, b, "rotation")?;
        }
        Caps::require(
            c.compositor_options || t.options.is_empty(),
            b,
            "compositor options",
        )?;
        Caps::require(
            c.workspaces || t.workspaces.is_empty(),
            b,
            "workspace binding",
        )
    }

    /// Captures the current value of everything `target` touches.
    fn capture(&self, target: &State, mons: &[Monitor]) -> Result<State> {
        let mut s = State::default();
        for want in &target.monitors {
            let cur = mons
                .iter()
                .find(|m| m.name == want.name)
                .ok_or_else(|| unknown(&want.name))?;
            s.monitors.push(MonitorSpec::from_monitor(cur));
        }
        for k in target.options.keys() {
            s.options.insert(*k, self.backend.get_option(*k)?);
        }
        if target.power.is_some() {
            s.power = Some(self.backend.power_get()?);
        }
        if !target.workspaces.is_empty() {
            let cur = self.backend.workspaces()?;
            for (id, _) in &target.workspaces {
                if let Some((_, mon)) = cur.iter().find(|(w, _)| w == id) {
                    s.workspaces.push((*id, mon.clone()));
                }
            }
        }
        Ok(s)
    }

    /// Applies `target` and verifies it. On failure restores what it
    /// changed and returns the error.
    fn execute(&self, target: &State, before: &State) -> Result<Vec<String>> {
        let mut changed = Vec::new();
        let res = self.execute_inner(target, &mut changed);
        if let Err(e) = res {
            let _ = self.execute_inner(before, &mut Vec::new());
            return Err(e);
        }
        Ok(changed)
    }

    fn execute_inner(&self, target: &State, changed: &mut Vec<String>) -> Result<()> {
        // 1. Monitors: enabled ones first so we never pass through "all off".
        let mons = self.backend.monitors()?;
        let mut order: Vec<&MonitorSpec> = target
            .monitors
            .iter()
            .filter(|w| !mons.iter().any(|m| m.name == w.name && satisfies(m, w)))
            .collect();
        order.sort_by_key(|s| !s.enabled);
        for spec in &order {
            if !mons.iter().any(|m| m.name == spec.name) {
                return Err(unknown(&spec.name));
            }
            self.backend.apply_monitor(spec)?;
            changed.push(spec.name.clone());
        }
        if !order.is_empty() {
            self.verify(&target.monitors)?;
        }
        // 2. Workspaces (after monitors: the target monitor must exist).
        for (id, mon) in &target.workspaces {
            self.backend.bind_workspace(*id, mon)?;
            changed.push(format!("workspace {id}"));
        }
        // 3. Compositor options.
        for (k, v) in &target.options {
            if self.backend.get_option(*k)? != *v {
                self.backend.set_option(*k, *v)?;
                if self.backend.get_option(*k)? != *v {
                    return Err(Error::new(
                        "not_applied",
                        format!("{} did not take effect", self.backend.option_name(*k)),
                    ));
                }
                changed.push(self.backend.option_name(*k).to_string());
            }
        }
        // 4. Power profile.
        if let Some(p) = target.power
            && self.backend.power_get()? != p
        {
            self.backend.power_set(p)?;
            changed.push(format!("power {}", p.as_str()));
        }
        Ok(())
    }

    /// `eval` says "ok" even when nothing happened, so re-read and compare.
    fn verify(&self, want: &[MonitorSpec]) -> Result<()> {
        let start = Instant::now();
        loop {
            let mons = self.backend.monitors()?;
            let bad: Vec<&str> = want
                .iter()
                .filter(|w| !mons.iter().any(|m| m.name == w.name && satisfies(m, w)))
                .map(|w| w.name.as_str())
                .collect();
            if bad.is_empty() {
                return Ok(());
            }
            if start.elapsed() >= VERIFY_FOR {
                return Err(Error::new(
                    "not_applied",
                    format!(
                        "Hyprland did not apply the change to {} (unsupported mode?)",
                        bad.join(", ")
                    ),
                ));
            }
            std::thread::sleep(POLL);
        }
    }

    /// Applies a user-started change with a confirmation deadline
    /// (`timeout_s == 0`: no timer, used for automatic changes).
    pub fn apply(
        &self,
        label: &str,
        target: &State,
        timeout_s: u64,
        mode_after: Option<ActiveMode>,
    ) -> Result<Outcome> {
        self.check_caps(target)?;
        let _lock = self.lock()?;
        let mut warnings = self.sweep();
        let mons = self.backend.monitors()?;
        // Screens seen for the first time; confirm / revert / automatic
        // changes record the final setup.
        crate::memory::record_new(&self.state_dir, &mons);
        let before = self.capture(target, &mons)?;
        let active_before = self.active().map(|(m, e)| (Some(m), e));
        let changed = self.execute(target, &before)?;
        // Automatic changes have no confirm step: what they set is final.
        if timeout_s == 0
            && let Ok(now) = self.backend.monitors()
        {
            crate::memory::record(&self.state_dir, &now);
        }

        if let Some(m) = mode_after
            && let Err(e) = self.set_active(m)
        {
            warnings.push(format!("could not record active mode: {}", e.message));
        }

        let mut pending = None;
        if timeout_s > 0 && !changed.is_empty() {
            let prev = self.read_pending();
            let (snapshot, generation, active_mode_before) = match prev {
                Some(p) => (
                    merge(p.snapshot, before),
                    p.generation + 1,
                    p.active_mode_before,
                ),
                None => (before, 1, active_before),
            };
            let p = Pending {
                session: self.backend.session_id().to_string(),
                generation,
                deadline_ms: now_ms() + timeout_s * 1000,
                timeout_ms: timeout_s * 1000,
                label: label.to_string(),
                snapshot,
                active_mode_before,
            };
            self.write_pending(&p)?;
            spawn_guardian(generation, &self.state_dir, self.backend.name())?;
            pending = self.pending_info();
        }
        Ok(Outcome {
            label: label.to_string(),
            changed,
            pending,
            warnings,
        })
    }

    pub fn confirm(&self) -> Result<bool> {
        let _lock = self.lock()?;
        self.sweep();
        let had = self.read_pending().is_some();
        self.clear_pending();
        if let Ok(mons) = self.backend.monitors() {
            crate::memory::record(&self.state_dir, &mons);
        }
        Ok(had)
    }

    /// Reverts the pending change now. Returns false if nothing was pending.
    pub fn revert(&self) -> Result<bool> {
        let _lock = self.lock()?;
        self.sweep();
        match self.read_pending() {
            Some(p) => self.restore(&p).map(|_| true),
            None => Ok(false),
        }
    }

    /// Restores a snapshot (verifying, one retry) and clears pending.
    /// Caller holds the lock.
    fn restore(&self, p: &Pending) -> Result<()> {
        // A monitor unplugged during the countdown has nothing to restore;
        // everything else still must come back.
        let mut snap = p.snapshot.clone();
        if let Ok(mons) = self.backend.monitors() {
            snap.monitors
                .retain(|s| mons.iter().any(|m| m.name == s.name));
            snap.workspaces
                .retain(|(_, mon)| mons.iter().any(|m| &m.name == mon));
        }
        let mut res = self.execute_inner(&snap, &mut Vec::new());
        if res.is_err() {
            std::thread::sleep(Duration::from_millis(300));
            res = self.execute_inner(&snap, &mut Vec::new());
        }
        self.clear_pending();
        if res.is_ok()
            && let Ok(mons) = self.backend.monitors()
        {
            crate::memory::record(&self.state_dir, &mons);
        }
        let _ = self.set_active(p.active_mode_before.clone().unwrap_or((None, false)));
        if res.is_err() {
            self.backend.notify(Notice::RevertFailed);
        }
        res
    }

    /// Body of `stance guardian <generation>`: waits for confirm or the
    /// deadline. Exits as soon as its generation is superseded.
    pub fn guard(&self, generation: u64) -> Result<()> {
        loop {
            let Some(p) = self.read_pending() else {
                return Ok(());
            };
            if p.generation != generation || p.session != self.backend.session_id() {
                return Ok(());
            }
            if now_ms() >= p.deadline_ms {
                let _lock = self.lock()?;
                match self.read_pending() {
                    Some(p) if p.generation == generation => {
                        let ok = self.restore(&p).is_ok();
                        if ok {
                            self.backend.notify(Notice::Reverted);
                        }
                    }
                    _ => {}
                }
                return Ok(());
            }
            // Compositor gone: nothing left to protect.
            if self.backend.version().is_err() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    }
}

/// Keeps the oldest captured value for everything; adds newly touched items.
fn merge(mut old: State, newer: State) -> State {
    for m in newer.monitors {
        if !old.monitors.iter().any(|o| o.name == m.name) {
            old.monitors.push(m);
        }
    }
    for (k, v) in newer.options {
        old.options.entry(k).or_insert(v);
    }
    if old.power.is_none() {
        old.power = newer.power;
    }
    for (id, mon) in newer.workspaces {
        if !old.workspaces.iter().any(|(o, _)| *o == id) {
            old.workspaces.push((id, mon));
        }
    }
    old
}

fn unknown(name: &str) -> Error {
    Error::new(
        "unknown_monitor",
        format!(
            "monitor `{}` is not connected",
            crate::model::sanitize(name)
        ),
    )
}

/// Does the live monitor match the wanted spec (within compositor rounding)?
pub fn satisfies(m: &Monitor, w: &MonitorSpec) -> bool {
    if m.enabled != w.enabled {
        return false;
    }
    if !w.enabled {
        return true;
    }
    if m.mirror_of != w.mirror {
        return false;
    }
    if w.mirror.is_some() {
        return true;
    }
    let vrr_ok = match w.vrr {
        0 => !m.vrr,
        1 => !m.vrr_capable || m.vrr,
        _ => true,
    };
    m.width == w.width
        && m.height == w.height
        && (m.refresh - w.refresh).abs() < 1.0
        && m.x == w.x
        && m.y == w.y
        // Hyprland snaps scales to values that divide the mode cleanly.
        && (m.scale - w.scale).abs() < 0.11
        && m.transform == w.transform
        && vrr_ok
}

fn spawn_guardian(generation: u64, state_dir: &Path, backend: &str) -> Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe()
        .map_err(|e| Error::new("io", format!("cannot locate own binary: {e}")))?;
    Command::new(exe)
        .arg("guardian")
        .arg(generation.to_string())
        .env("STANCE_STATE_DIR", state_dir)
        .env("STANCE_BACKEND", backend)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Own process group: a closing terminal or bar does not take it down.
        .process_group(0)
        .spawn()
        .map_err(|e| Error::new("io", format!("cannot start guardian: {e}")))?;
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let text = store::read_checked(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> Result<()> {
    let data = serde_json::to_vec(v).map_err(|e| Error::new("io", e.to_string()))?;
    store::atomic_write(path, &data, false)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub use crate::backend::fake::{Fake, mon};

    pub fn ctx<'a>(b: &'a Fake, tag: &str) -> Ctx<'a> {
        let d = std::env::temp_dir().join(format!("stance-apply-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        store::ensure_private_dir(&d).unwrap();
        Ctx {
            backend: b,
            state_dir: d,
        }
    }

    fn moved(b: &Fake, name: &str, x: i32) -> State {
        let m = b
            .mons
            .borrow()
            .iter()
            .find(|m| m.name == name)
            .unwrap()
            .clone();
        State {
            monitors: vec![MonitorSpec {
                x,
                ..MonitorSpec::from_monitor(&m)
            }],
            ..Default::default()
        }
    }

    #[test]
    fn apply_without_timer_then_nothing_pending() {
        let b = Fake::new();
        let c = ctx(&b, "a");
        let o = c.apply("t", &moved(&b, "B-1", 3000), 0, None).unwrap();
        assert_eq!(o.changed, vec!["B-1"]);
        assert!(o.pending.is_none());
        assert_eq!(b.mons.borrow()[1].x, 3000);
        // Already satisfied → no-op.
        assert!(
            c.apply("t", &moved(&b, "B-1", 3000), 0, None)
                .unwrap()
                .changed
                .is_empty()
        );
    }

    #[test]
    fn failed_verification_rolls_back() {
        let mut b = Fake::new();
        b.ignore = vec!["B-1".into()];
        let c = ctx(&b, "b");
        let mut t = moved(&b, "A-1", 500);
        t.monitors.extend(moved(&b, "B-1", 4000).monitors);
        let e = c.apply("t", &t, 0, None).unwrap_err();
        assert_eq!(e.code, "not_applied");
        assert_eq!(b.mons.borrow()[0].x, 0, "A-1 restored");
    }

    #[test]
    fn unknown_monitor_errors() {
        let b = Fake::new();
        let c = ctx(&b, "c");
        let mut t = moved(&b, "A-1", 10);
        t.monitors[0].name = "Z-9".into();
        assert_eq!(
            c.apply("t", &t, 0, None).unwrap_err().code,
            "unknown_monitor"
        );
    }

    #[test]
    fn pending_keeps_oldest_snapshot_and_revert_restores_it() {
        let b = Fake::new();
        let c = ctx(&b, "d");
        let mut t = moved(&b, "B-1", 3000);
        t.options.insert(OptKey::Blur, OptValue::Bool(false));
        // Write pending by hand instead of spawning a guardian.
        let before = c.capture(&t, &b.monitors().unwrap()).unwrap();
        c.execute(&t, &before).unwrap();
        c.write_pending(&Pending {
            session: "fake".into(),
            generation: 1,
            deadline_ms: now_ms() + 60_000,
            timeout_ms: 0,
            label: "one".into(),
            snapshot: before,
            active_mode_before: None,
        })
        .unwrap();
        // Second change on top: merge keeps x=1920 from the first snapshot.
        let t2 = moved(&b, "B-1", 5000);
        let before2 = c.capture(&t2, &b.monitors().unwrap()).unwrap();
        let merged = merge(c.read_pending().unwrap().snapshot, before2);
        assert_eq!(merged.monitors[0].x, 1920);

        assert!(c.revert().unwrap());
        assert_eq!(b.mons.borrow()[1].x, 1920);
        assert_eq!(b.get_option(OptKey::Blur).unwrap(), OptValue::Bool(true));
        assert!(c.read_pending().is_none());
        assert!(!c.revert().unwrap());
    }

    #[test]
    fn sweep_drops_other_session_and_reverts_expired() {
        let b = Fake::new();
        let c = ctx(&b, "e");
        let snap = moved(&b, "B-1", 1920);
        b.mons.borrow_mut()[1].x = 7777;
        let mut p = Pending {
            session: "other".into(),
            generation: 1,
            deadline_ms: 0,
            timeout_ms: 0,
            label: "x".into(),
            snapshot: snap,
            active_mode_before: None,
        };
        c.write_pending(&p).unwrap();
        c.sweep();
        assert!(c.read_pending().is_none());
        assert_eq!(b.mons.borrow()[1].x, 7777, "not applied to another session");

        p.session = "fake".into();
        c.write_pending(&p).unwrap();
        c.sweep();
        assert_eq!(b.mons.borrow()[1].x, 1920, "expired change reverted");
    }

    #[test]
    fn guardian_reverts_after_deadline() {
        let b = Fake::new();
        let c = ctx(&b, "f");
        let snap = moved(&b, "B-1", 1920);
        b.mons.borrow_mut()[1].x = 42;
        c.write_pending(&Pending {
            session: "fake".into(),
            generation: 3,
            deadline_ms: now_ms() + 200,
            timeout_ms: 0,
            label: "x".into(),
            snapshot: snap,
            active_mode_before: Some((Some("work".into()), true)),
        })
        .unwrap();
        c.guard(3).unwrap();
        assert_eq!(b.mons.borrow()[1].x, 1920);
        assert_eq!(c.active_mode().as_deref(), Some("work"));
    }

    #[test]
    fn revert_skips_unplugged_monitor() {
        let b = Fake::new();
        let c = ctx(&b, "i");
        let mut snap = moved(&b, "B-1", 1920);
        snap.monitors.extend(moved(&b, "A-1", 0).monitors);
        b.mons.borrow_mut()[1].x = 42;
        b.mons.borrow_mut()[0].x = 77;
        c.write_pending(&Pending {
            session: "fake".into(),
            generation: 1,
            deadline_ms: now_ms() + 60_000,
            timeout_ms: 0,
            label: "x".into(),
            snapshot: snap,
            active_mode_before: None,
        })
        .unwrap();
        b.mons.borrow_mut().remove(1); // B-1 unplugged
        assert!(c.revert().unwrap());
        assert_eq!(b.mons.borrow()[0].x, 0, "A-1 still restored");
    }

    #[test]
    fn guardian_exits_when_superseded() {
        let b = Fake::new();
        let c = ctx(&b, "g");
        b.mons.borrow_mut()[1].x = 42;
        c.write_pending(&Pending {
            session: "fake".into(),
            generation: 4,
            deadline_ms: now_ms(),
            timeout_ms: 0,
            label: "x".into(),
            snapshot: moved(&b, "B-1", 1920),
            active_mode_before: None,
        })
        .unwrap();
        c.guard(3).unwrap();
        assert_eq!(b.mons.borrow()[1].x, 42, "old guardian must not revert");
    }

    #[test]
    fn options_power_and_workspaces() {
        let b = Fake::new();
        let c = ctx(&b, "h");
        let t = State {
            options: BTreeMap::from([(OptKey::Blur, OptValue::Bool(false))]),
            power: Some(PowerProfile::Performance),
            workspaces: vec![(1, "B-1".into())],
            ..Default::default()
        };
        let o = c
            .apply("m", &t, 0, Some((Some("gaming".into()), true)))
            .unwrap();
        assert_eq!(o.changed.len(), 3);
        assert_eq!(*b.power.borrow(), PowerProfile::Performance);
        assert_eq!(b.ws.borrow()[0].1, "B-1");
        assert_eq!(c.active_mode().as_deref(), Some("gaming"));
    }
}
