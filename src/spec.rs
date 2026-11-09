//! Typed desired state for one monitor, and the only place that turns it
//! into compositor syntax. Every value is validated or formatted by us;
//! no user string is ever pasted into Lua or hyprlang.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::Monitor;

/// Connector names as Hyprland reports them (`DP-1`, `eDP-1`, `HDMI-A-1`).
pub fn valid_connector(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn check_connector(s: &str) -> Result<()> {
    if valid_connector(s) {
        Ok(())
    } else {
        Err(Error::new(
            "bad_connector",
            format!("invalid connector name `{}`", crate::model::sanitize(s)),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitorSpec {
    pub name: String,
    pub enabled: bool,
    pub width: u32,
    pub height: u32,
    pub refresh: f64,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: u8,
    /// 0 off, 1 on, 2 fullscreen only.
    pub vrr: u8,
    pub mirror: Option<String>,
}

impl MonitorSpec {
    pub fn from_monitor(m: &Monitor) -> Self {
        Self {
            name: m.name.clone(),
            enabled: m.enabled,
            width: m.width,
            height: m.height,
            refresh: m.refresh,
            x: m.x,
            y: m.y,
            scale: m.scale,
            transform: m.transform,
            vrr: u8::from(m.vrr),
            mirror: m.mirror_of.clone(),
        }
    }

    /// Size in layout coordinates (scaled, rotated).
    pub fn logical_size(&self) -> (i32, i32) {
        let (w, h) = if self.transform % 2 == 1 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        };
        let s = if self.scale > 0.0 { self.scale } else { 1.0 };
        (
            (f64::from(w) / s).round() as i32,
            (f64::from(h) / s).round() as i32,
        )
    }

    pub fn validate(&self) -> Result<()> {
        check_connector(&self.name)?;
        if let Some(m) = &self.mirror {
            check_connector(m)?;
            if *m == self.name {
                return Err(Error::new("bad_value", "a monitor cannot mirror itself"));
            }
        }
        if !self.enabled {
            return Ok(());
        }
        let bad = |what: &str| {
            Err(Error::new(
                "bad_value",
                format!("{} out of range for {}", what, self.name),
            ))
        };
        if !(1..=16384).contains(&self.width) || !(1..=16384).contains(&self.height) {
            return bad("resolution");
        }
        if !self.refresh.is_finite() || !(1.0..=1000.0).contains(&self.refresh) {
            return bad("refresh rate");
        }
        if !self.scale.is_finite() || !(0.25..=10.0).contains(&self.scale) {
            return bad("scale");
        }
        if self.transform > 7 {
            return bad("transform");
        }
        if self.vrr > 2 {
            return bad("vrr");
        }
        if self.x.unsigned_abs() > 100_000 || self.y.unsigned_abs() > 100_000 {
            return bad("position");
        }
        Ok(())
    }

    pub fn mode_string(&self) -> String {
        format!("{}x{}@{}", self.width, self.height, fmt_num(self.refresh))
    }

    /// Lua table for `hl.monitor`.
    pub fn to_lua(&self) -> Result<String> {
        self.validate()?;
        if !self.enabled {
            return Ok(format!(
                "hl.monitor({{ output = \"{}\", disabled = true }})",
                self.name
            ));
        }
        Ok(format!(
            "hl.monitor({{ output = \"{}\", disabled = false, mode = \"{}\", position = \"{}x{}\", scale = {}, transform = {}, vrr = {}, mirror = \"{}\" }})",
            self.name,
            self.mode_string(),
            self.x,
            self.y,
            fmt_num(self.scale),
            self.transform,
            self.vrr,
            self.mirror.as_deref().unwrap_or(""),
        ))
    }

    /// Value of a hyprlang `monitor =` line / `keyword monitor` argument.
    pub fn to_legacy(&self) -> Result<String> {
        self.validate()?;
        if !self.enabled {
            return Ok(format!("{},disable", self.name));
        }
        let mut s = format!(
            "{},{},{}x{},{},transform,{},vrr,{}",
            self.name,
            self.mode_string(),
            self.x,
            self.y,
            fmt_num(self.scale),
            self.transform,
            self.vrr
        );
        if let Some(m) = &self.mirror {
            s.push_str(",mirror,");
            s.push_str(m);
        }
        Ok(s)
    }
}

/// Formats a finite number without exponent and without trailing zeros
/// (`180`, `59.95`, `1.25`).
pub fn fmt_num(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> MonitorSpec {
        MonitorSpec {
            name: "DP-1".into(),
            enabled: true,
            width: 2560,
            height: 1440,
            refresh: 180.0,
            x: 1920,
            y: 0,
            scale: 1.25,
            transform: 0,
            vrr: 1,
            mirror: None,
        }
    }

    #[test]
    fn renders_lua() {
        assert_eq!(
            spec().to_lua().unwrap(),
            "hl.monitor({ output = \"DP-1\", disabled = false, mode = \"2560x1440@180\", position = \"1920x0\", scale = 1.25, transform = 0, vrr = 1, mirror = \"\" })"
        );
        let off = MonitorSpec {
            enabled: false,
            ..spec()
        };
        assert_eq!(
            off.to_lua().unwrap(),
            "hl.monitor({ output = \"DP-1\", disabled = true })"
        );
    }

    #[test]
    fn renders_legacy() {
        assert_eq!(
            spec().to_legacy().unwrap(),
            "DP-1,2560x1440@180,1920x0,1.25,transform,0,vrr,1"
        );
        let m = MonitorSpec {
            mirror: Some("eDP-1".into()),
            ..spec()
        };
        assert!(m.to_legacy().unwrap().ends_with(",mirror,eDP-1"));
    }

    #[test]
    fn rejects_injection() {
        for bad in ["DP-1\"})os.execute(\"x", "DP 1", "", "a,b", "DP-1\n"] {
            assert!(
                MonitorSpec {
                    name: bad.into(),
                    ..spec()
                }
                .to_lua()
                .is_err(),
                "{bad:?}"
            );
            assert!(
                MonitorSpec {
                    mirror: Some(bad.into()),
                    ..spec()
                }
                .to_lua()
                .is_err(),
                "{bad:?}"
            );
        }
        assert!(
            MonitorSpec {
                scale: f64::NAN,
                ..spec()
            }
            .to_lua()
            .is_err()
        );
        assert!(
            MonitorSpec {
                refresh: f64::INFINITY,
                ..spec()
            }
            .to_lua()
            .is_err()
        );
        assert!(
            MonitorSpec {
                transform: 8,
                ..spec()
            }
            .to_lua()
            .is_err()
        );
    }

    #[test]
    fn numbers() {
        assert_eq!(fmt_num(180.0), "180");
        assert_eq!(fmt_num(59.95), "59.95");
        assert_eq!(fmt_num(1.0 / 3.0), "0.333333");
        assert_eq!(fmt_num(-0.0), "0");
    }

    #[test]
    fn logical() {
        assert_eq!(spec().logical_size(), (2048, 1152));
        assert_eq!(
            MonitorSpec {
                transform: 1,
                scale: 1.0,
                ..spec()
            }
            .logical_size(),
            (1440, 2560)
        );
    }
}
