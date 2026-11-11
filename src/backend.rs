//! Compositor backends. v1 ships Hyprland only; other desktops implement
//! the same trait later without touching the interfaces.

pub mod fake;
pub mod hyprland;

use std::path::Path;
use std::sync::Arc;

pub use hyprland::Hyprland;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::{Monitor, Parser};
use crate::spec::MonitorSpec;

/// Whitelisted compositor options a mode may change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptKey {
    Animations,
    Blur,
    GapsIn,
    GapsOut,
    #[serde(rename = "allow_tearing", alias = "tearing")]
    Tearing,
}

impl OptKey {
    /// Neutral name (also the serialized one).
    pub fn name(self) -> &'static str {
        match self {
            OptKey::Animations => "animations",
            OptKey::Blur => "blur",
            OptKey::GapsIn => "gaps_in",
            OptKey::GapsOut => "gaps_out",
            OptKey::Tearing => "allow_tearing",
        }
    }

    pub fn is_gap(self) -> bool {
        matches!(self, OptKey::GapsIn | OptKey::GapsOut)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OptValue {
    Bool(bool),
    /// CSS order: top, right, bottom, left.
    Gaps([u16; 4]),
}

impl OptValue {
    pub fn check(&self, key: OptKey) -> Result<()> {
        match (self, key.is_gap()) {
            (OptValue::Bool(_), false) => Ok(()),
            (OptValue::Gaps(g), true) if g.iter().all(|v| *v <= 500) => Ok(()),
            _ => Err(Error::new(
                "bad_value",
                format!("invalid value for {}", key.name()),
            )),
        }
    }
}

/// Backend-neutral things that happened on the desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// `name` is empty when the backend did not give a usable connector.
    MonitorAdded {
        name: String,
    },
    MonitorRemoved {
        name: String,
    },
    /// The compositor reloaded its config (runtime changes are gone).
    ConfigReloaded,
}

/// What one wait on an [`EventSource`] returned.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    Event(Event),
    /// The timeout passed, or something uninteresting happened: look at
    /// your timers.
    Tick,
    /// The compositor went away.
    Closed,
}

/// A blocking stream of [`Event`]s. `next` waits at most `timeout`
/// (`None` = until something happens).
pub trait EventSource {
    fn next(&mut self, timeout: Option<std::time::Duration>) -> Result<Next>;
}

#[derive(Debug, Clone, Copy)]
pub enum Notice {
    Reverted,
    RevertFailed,
    AutoPaused,
}

impl Notice {
    fn text(self) -> &'static str {
        match self {
            Notice::Reverted => "stance: display change not confirmed, reverted",
            Notice::RevertFailed => "stance: revert did not fully apply, check your displays",
            Notice::AutoPaused => {
                "stance: too many automatic changes, hotplug rules paused for 60 s"
            }
        }
    }
}

/// What a backend can do; commands needing a missing capability fail with
/// code `unsupported`, the panel hides what is not there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Caps {
    pub displays: bool,
    pub mirror: bool,
    pub vrr: bool,
    pub transform: bool,
    pub workspaces: bool,
    pub compositor_options: bool,
    pub persist: bool,
    pub events: bool,
}

impl Default for Caps {
    /// A plain multi-screen desktop: everything but compositor-specific
    /// extras.
    fn default() -> Self {
        Self {
            displays: true,
            mirror: true,
            vrr: true,
            transform: true,
            workspaces: true,
            compositor_options: true,
            persist: false,
            events: false,
        }
    }
}

impl Caps {
    pub fn require(ok: bool, backend: &str, what: &str) -> Result<()> {
        if ok {
            Ok(())
        } else {
            Err(Error::new(
                "unsupported",
                format!("{what} is not supported on the {backend} backend"),
            ))
        }
    }
}

pub trait Backend {
    /// Backend name as `--backend` spells it.
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> Caps;
    /// How this backend calls `key` (shown in results); Hyprland: its
    /// hyprlang path.
    fn option_name(&self, key: OptKey) -> String {
        key.name().to_string()
    }
    /// Identifies this compositor session; pending state is bound to it.
    fn session_id(&self) -> &str;
    /// Night light of this session (hyprsunset on Hyprland); `None` where
    /// there is none.
    fn night_link(&self) -> Option<Arc<dyn crate::night::Link>> {
        None
    }
    /// Neutral events (needs `caps.events`).
    fn events(&self) -> Result<Box<dyn EventSource>> {
        Err(Error::new(
            "unsupported",
            format!("events are not available on the {} backend", self.name()),
        ))
    }
    /// Refuses (code `untested_version`) when this compositor is newer than
    /// any version stance was tested with. Called before display changes.
    fn compat_check(&self) -> Result<()> {
        Ok(())
    }
    fn version(&self) -> Result<String>;
    fn parser(&self) -> Result<Parser>;
    fn monitors(&self) -> Result<Vec<Monitor>>;
    fn apply_monitor(&self, spec: &MonitorSpec) -> Result<()>;
    fn get_option(&self, key: OptKey) -> Result<OptValue>;
    fn set_option(&self, key: OptKey, value: OptValue) -> Result<()>;
    /// (workspace id, monitor connector) for existing numbered workspaces.
    fn workspaces(&self) -> Result<Vec<(u32, String)>>;
    fn bind_workspace(&self, id: u32, monitor: &str) -> Result<()>;
    fn notify(&self, notice: Notice);

    fn power_get(&self) -> Result<crate::power::PowerProfile> {
        crate::power::get()
    }

    fn power_set(&self, p: crate::power::PowerProfile) -> Result<()> {
        crate::power::set(p)
    }
}

/// Picks the backend: `--backend NAME`, else `STANCE_BACKEND`, else the
/// running session (Hyprland today).
pub fn connect(name: Option<&str>, state_dir: &Path) -> Result<Box<dyn Backend>> {
    let env = std::env::var("STANCE_BACKEND")
        .ok()
        .filter(|v| !v.is_empty());
    match name.or(env.as_deref()) {
        None | Some("hyprland") => Ok(Box::new(Hyprland::connect()?)),
        Some("fake") => {
            let path = match std::env::var_os("STANCE_FAKE_STATE").map(std::path::PathBuf::from) {
                Some(p) if p.is_absolute() => p,
                _ => state_dir.join("fake.json"),
            };
            Ok(Box::new(fake::Fake::open(path)?))
        }
        Some(other) => Err(Error::new(
            "usage",
            format!("unknown backend `{other}` (hyprland, fake)"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_values() {
        assert!(OptValue::Bool(true).check(OptKey::GapsIn).is_err());
        assert!(OptValue::Gaps([9999; 4]).check(OptKey::GapsIn).is_err());
        assert!(OptValue::Gaps([1; 4]).check(OptKey::GapsIn).is_ok());
        let k: OptKey = serde_json::from_str("\"allow_tearing\"").unwrap();
        assert_eq!(serde_json::to_string(&k).unwrap(), "\"allow_tearing\"");
        assert_eq!(serde_json::from_str::<OptKey>("\"tearing\"").unwrap(), k);
    }
}
