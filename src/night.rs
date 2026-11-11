//! Night light through hyprsunset's IPC socket (`.hyprsunset.sock` in the
//! Hyprland instance dir, same request/reply framing as socket1).
//!
//! "Off" means the neutral temperature, never `identity`: identity would also
//! reset a gamma the user set on their own hyprsunset. A second hyprsunset is
//! only started when none answers (two would fight over the colour matrix).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::backend::Backend;
use crate::error::{Error, Result};
use crate::model::sanitize;
use crate::store;

/// hyprsunset's own default; treated as "night light off".
pub const NEUTRAL: u32 = 6000;
pub const DEFAULT_WARM: u32 = 4500;
pub const MIN_K: u32 = 1000;
pub const MAX_K: u32 = 20000;
const START_WAIT: Duration = Duration::from_millis(2500);

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// hyprsunset is installed (or already answering).
    pub available: bool,
    /// A hyprsunset answers on this Hyprland instance.
    pub running: bool,
    pub on: bool,
    pub temperature: Option<u32>,
    pub gamma: Option<u32>,
    /// Temperature `stance night on` / `toggle` uses (last one set).
    pub warm: u32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    warm: u32,
}

pub fn check_temperature(k: u32) -> Result<()> {
    if (MIN_K..=MAX_K).contains(&k) {
        Ok(())
    } else {
        Err(Error::new(
            "bad_value",
            format!("temperature must be {MIN_K}-{MAX_K} K"),
        ))
    }
}

/// Parses a reply that should be a plain number (`6000`).
pub fn parse_number(reply: &str) -> Option<u32> {
    let t = reply.trim();
    (!t.is_empty() && t.len() <= 6 && t.bytes().all(|b| b.is_ascii_digit()))
        .then(|| t.parse().ok())
        .flatten()
}

fn expect_ok(reply: &str) -> Result<()> {
    if reply.trim() == "ok" {
        Ok(())
    } else {
        Err(Error::new(
            "night",
            format!("hyprsunset: {}", sanitize(reply.trim())),
        ))
    }
}

/// How to reach the night-light daemon of one desktop session. `Send` so
/// the compositor daemon can use it from a worker thread.
pub trait Link: Send + Sync {
    /// One request/reply exchange (hyprsunset's framing).
    fn ask(&self, req: &str) -> Result<String>;
    /// Environment a freshly started hyprsunset needs to find the session.
    fn env(&self) -> Vec<(&'static str, String)>;
}

#[derive(Clone)]
pub struct Night {
    link: Arc<dyn Link>,
    state_dir: PathBuf,
}

impl Night {
    pub fn new(link: Arc<dyn Link>, state_dir: &Path) -> Self {
        Self {
            link,
            state_dir: state_dir.to_path_buf(),
        }
    }

    /// Night light of `backend`'s session, or `unsupported`.
    pub fn open(backend: &dyn Backend, state_dir: &Path) -> Result<Self> {
        backend
            .night_link()
            .map(|l| Self::new(l, state_dir))
            .ok_or_else(|| {
                Error::new(
                    "unsupported",
                    format!(
                        "night light is not available on the {} backend",
                        backend.name()
                    ),
                )
            })
    }

    fn ask(&self, req: &str) -> Result<String> {
        self.link.ask(req)
    }

    fn saved_path(&self) -> PathBuf {
        self.state_dir.join("night.json")
    }

    fn warm(&self) -> u32 {
        store::read_checked(&self.saved_path())
            .ok()
            .and_then(|t| serde_json::from_str::<Saved>(&t).ok())
            .map(|s| s.warm)
            .filter(|k| check_temperature(*k).is_ok() && *k != NEUTRAL)
            .unwrap_or(DEFAULT_WARM)
    }

    fn remember(&self, k: u32) {
        if k == NEUTRAL || store::ensure_private_dir(&self.state_dir).is_err() {
            return;
        }
        if let Ok(data) = serde_json::to_vec(&Saved { warm: k }) {
            let _ = store::atomic_write(&self.saved_path(), &data, false);
        }
    }

    /// The leftover socket file of an exited hyprsunset stays behind, so
    /// "running" means "answers", not "socket exists".
    fn temperature(&self) -> Option<u32> {
        self.ask("temperature").ok().and_then(|r| parse_number(&r))
    }

    pub fn status(&self) -> Status {
        let temperature = self.temperature();
        let gamma = temperature
            .and_then(|_| self.ask("gamma").ok())
            .and_then(|r| parse_number(&r));
        Status {
            available: temperature.is_some() || crate::tool::find("hyprsunset").is_some(),
            running: temperature.is_some(),
            on: temperature.is_some_and(|k| k != NEUTRAL),
            temperature,
            gamma,
            warm: self.warm(),
        }
    }

    /// Sets the colour temperature, starting hyprsunset if none answers.
    pub fn set(&self, k: u32) -> Result<Status> {
        check_temperature(k)?;
        // Serialises concurrent clicks: without it two callers can both see
        // "nobody answers" and each spawn a hyprsunset. Dropped on return;
        // the fd is close-on-exec, so the spawned hyprsunset never holds it.
        let _lock = self.lock();
        if self.temperature().is_some() {
            expect_ok(&self.ask(&format!("temperature {k}"))?)?;
        } else if k == NEUTRAL {
            // Nothing running means nothing tinted: already off.
            return Ok(self.status());
        } else {
            self.start(k)?;
        }
        self.remember(k);
        let st = self.status();
        if st.temperature != Some(k) {
            return Err(Error::new(
                "not_applied",
                "hyprsunset did not take the temperature",
            ));
        }
        Ok(st)
    }

    pub fn on(&self, k: Option<u32>) -> Result<Status> {
        self.set(k.unwrap_or_else(|| self.warm()))
    }

    pub fn off(&self) -> Result<Status> {
        self.set(NEUTRAL)
    }

    pub fn toggle(&self) -> Result<Status> {
        if self.status().on {
            self.off()
        } else {
            self.on(None)
        }
    }

    fn lock(&self) -> Option<std::fs::File> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        store::ensure_private_dir(&self.state_dir).ok()?;
        let f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.state_dir.join("night.lock"))
            .ok()?;
        // Blocking is bounded by the holder's START_WAIT.
        // SAFETY: valid fd owned by `f`.
        (unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } == 0).then_some(f)
    }

    fn start(&self, k: u32) -> Result<()> {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};
        let tool = crate::tool::find("hyprsunset")
            .ok_or_else(|| Error::new("night", "hyprsunset not found (install hyprsunset)"))?;
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return Err(Error::new(
                "night",
                "WAYLAND_DISPLAY is not set; cannot start hyprsunset",
            ));
        }
        let mut cmd = Command::new(tool);
        cmd.arg("-t").arg(k.to_string());
        for (key, val) in self.link.env() {
            cmd.env(key, val);
        }
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // Outlives the bar click that started it.
            .process_group(0)
            .spawn()
            .map_err(|e| Error::new("night", format!("cannot start hyprsunset: {e}")))?;
        // Reap it whenever it exits (matters in the long-lived daemon).
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        let t0 = Instant::now();
        while t0.elapsed() < START_WAIT {
            std::thread::sleep(Duration::from_millis(100));
            if self.temperature() == Some(k) {
                return Ok(());
            }
        }
        Err(Error::new(
            "night",
            "hyprsunset started but does not answer",
        ))
    }
}

/// `night_light` in a mode file: `false` (off), `true` (on, last warmth)
/// or a temperature in K.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NightSetting {
    Switch(bool),
    Kelvin(u32),
}

impl NightSetting {
    pub fn check(self) -> Result<()> {
        match self {
            NightSetting::Kelvin(k) => check_temperature(k),
            NightSetting::Switch(_) => Ok(()),
        }
    }

    pub fn apply(self, n: &Night) -> Result<Status> {
        match self {
            NightSetting::Switch(true) => n.on(None),
            NightSetting::Switch(false) => n.off(),
            NightSetting::Kelvin(k) => n.set(k),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_ranges() {
        assert_eq!(parse_number("6000"), Some(6000));
        assert_eq!(parse_number(" 90\n"), Some(90));
        assert_eq!(parse_number("invalid command"), None);
        assert_eq!(parse_number(""), None);
        assert_eq!(parse_number("-1"), None);
        assert_eq!(parse_number("99999999"), None);
        assert!(check_temperature(999).is_err());
        assert!(check_temperature(20001).is_err());
        assert!(check_temperature(4500).is_ok());
        assert!(expect_ok("ok").is_ok());
        assert!(expect_ok("Invalid temperature").is_err());
    }

    #[test]
    fn mode_setting_parses() {
        #[derive(Deserialize)]
        struct T {
            n: NightSetting,
        }
        let p = |s: &str| toml::from_str::<T>(s).map(|t| t.n);
        assert_eq!(p("n = false").unwrap(), NightSetting::Switch(false));
        assert_eq!(p("n = 4000").unwrap(), NightSetting::Kelvin(4000));
        assert!(p("n = \"warm\"").is_err());
        assert!(p("n = -5").is_err());
        assert!(NightSetting::Kelvin(50).check().is_err());
    }
}
