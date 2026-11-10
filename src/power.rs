//! Power profile via `powerprofilesctl` (argv only, never a shell).

use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PowerProfile {
    PowerSaver,
    Balanced,
    Performance,
}

impl PowerProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            PowerProfile::PowerSaver => "power-saver",
            PowerProfile::Balanced => "balanced",
            PowerProfile::Performance => "performance",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "power-saver" => Some(PowerProfile::PowerSaver),
            "balanced" => Some(PowerProfile::Balanced),
            "performance" => Some(PowerProfile::Performance),
            _ => None,
        }
    }
}

fn tool() -> Option<PathBuf> {
    crate::tool::find("powerprofilesctl")
}

pub fn available() -> bool {
    tool().is_some()
}

fn run(args: &[&str]) -> Result<String> {
    let tool = tool().ok_or_else(|| {
        Error::new(
            "power",
            "powerprofilesctl not found (install power-profiles-daemon)",
        )
    })?;
    let out = Command::new(tool)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| Error::new("power", format!("powerprofilesctl: {e}")))?;
    if !out.status.success() {
        return Err(Error::new(
            "power",
            format!(
                "powerprofilesctl failed: {}",
                crate::model::sanitize(String::from_utf8_lossy(&out.stderr).trim())
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn get() -> Result<PowerProfile> {
    let out = run(&["get"])?;
    PowerProfile::parse(&out)
        .ok_or_else(|| Error::new("power", "unexpected powerprofilesctl output"))
}

pub fn set(p: PowerProfile) -> Result<()> {
    run(&["set", p.as_str()]).map(|_| ())
}

/// Is the machine on mains power? `None` when there is no mains supply to
/// ask (a desktop): power rules then never apply.
pub fn on_ac() -> Option<bool> {
    on_ac_in(std::path::Path::new("/sys/class/power_supply"))
}

pub fn on_ac_in(dir: &std::path::Path) -> Option<bool> {
    let mut seen = false;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        let kind = std::fs::read_to_string(p.join("type")).unwrap_or_default();
        if kind.trim() != "Mains" {
            continue;
        }
        seen = true;
        if std::fs::read_to_string(p.join("online")).is_ok_and(|v| v.trim() == "1") {
            return Some(true);
        }
    }
    seen.then_some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mains_detection() {
        let d = std::env::temp_dir().join(format!("stance-ps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("BAT1")).unwrap();
        std::fs::write(d.join("BAT1/type"), "Battery\n").unwrap();
        assert_eq!(on_ac_in(&d), None);
        std::fs::create_dir_all(d.join("ACAD")).unwrap();
        std::fs::write(d.join("ACAD/type"), "Mains\n").unwrap();
        std::fs::write(d.join("ACAD/online"), "0\n").unwrap();
        assert_eq!(on_ac_in(&d), Some(false));
        std::fs::write(d.join("ACAD/online"), "1\n").unwrap();
        assert_eq!(on_ac_in(&d), Some(true));
        let _ = std::fs::remove_dir_all(&d);
    }
}
