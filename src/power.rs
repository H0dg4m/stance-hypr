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
