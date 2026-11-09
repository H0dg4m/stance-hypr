//! Backend-neutral data types. Interfaces only ever see these, never raw
//! compositor JSON.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Parser {
    /// Lua config (Hyprland 0.55+): runtime changes go through `eval`.
    Lua,
    /// hyprlang `.conf`: runtime changes go through `keyword`.
    Legacy,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    /// Hz, as reported by the compositor (e.g. 59.95).
    pub refresh: f64,
}

impl Mode {
    /// Parses `2560x1440@180.00Hz` (the `Hz` suffix is optional).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.strip_suffix("Hz").unwrap_or(s);
        let (res, hz) = s.split_once('@')?;
        let (w, h) = res.split_once('x')?;
        let refresh: f64 = hz.parse().ok()?;
        if !refresh.is_finite() || refresh <= 0.0 {
            return None;
        }
        Some(Self {
            width: w.parse().ok()?,
            height: h.parse().ok()?,
            refresh,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Monitor {
    /// Connector, e.g. `DP-1`. The only identifier ever sent back to the compositor.
    pub name: String,
    /// Human description (EDID derived). Display only; never interpolated into config.
    pub description: String,
    pub make: String,
    pub model: String,
    pub serial: String,
    pub enabled: bool,
    pub focused: bool,
    pub dpms: bool,
    pub width: u32,
    pub height: u32,
    pub refresh: f64,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: u8,
    pub vrr: bool,
    pub vrr_capable: bool,
    /// Connector this monitor mirrors, if any.
    pub mirror_of: Option<String>,
    pub modes: Vec<Mode>,
}

/// Replaces control characters so EDID strings are safe to print or log.
pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .take(128)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_parse() {
        assert_eq!(
            Mode::parse("2560x1440@180.00Hz"),
            Some(Mode {
                width: 2560,
                height: 1440,
                refresh: 180.0
            })
        );
        assert_eq!(
            Mode::parse("1920x1080@59.94").map(|m| m.refresh),
            Some(59.94)
        );
        assert_eq!(Mode::parse("1920x1080"), None);
        assert_eq!(Mode::parse("axb@60"), None);
        assert_eq!(Mode::parse("1x1@NaN"), None);
        assert_eq!(Mode::parse("1x1@-5"), None);
    }

    #[test]
    fn sanitize_strips_control() {
        assert_eq!(sanitize("JDR\x1b[31m EPIC\n"), "JDR?[31m EPIC?");
    }
}
