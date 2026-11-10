//! Brightness: internal panels via /sys/class/backlight (+ brightnessctl to
//! write), external monitors via ddcutil (VCP 0x10). Connector -> i2c bus
//! comes from ddcutil's own `DRM connector:` line, cached under
//! $XDG_RUNTIME_DIR/stance/ddc.json.
//!
//! Clamping: requested 0..=100 is accepted, applied as at least 5% for
//! backlights and 1 for DDC. `SetResult.warnings` says when that happened.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::tool;

const DETECT_TIMEOUT: Duration = Duration::from_secs(10);
const VCP_TIMEOUT: Duration = Duration::from_secs(4);
const BL_TIMEOUT: Duration = Duration::from_secs(3);
const BACKLIGHT_DIR: &str = "/sys/class/backlight";
const MIN_BACKLIGHT_PCT: u32 = 5;
const MIN_DDC: u32 = 1;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Display {
    pub monitor: String,
    /// "backlight" or "ddc"
    pub kind: &'static str,
    pub value: u32,
    pub max: u32,
    pub percent: u8,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SetResult {
    pub display: Display,
    /// Non-empty when the request was clamped.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Abs(u8),
    Up(u8),
    Down(u8),
}

pub fn available() -> bool {
    tool::find("ddcutil").is_some() || tool::find("brightnessctl").is_some()
}

pub fn parse_change(s: &str) -> Result<Change> {
    let bad = || {
        Error::new(
            "brightness",
            "brightness must be N, +N or -N with N in 0..100",
        )
    };
    let s = s.trim();
    let (rest, mk): (&str, fn(u8) -> Change) = match s.as_bytes().first() {
        Some(b'+') => (&s[1..], Change::Up),
        Some(b'-') => (&s[1..], Change::Down),
        _ => (s, Change::Abs),
    };
    if rest.is_empty() || rest.len() > 3 || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let n: u8 = rest.parse().map_err(|_| bad())?;
    if n > 100 {
        return Err(bad());
    }
    Ok(mk(n))
}

fn valid_connector(m: &str) -> bool {
    !m.is_empty()
        && m.len() <= 32
        && m.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

fn valid_device(d: &str) -> bool {
    !d.is_empty()
        && d.len() <= 64
        && d.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
}

pub fn is_internal(connector: &str) -> bool {
    ["eDP", "LVDS", "DSI"]
        .iter()
        .any(|p| connector.starts_with(p))
}

fn percent(value: u32, max: u32) -> u8 {
    if max == 0 {
        return 0;
    }
    ((u64::from(value) * 100 + u64::from(max) / 2) / u64::from(max)).min(100) as u8
}

// ---------- pure parsers ----------

/// `ddcutil detect --brief` -> (connector without `cardN-`, i2c bus).
/// "Invalid display" blocks are skipped.
pub fn parse_detect(text: &str) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    let mut valid = false;
    let mut bus: Option<u32> = None;
    let mut conn: Option<String> = None;
    let mut flush = |valid: bool, bus: &mut Option<u32>, conn: &mut Option<String>| {
        if let (true, Some(b), Some(c)) = (valid, bus.take(), conn.take()) {
            out.push((c, b));
        }
        *bus = None;
        *conn = None;
    };
    for line in text.lines() {
        let t = line.trim();
        if !line.starts_with(' ') && !line.starts_with('\t') && !t.is_empty() {
            flush(valid, &mut bus, &mut conn);
            valid = t.starts_with("Display ");
        } else if let Some(v) = t.strip_prefix("I2C bus:") {
            bus = v
                .trim()
                .strip_prefix("/dev/i2c-")
                .and_then(|n| n.parse().ok());
        } else if let Some(v) = t
            .strip_prefix("DRM connector:")
            .or_else(|| t.strip_prefix("DRM_connector:"))
        {
            let v = v.trim();
            let name = match v.strip_prefix("card") {
                Some(r) => {
                    let digits = r.bytes().take_while(u8::is_ascii_digit).count();
                    r[digits..].strip_prefix('-').unwrap_or(v)
                }
                None => v,
            };
            if valid_connector(name) {
                conn = Some(name.to_owned());
            }
        }
    }
    flush(valid, &mut bus, &mut conn);
    out
}

/// `ddcutil getvcp 10 --brief` -> (current, max). Accepts `VCP 10 C 1 100`.
pub fn parse_getvcp(text: &str) -> Result<(u32, u32)> {
    let bad = || Error::new("brightness", "unexpected ddcutil getvcp output");
    let f: Vec<&str> = text.split_whitespace().collect();
    if f.len() != 5 || f[0] != "VCP" || !f[1].eq_ignore_ascii_case("10") || f[2] != "C" {
        return Err(bad());
    }
    let cur: u32 = f[3].parse().map_err(|_| bad())?;
    let max: u32 = f[4].parse().map_err(|_| bad())?;
    if max == 0 || cur > max {
        return Err(bad());
    }
    Ok((cur, max))
}

// ---------- backlight ----------

struct Backlight {
    dev: String,
    value: u32,
    max: u32,
}

fn type_rank(t: &str) -> u8 {
    match t.trim() {
        "raw" => 0,
        "platform" => 1,
        "firmware" => 2,
        _ => 3,
    }
}

fn read_u32(p: &Path) -> Option<u32> {
    std::fs::read_to_string(p).ok()?.trim().parse().ok()
}

fn backlight_in(dir: &Path) -> Option<Backlight> {
    let mut best: Option<(u8, Backlight)> = None;
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    names.sort();
    for dev in names {
        if !valid_device(&dev) {
            continue;
        }
        let p = dir.join(&dev);
        let (Some(value), Some(max)) = (
            read_u32(&p.join("brightness")),
            read_u32(&p.join("max_brightness")),
        ) else {
            continue;
        };
        if max == 0 {
            continue;
        }
        let rank = type_rank(&std::fs::read_to_string(p.join("type")).unwrap_or_default());
        if best.as_ref().is_none_or(|(r, _)| rank < *r) {
            best = Some((rank, Backlight { dev, value, max }));
        }
    }
    best.map(|(_, b)| b)
}

fn backlight() -> Option<Backlight> {
    backlight_in(Path::new(BACKLIGHT_DIR))
}

fn bl_display(monitor: &str, b: &Backlight) -> Display {
    Display {
        monitor: monitor.into(),
        kind: "backlight",
        value: b.value,
        max: b.max,
        percent: percent(b.value, b.max),
    }
}

// ---------- ddc ----------

fn cache_path() -> Result<PathBuf> {
    Ok(crate::hypr::runtime_dir()?.join("stance").join("ddc.json"))
}

fn load_cache() -> BTreeMap<String, u32> {
    let Ok(p) = cache_path() else {
        return BTreeMap::new();
    };
    let Ok(s) = crate::store::read_checked(&p) else {
        return BTreeMap::new();
    };
    let m: BTreeMap<String, u32> = serde_json::from_str(&s).unwrap_or_default();
    m.into_iter().filter(|(k, _)| valid_connector(k)).collect()
}

fn save_cache(m: &BTreeMap<String, u32>) {
    let Ok(p) = cache_path() else { return };
    let Some(dir) = p.parent() else { return };
    if crate::store::ensure_private_dir(dir).is_err() {
        return;
    }
    if let Ok(s) = serde_json::to_string(m) {
        let _ = crate::store::atomic_write(&p, s.as_bytes(), false);
    }
}

fn detect() -> Result<BTreeMap<String, u32>> {
    let dd = tool::find("ddcutil")
        .ok_or_else(|| Error::new("brightness", "ddcutil not found (install ddcutil)"))?;
    let out = tool::run_tool(&dd, &["detect", "--brief"], DETECT_TIMEOUT)?;
    let m: BTreeMap<String, u32> = parse_detect(&out).into_iter().collect();
    save_cache(&m);
    Ok(m)
}

fn vcp_get(bus: u32) -> Result<(u32, u32)> {
    let dd = tool::find("ddcutil")
        .ok_or_else(|| Error::new("brightness", "ddcutil not found (install ddcutil)"))?;
    let out = tool::run_tool(
        &dd,
        &["getvcp", "10", "--brief", "--bus", &bus.to_string()],
        VCP_TIMEOUT,
    )?;
    parse_getvcp(&out)
}

/// Reads DDC brightness for `connector`, using the cached bus first. On a
/// failure the cache is refreshed once and the read retried.
fn ddc_get(connector: &str, cache: &mut BTreeMap<String, u32>) -> Result<Display> {
    let mk = |(v, m): (u32, u32)| Display {
        monitor: connector.into(),
        kind: "ddc",
        value: v,
        max: m,
        percent: percent(v, m),
    };
    if let Some(&bus) = cache.get(connector)
        && let Ok(r) = vcp_get(bus)
    {
        return Ok(mk(r));
    }
    *cache = detect()?;
    let bus = *cache.get(connector).ok_or_else(|| {
        Error::new(
            "brightness_not_found",
            format!("no DDC display for {connector}"),
        )
    })?;
    Ok(mk(vcp_get(bus)?))
}

// ---------- public API ----------

/// Brightness of every given connector that has a controllable device.
/// Monitors without one are simply absent. Runs `ddcutil detect` only when
/// the cache lacks an external connector.
pub fn list(connectors: &[String]) -> Result<Vec<Display>> {
    let mut out = Vec::new();
    let mut cache = load_cache();
    let mut detected = false;
    let mut bl_used = false;
    for c in connectors.iter().filter(|c| valid_connector(c)) {
        if is_internal(c) {
            if !bl_used && let Some(b) = backlight() {
                bl_used = true;
                out.push(bl_display(c, &b));
            }
            continue;
        }
        if tool::find("ddcutil").is_none() {
            continue;
        }
        if !cache.contains_key(c.as_str()) && !detected {
            detected = true;
            if let Ok(m) = detect() {
                cache = m;
            }
        }
        if let Some(&bus) = cache.get(c.as_str()) {
            match vcp_get(bus) {
                Ok((v, m)) => out.push(Display {
                    monitor: c.clone(),
                    kind: "ddc",
                    value: v,
                    max: m,
                    percent: percent(v, m),
                }),
                Err(_) => {
                    // stale bus: drop it so the next call re-detects
                    cache.remove(c.as_str());
                    save_cache(&cache);
                }
            }
        }
    }
    Ok(out)
}

pub fn get(monitor: &str) -> Result<Display> {
    if !valid_connector(monitor) {
        return Err(Error::new("brightness", "invalid monitor name"));
    }
    if is_internal(monitor) {
        let b =
            backlight().ok_or_else(|| Error::new("brightness_not_found", "no backlight device"))?;
        return Ok(bl_display(monitor, &b));
    }
    ddc_get(monitor, &mut load_cache())
}

/// Pure: target percent and clamp warning for a change from `cur` percent.
fn target(cur: u8, c: Change, min: u32) -> (u32, Option<String>) {
    let want: i32 = match c {
        Change::Abs(n) => i32::from(n),
        Change::Up(n) => i32::from(cur) + i32::from(n),
        Change::Down(n) => i32::from(cur) - i32::from(n),
    };
    let want = want.clamp(0, 100) as u32;
    if want < min {
        (min, Some(format!("clamped to {min}% (minimum)")))
    } else {
        (want, None)
    }
}

pub fn set(monitor: &str, c: Change) -> Result<SetResult> {
    if !valid_connector(monitor) {
        return Err(Error::new("brightness", "invalid monitor name"));
    }
    if is_internal(monitor) {
        let b =
            backlight().ok_or_else(|| Error::new("brightness_not_found", "no backlight device"))?;
        if !valid_device(&b.dev) {
            return Err(Error::new("brightness", "invalid backlight device name"));
        }
        let cur = percent(b.value, b.max);
        let (pct, warn) = target(cur, c, MIN_BACKLIGHT_PCT);
        let raw = ((u64::from(pct) * u64::from(b.max) + 50) / 100).max(1) as u32;
        let bc = tool::find("brightnessctl")
            .ok_or_else(|| Error::new("brightness", "brightnessctl not found"))?;
        tool::run_tool(&bc, &["-d", &b.dev, "set", &raw.to_string()], BL_TIMEOUT)?;
        let nb = Backlight {
            dev: b.dev,
            value: raw,
            max: b.max,
        };
        return Ok(SetResult {
            display: bl_display(monitor, &nb),
            warnings: warn.into_iter().collect(),
        });
    }
    let mut cache = load_cache();
    let cur = ddc_get(monitor, &mut cache)?;
    let (pct, warn) = target(cur.percent, c, MIN_DDC);
    let raw = ((u64::from(pct) * u64::from(cur.max) + 50) / 100).max(1) as u32;
    let bus = *cache.get(monitor).ok_or_else(|| {
        Error::new(
            "brightness_not_found",
            format!("no DDC display for {monitor}"),
        )
    })?;
    let dd = tool::find("ddcutil")
        .ok_or_else(|| Error::new("brightness", "ddcutil not found (install ddcutil)"))?;
    let res = tool::run_tool(
        &dd,
        &[
            "setvcp",
            "10",
            &raw.to_string(),
            "--bus",
            &bus.to_string(),
            "--noverify",
        ],
        VCP_TIMEOUT,
    );
    if let Err(e) = res {
        cache.remove(monitor);
        save_cache(&cache);
        return Err(e);
    }
    Ok(SetResult {
        display: Display {
            monitor: monitor.into(),
            kind: "ddc",
            value: raw,
            max: cur.max,
            percent: percent(raw, cur.max),
        },
        warnings: warn.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DETECT: &str = include_str!("../tests/fixtures/ddcutil-detect-brief.txt");
    const VCP: &str = include_str!("../tests/fixtures/ddcutil-getvcp-brief.txt");

    #[test]
    fn detect_skips_invalid() {
        assert_eq!(parse_detect(DETECT), vec![("DP-1".to_string(), 3)]);
        assert!(parse_detect("").is_empty());
        assert!(parse_detect("Display 1\n   I2C bus: /dev/i2c-x\n").is_empty());
    }

    #[test]
    fn detect_two_valid() {
        let t = "Display 1\n   I2C bus: /dev/i2c-3\n   DRM_connector: card0-DP-1\n\nDisplay 2\n   I2C bus: /dev/i2c-4\n   DRM connector: card0-HDMI-A-1\n";
        assert_eq!(
            parse_detect(t),
            vec![("DP-1".into(), 3), ("HDMI-A-1".into(), 4)]
        );
    }

    #[test]
    fn detect_rejects_hostile_connector() {
        let t = "Display 1\n   I2C bus: /dev/i2c-3\n   DRM connector: card0-DP 1;rm\n";
        assert!(parse_detect(t).is_empty());
    }

    #[test]
    fn getvcp() {
        assert_eq!(parse_getvcp(VCP).unwrap(), (1, 100));
        assert!(parse_getvcp("VCP 10 ERR").is_err());
        assert!(parse_getvcp("VCP 10 C 5 0").is_err());
        assert!(parse_getvcp("VCP 10 C 500 100").is_err());
        assert!(parse_getvcp("").is_err());
    }

    #[test]
    fn change_parse() {
        assert_eq!(parse_change("60").unwrap(), Change::Abs(60));
        assert_eq!(parse_change("+10").unwrap(), Change::Up(10));
        assert_eq!(parse_change("-5").unwrap(), Change::Down(5));
        for b in ["", "+", "101", "1.5", "abc", "- 5", "1000", "--5", "-0x5"] {
            assert!(parse_change(b).is_err(), "{b}");
        }
    }

    #[test]
    fn targets_and_clamps() {
        assert_eq!(target(50, Change::Abs(60), 5), (60, None));
        assert_eq!(target(50, Change::Abs(0), 5).0, 5);
        assert!(target(50, Change::Abs(0), 5).1.is_some());
        assert_eq!(target(98, Change::Up(10), 1), (100, None));
        assert_eq!(target(3, Change::Down(10), 1).0, 1);
        assert_eq!(target(50, Change::Abs(0), 1).0, 1);
    }

    #[test]
    fn internal_and_names() {
        assert!(is_internal("eDP-1") && is_internal("LVDS-1") && is_internal("DSI-1"));
        assert!(!is_internal("DP-1"));
        assert!(valid_connector("HDMI-A-1") && !valid_connector("a b") && !valid_connector(""));
        assert!(valid_device("nvidia_0") && !valid_device("a/b"));
        assert_eq!(percent(1, 3), 33);
        assert_eq!(percent(5, 0), 0);
    }

    #[test]
    fn backlight_prefers_raw() {
        let d = std::env::temp_dir().join(format!("stance-bl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for (n, t, v) in [("acpi_video0", "firmware", "7"), ("intel", "raw", "50")] {
            let p = d.join(n);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("type"), t).unwrap();
            std::fs::write(p.join("brightness"), v).unwrap();
            std::fs::write(p.join("max_brightness"), "100").unwrap();
        }
        let b = backlight_in(&d).unwrap();
        assert_eq!((b.dev.as_str(), b.value), ("intel", 50));
        let _ = std::fs::remove_dir_all(&d);
    }
}
