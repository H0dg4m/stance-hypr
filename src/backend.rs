//! Compositor backends. v1 ships Hyprland only; other desktops implement
//! the same trait later without touching the interfaces.

use std::cell::OnceCell;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::hypr::Instance;
use crate::model::{Mode, Monitor, Parser, sanitize};

pub trait Backend {
    fn version(&self) -> Result<String>;
    fn parser(&self) -> Result<Parser>;
    fn monitors(&self) -> Result<Vec<Monitor>>;
}

pub struct Hyprland {
    pub instance: Instance,
    parser: OnceCell<Parser>,
}

impl Hyprland {
    pub fn new(instance: Instance) -> Self {
        Self {
            instance,
            parser: OnceCell::new(),
        }
    }

    pub fn connect() -> Result<Self> {
        Ok(Self::new(Instance::from_env()?))
    }
}

impl Backend for Hyprland {
    fn version(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct V {
            version: String,
        }
        Ok(self.instance.request_json::<V>("version")?.version)
    }

    /// Lazily detected and cached per process. `eval return 1` has no
    /// side effects: Lua answers `ok`, hyprlang rejects the request.
    fn parser(&self) -> Result<Parser> {
        if let Some(p) = self.parser.get() {
            return Ok(*p);
        }
        let reply = self.instance.request("eval return 1")?;
        let p = if reply.trim() == "ok" {
            Parser::Lua
        } else {
            Parser::Legacy
        };
        Ok(*self.parser.get_or_init(|| p))
    }

    fn monitors(&self) -> Result<Vec<Monitor>> {
        let raw: Vec<RawMonitor> = self.instance.request_json("monitors all")?;
        Ok(raw.into_iter().map(Monitor::from).collect())
    }
}

/// Subset of `monitors -j` we rely on. Unknown fields are ignored on
/// purpose: Hyprland adds fields between releases.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawMonitor {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    make: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    serial: String,
    width: u32,
    height: u32,
    refresh_rate: f64,
    x: i32,
    y: i32,
    scale: f64,
    transform: u8,
    #[serde(default)]
    focused: bool,
    #[serde(default)]
    dpms_status: bool,
    #[serde(default)]
    vrr: bool,
    #[serde(default)]
    disabled: bool,
    #[serde(default)]
    mirror_of: Option<String>,
    #[serde(default)]
    available_modes: Vec<String>,
    #[serde(default)]
    hardware_details: Option<HardwareDetails>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HardwareDetails {
    #[serde(default)]
    vrr_capable: bool,
}

impl From<RawMonitor> for Monitor {
    fn from(r: RawMonitor) -> Self {
        Monitor {
            name: r.name,
            description: sanitize(&r.description),
            make: sanitize(&r.make),
            model: sanitize(&r.model),
            serial: sanitize(&r.serial),
            enabled: !r.disabled,
            focused: r.focused,
            dpms: r.dpms_status,
            width: r.width,
            height: r.height,
            refresh: (r.refresh_rate * 100.0).round() / 100.0,
            x: r.x,
            y: r.y,
            scale: r.scale,
            transform: r.transform,
            vrr: r.vrr,
            vrr_capable: r.hardware_details.is_some_and(|h| h.vrr_capable),
            mirror_of: r.mirror_of.filter(|m| !m.is_empty() && m != "none"),
            modes: r
                .available_modes
                .iter()
                .filter_map(|m| Mode::parse(m))
                .collect(),
        }
    }
}

/// Picks the backend for the current session.
pub fn detect() -> Result<Box<dyn Backend>> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        return Ok(Box::new(Hyprland::connect()?));
    }
    Err(Error::new(
        "no_backend",
        "no supported compositor detected (only Hyprland is supported for now)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_0_57_reply() {
        let raw: Vec<RawMonitor> =
            serde_json::from_str(include_str!("../tests/fixtures/monitors-0.57.1.json")).unwrap();
        let mons: Vec<Monitor> = raw.into_iter().map(Monitor::from).collect();
        assert_eq!(mons.len(), 2);
        let dp = &mons[0];
        assert_eq!(dp.name, "DP-1");
        assert_eq!(dp.description, "JDR EPIC Q40");
        assert_eq!(dp.serial, "");
        assert_eq!((dp.width, dp.height, dp.refresh), (2560, 1440, 180.0));
        assert!(dp.enabled && dp.vrr_capable && !dp.vrr);
        assert_eq!(dp.mirror_of, None);
        assert!(dp.modes.iter().any(|m| m.refresh == 165.0));
        assert_eq!(mons[1].name, "eDP-1");
    }
}
