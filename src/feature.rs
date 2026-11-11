//! Optional features that people can turn off (docs/themes.md#features).
//! `~/.config/stance/features.json` remembers the choice (missing = on);
//! turning a feature off also stops its user units, never anything else.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::store;

pub struct Feature {
    pub name: &'static str,
    pub units: &'static [&'static str],
}

pub const FEATURES: &[Feature] = &[
    Feature {
        name: "clip",
        units: &["stance-clip-text.service", "stance-clip-image.service"],
    },
    Feature {
        name: "deck",
        units: &["stance-deck.service"],
    },
    Feature {
        name: "rules",
        units: &["stance.service"],
    },
    Feature {
        name: "quick",
        units: &[],
    },
];

#[derive(Debug, Serialize)]
pub struct State {
    pub name: &'static str,
    pub on: bool,
    /// First unit of the feature if it is installed, else null.
    pub unit: Option<&'static str>,
    pub unit_active: Option<bool>,
}

pub fn find(name: &str) -> Result<&'static Feature> {
    FEATURES.iter().find(|f| f.name == name).ok_or_else(|| {
        Error::new(
            "usage",
            format!("unknown feature `{name}` (clip, deck, rules, quick)"),
        )
    })
}

fn path() -> Result<PathBuf> {
    Ok(store::config_dir()?.join("features.json"))
}

pub fn load_at(p: &Path) -> Result<BTreeMap<String, bool>> {
    if !p.exists() {
        return Ok(BTreeMap::new());
    }
    let mut m: BTreeMap<String, bool> = serde_json::from_str(&store::read_checked(p)?)
        .map_err(|e| Error::new("invalid", format!("features.json: {e}")))?;
    m.retain(|k, _| FEATURES.iter().any(|f| f.name == k));
    Ok(m)
}

pub fn is_on(name: &str) -> bool {
    path()
        .and_then(|p| load_at(&p))
        .map(|m| m.get(name).copied().unwrap_or(true))
        .unwrap_or(true)
}

fn systemctl() -> Option<PathBuf> {
    crate::tool::find("systemctl")
}

/// True if systemd knows the user unit (installed by a package or by hand).
fn unit_installed(sc: &Path, unit: &str) -> bool {
    Command::new(sc)
        .args(["--user", "cat", unit])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn unit_active(sc: &Path, unit: &str) -> bool {
    Command::new(sc)
        .args(["--user", "is-active", "--quiet", unit])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub fn list() -> Result<Vec<State>> {
    let m = load_at(&path()?)?;
    let sc = systemctl();
    Ok(FEATURES
        .iter()
        .map(|f| {
            let unit = f
                .units
                .first()
                .copied()
                .filter(|u| sc.as_deref().is_some_and(|sc| unit_installed(sc, u)));
            State {
                name: f.name,
                on: m.get(f.name).copied().unwrap_or(true),
                unit,
                unit_active: unit.and_then(|u| sc.as_deref().map(|sc| unit_active(sc, u))),
            }
        })
        .collect())
}

/// Turns a feature on or off: remembers it, then starts/stops its units.
/// Returns notes for units that could not be changed (not fatal: the
/// choice is saved either way).
pub fn set(name: &str, on: bool, clip_state_dir: &Path) -> Result<Vec<String>> {
    let f = find(name)?;
    let p = path()?;
    let mut m = load_at(&p)?;
    m.insert(name.to_string(), on);
    let parent = p
        .parent()
        .ok_or_else(|| Error::new("io", "config path has no parent"))?;
    store::ensure_private_dir(parent)?;
    let body = serde_json::to_string_pretty(&m).unwrap_or_default() + "\n";
    store::atomic_write(&p, body.as_bytes(), false)?;

    if f.name == "clip" {
        let c = crate::clip::Clip::open(clip_state_dir)?;
        c.put_config(&format!(r#"{{"enabled":{on}}}"#))?;
    }
    let mut notes = Vec::new();
    let Some(sc) = systemctl() else {
        if !f.units.is_empty() {
            notes.push("systemctl not found; units not changed".into());
        }
        return Ok(notes);
    };
    for u in f.units {
        if !unit_installed(&sc, u) {
            continue;
        }
        let verb = if on { "enable" } else { "disable" };
        if let Err(e) =
            crate::tool::run_tool(&sc, &["--user", verb, "--now", u], Duration::from_secs(20))
        {
            notes.push(format!("{u}: {e}"));
        }
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_file() {
        assert!(find("clip").is_ok());
        assert!(find("../x").is_err());
        let d = std::env::temp_dir().join(format!("stance-feat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        store::ensure_private_dir(&d).unwrap();
        let p = d.join("features.json");
        assert!(load_at(&p).unwrap().is_empty());
        std::fs::write(&p, r#"{"clip":false,"bogus":true}"#).unwrap();
        let m = load_at(&p).unwrap();
        assert_eq!(m.get("clip"), Some(&false));
        assert!(!m.contains_key("bogus"));
        std::fs::remove_dir_all(&d).ok();
    }
}
