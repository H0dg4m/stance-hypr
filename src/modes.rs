//! Mode files (`~/.config/stance/modes/<name>.toml`) and hotplug rules
//! (`~/.config/stance/rules.toml`). Both are strict: unknown fields are
//! errors, sizes and counts are capped, and there is no way to run commands.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::apply::State;
use crate::backend::{Backend, OptKey, OptValue};
use crate::error::{Error, Result};
use crate::model::{Mode, Monitor, sanitize};
use crate::night::NightSetting;
use crate::plan::{parse_pos, pick_mode, resolve};
use crate::power::PowerProfile;
use crate::spec::MonitorSpec;
use crate::store;

const MAX_MONITORS: usize = 16;
const MAX_WORKSPACES: usize = 100;
const MAX_RULES: usize = 32;

pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn check_name(s: &str) -> Result<()> {
    if valid_name(s) {
        Ok(())
    } else {
        Err(Error::new(
            "bad_name",
            "mode names may only use letters, digits, - and _ (max 64)",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Gaps {
    All(u16),
    Css([u16; 4]),
}

impl Gaps {
    fn value(&self) -> OptValue {
        match self {
            Gaps::All(v) => OptValue::Gaps([*v; 4]),
            Gaps::Css(v) => OptValue::Gaps(*v),
        }
    }

    fn from_value(v: OptValue) -> Option<Self> {
        match v {
            OptValue::Gaps(g) if g.iter().all(|x| *x == g[0]) => Some(Gaps::All(g[0])),
            OptValue::Gaps(g) => Some(Gaps::Css(g)),
            OptValue::Bool(_) => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HyprSection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animations: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blur: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gaps_in: Option<Gaps>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gaps_out: Option<Gaps>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_tearing: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeMonitor {
    #[serde(rename = "match")]
    pub matcher: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// `2560x1440@180` or `preferred`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// `1920x0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vrr: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<u8>,
    /// Match string of the monitor to mirror.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mirror: Option<String>,
}

/// `schema = N` of a mode or rules file. Missing reads as 1; always written.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Schema(pub u32);

impl Default for Schema {
    fn default() -> Self {
        Schema(crate::migrate::CURRENT)
    }
}

/// Parse errors of a file from a newer stance are `newer_schema`, not a
/// complaint about keys this build does not know.
fn parse_error(
    text: &str,
    file: &str,
    code: &'static str,
    what: &str,
    e: &toml::de::Error,
) -> Error {
    let found = crate::migrate::toml_schema(text);
    if found > crate::migrate::CURRENT {
        crate::migrate::newer(file, found)
    } else {
        Error::new(code, format!("{what}: {}", e.message()))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeFile {
    #[serde(default)]
    pub schema: Schema,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_profile: Option<PowerProfile>,
    /// `"1-5" = "JDR EPIC Q40"`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub workspaces: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hyprland: Option<HyprSection>,
    /// `false`, `true` or a temperature in K (hyprsunset).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub night_light: Option<NightSetting>,
    /// Default audio output: sink id, node name or a unique substring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_sink: Option<String>,
    /// `"eDP-1" = 60`: monitor match → brightness percent.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub brightness: BTreeMap<String, u8>,
    #[serde(default, rename = "monitor", skip_serializing_if = "Vec::is_empty")]
    pub monitors: Vec<ModeMonitor>,
}

/// Settings a mode applies after the display change. They cannot black out
/// a screen, so they are not part of the revert snapshot, and a failure
/// (headset unplugged, no DDC) is a warning, never an error.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Quick {
    pub night: Option<NightSetting>,
    pub audio: Option<String>,
    /// (connector, percent).
    pub brightness: Vec<(String, u8)>,
}

impl Quick {
    pub fn is_empty(&self) -> bool {
        self.night.is_none() && self.audio.is_none() && self.brightness.is_empty()
    }
}

pub fn modes_dir() -> Result<PathBuf> {
    Ok(store::config_dir()?.join("modes"))
}

fn mode_path(name: &str) -> Result<PathBuf> {
    check_name(name)?;
    Ok(modes_dir()?.join(format!("{name}.toml")))
}

pub fn parse_mode(text: &str) -> Result<ModeFile> {
    let m: ModeFile = toml::from_str(text)
        .map_err(|e| parse_error(text, "mode file", "bad_mode", "invalid mode file", &e))?;
    if m.monitors.len() > MAX_MONITORS {
        return Err(Error::new("bad_mode", "too many [[monitor]] entries"));
    }
    if m.description
        .as_ref()
        .is_some_and(|d| d.chars().count() > 200)
    {
        return Err(Error::new("bad_mode", "description is too long"));
    }
    workspace_ids(&m.workspaces)?;
    if let Some(n) = m.night_light {
        n.check()
            .map_err(|e| Error::new("bad_mode", format!("night_light: {}", e.message)))?;
    }
    if let Some(a) = &m.audio_sink {
        crate::audio::validate_match(a)
            .map_err(|e| Error::new("bad_mode", format!("audio_sink: {}", e.message)))?;
    }
    if m.brightness.len() > MAX_MONITORS {
        return Err(Error::new("bad_mode", "too many [brightness] entries"));
    }
    if let Some((k, _)) = m.brightness.iter().find(|(_, v)| **v > 100) {
        return Err(Error::new(
            "bad_mode",
            format!("brightness for `{}` must be 0-100", sanitize(k)),
        ));
    }
    Ok(m)
}

pub fn load(name: &str) -> Result<ModeFile> {
    let path = mode_path(name)?;
    if !path.exists() {
        return Err(Error::new(
            "unknown_mode",
            format!("no mode named `{name}`"),
        ));
    }
    parse_mode(&store::read_checked(&path)?)
}

#[derive(Debug, Serialize)]
pub struct ModeEntry {
    pub name: String,
    pub description: Option<String>,
    pub error: Option<String>,
}

pub fn list() -> Result<Vec<ModeEntry>> {
    let dir = modes_dir()?;
    let Ok(rd) = fs::read_dir(&dir) else {
        return Ok(vec![]);
    };
    let mut out: Vec<ModeEntry> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let f = e.file_name().into_string().ok()?;
            let name = f.strip_suffix(".toml")?.to_string();
            valid_name(&name).then_some(name)
        })
        .map(|name| match load(&name) {
            Ok(m) => ModeEntry {
                name,
                description: m.description,
                error: None,
            },
            Err(e) => ModeEntry {
                name,
                description: None,
                error: Some(e.message),
            },
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn save(name: &str, mode: &ModeFile, overwrite: bool) -> Result<PathBuf> {
    let path = mode_path(name)?;
    if path.exists() && !overwrite {
        return Err(Error::new(
            "exists",
            format!("mode `{name}` exists (use --force to replace)"),
        ));
    }
    store::ensure_private_dir(&store::config_dir()?)?;
    store::ensure_private_dir(&modes_dir()?)?;
    crate::migrate::guard_toml(&path, &format!("modes/{name}.toml"))?;
    let mut mode = mode.clone();
    mode.schema = Schema::default();
    let body = toml::to_string(&mode).map_err(|e| Error::new("io", e.to_string()))?;
    let text = format!("# stance mode `{name}` (see `stance mode show {name}`)\n{body}");
    store::atomic_write(&path, text.as_bytes(), true)?;
    Ok(path)
}

/// A mode from the panel editor: JSON with the same keys as the TOML file,
/// checked by the same strict parser (unknown keys, caps, ranges).
pub fn from_json(data: &str) -> Result<ModeFile> {
    if data.len() as u64 > store::MAX_INPUT {
        return Err(Error::new("bad_mode", "mode data is too large"));
    }
    let m: ModeFile = serde_json::from_str(data)
        .map_err(|e| Error::new("bad_mode", format!("invalid mode data: {e}")))?;
    let text = toml::to_string(&m).map_err(|e| Error::new("bad_mode", e.to_string()))?;
    parse_mode(&text)
}

pub fn rules_from_json(data: &str) -> Result<Rules> {
    if data.len() as u64 > store::MAX_INPUT {
        return Err(Error::new("bad_rules", "rules data is too large"));
    }
    let r: Rules = serde_json::from_str(data)
        .map_err(|e| Error::new("bad_rules", format!("invalid rules data: {e}")))?;
    let text = toml::to_string(&r).map_err(|e| Error::new("bad_rules", e.to_string()))?;
    let r = parse_rules(&text)?;
    // A rule for a mode that does not exist would fail on every hotplug.
    for rule in &r.rules {
        load(&rule.mode).map_err(|e| {
            Error::new(
                "bad_rules",
                format!("rule for `{}`: {}", rule.mode, e.message),
            )
        })?;
    }
    Ok(r)
}

pub fn save_rules(r: &Rules) -> Result<PathBuf> {
    let path = rules_path()?;
    store::ensure_private_dir(&store::config_dir()?)?;
    crate::migrate::guard_toml(&path, "rules.toml")?;
    let mut r = r.clone();
    r.schema = Schema::default();
    let body = toml::to_string(&r).map_err(|e| Error::new("io", e.to_string()))?;
    let text = format!("# stance hotplug rules (see `stance rules`)\n{body}");
    store::atomic_write(&path, text.as_bytes(), true)?;
    Ok(path)
}

pub fn remove(name: &str) -> Result<()> {
    let path = mode_path(name)?;
    fs::remove_file(&path).map_err(|e| Error::new("unknown_mode", format!("{name}: {e}")))
}

/// Expands `"1-5"`, `"3"` keys into workspace ids.
fn workspace_ids(map: &BTreeMap<String, String>) -> Result<Vec<(u32, String)>> {
    let mut out = Vec::new();
    for (k, v) in map {
        let bad = || Error::new("bad_mode", format!("bad workspace range `{}`", sanitize(k)));
        let (a, b) = match k.split_once('-') {
            Some((a, b)) => (
                a.trim().parse::<u32>().map_err(|_| bad())?,
                b.trim().parse::<u32>().map_err(|_| bad())?,
            ),
            None => {
                let n = k.trim().parse::<u32>().map_err(|_| bad())?;
                (n, n)
            }
        };
        if a == 0 || b < a {
            return Err(bad());
        }
        for id in a..=b {
            out.push((id, v.clone()));
            if out.len() > MAX_WORKSPACES {
                return Err(Error::new("bad_mode", "too many workspaces"));
            }
        }
    }
    Ok(out)
}

/// Resolves a mode against the connected monitors. Monitors the mode names
/// but that are not connected are skipped with a warning, so one mode can
/// work with and without an external screen.
pub fn resolve_mode(m: &ModeFile, mons: &[Monitor], extras: bool) -> Result<(State, Vec<String>)> {
    let mut st = State::default();
    let mut warnings = Vec::new();
    for mm in &m.monitors {
        let cur = match resolve(mons, &mm.matcher) {
            Ok(c) => c,
            Err(e) if e.code == "unknown_monitor" => {
                warnings.push(format!(
                    "skipped `{}`: not connected",
                    sanitize(&mm.matcher)
                ));
                continue;
            }
            Err(e) => return Err(e),
        };
        let mut s = MonitorSpec::from_monitor(cur);
        s.enabled = mm.enabled.unwrap_or(true);
        if s.enabled {
            let mode = match mm.mode.as_deref() {
                None if cur.enabled => None,
                None | Some("preferred") => crate::plan::preferred(cur),
                Some(txt) => {
                    let want = Mode::parse(txt).ok_or_else(|| {
                        Error::new("bad_mode", format!("bad mode `{}`", sanitize(txt)))
                    })?;
                    Some(pick_mode(
                        cur,
                        (want.width, want.height),
                        Some(want.refresh),
                        false,
                    )?)
                }
            };
            if let Some(md) = mode {
                (s.width, s.height, s.refresh) = (md.width, md.height, md.refresh);
            }
            if let Some(p) = &mm.position {
                (s.x, s.y) = parse_pos(p)?;
            }
            if let Some(v) = mm.scale {
                s.scale = v;
            }
            if let Some(v) = mm.vrr {
                s.vrr = v;
            }
            if let Some(v) = mm.transform {
                s.transform = v;
            }
            s.mirror = match &mm.mirror {
                Some(q) => Some(resolve(mons, q)?.name.clone()),
                None => None,
            };
        }
        s.validate()?;
        st.monitors.push(s);
    }
    for (id, q) in workspace_ids(&m.workspaces)? {
        match resolve(mons, &q) {
            Ok(mon) => st.workspaces.push((id, mon.name.clone())),
            Err(_) => {
                warnings.push(format!("workspace {id}: `{}` not connected", sanitize(&q)));
            }
        }
    }
    if extras {
        if let Some(h) = &m.hyprland {
            let pairs = [
                (OptKey::Animations, h.animations.map(OptValue::Bool)),
                (OptKey::Blur, h.blur.map(OptValue::Bool)),
                (OptKey::Tearing, h.allow_tearing.map(OptValue::Bool)),
                (OptKey::GapsIn, h.gaps_in.as_ref().map(Gaps::value)),
                (OptKey::GapsOut, h.gaps_out.as_ref().map(Gaps::value)),
            ];
            for (k, v) in pairs {
                if let Some(v) = v {
                    v.check(k)?;
                    st.options.insert(k, v);
                }
            }
        }
        st.power = m.power_profile;
    }
    warnings.extend(crate::plan::check_result(mons, &st.monitors)?);
    Ok((st, warnings))
}

/// The mode's quick settings (only with extras), with brightness keys
/// resolved to connectors. Unconnected monitors are skipped with a warning.
pub fn resolve_quick(m: &ModeFile, mons: &[Monitor], extras: bool) -> (Quick, Vec<String>) {
    let mut q = Quick::default();
    let mut warnings = Vec::new();
    if !extras {
        return (q, warnings);
    }
    q.night = m.night_light;
    q.audio = m.audio_sink.clone();
    for (k, v) in &m.brightness {
        match resolve(mons, k) {
            Ok(mon) => q.brightness.push((mon.name.clone(), *v)),
            Err(_) => warnings.push(format!("brightness: `{}` not connected", sanitize(k))),
        }
    }
    (q, warnings)
}

/// Applies quick settings best-effort: returns (changed, warnings).
pub fn apply_quick(q: &Quick, night: Option<&crate::night::Night>) -> (Vec<String>, Vec<String>) {
    let mut changed = Vec::new();
    let mut warnings = Vec::new();
    if let Some(n) = q.night {
        match night.map(|nl| n.apply(nl)) {
            Some(Ok(st)) => changed.push(match st.temperature {
                Some(k) if st.on => format!("night light {k} K"),
                _ => "night light off".to_string(),
            }),
            Some(Err(e)) => warnings.push(format!("night light: {}", e.message)),
            None => warnings.push("night light: not available on this backend".to_string()),
        }
    }
    if let Some(a) = &q.audio {
        match crate::audio::set_default(a) {
            Ok(s) => changed.push(format!("audio {}", sanitize(&s.description))),
            Err(e) => warnings.push(format!("audio: {}", e.message)),
        }
    }
    for (mon, pct) in &q.brightness {
        match crate::brightness::set(mon, crate::brightness::Change::Abs(*pct)) {
            Ok(r) => {
                changed.push(format!("brightness {mon} {}%", r.display.percent));
                warnings.extend(
                    r.warnings
                        .into_iter()
                        .map(|w| format!("brightness {mon}: {w}")),
                );
            }
            Err(e) => warnings.push(format!("brightness {mon}: {}", e.message)),
        }
    }
    (changed, warnings)
}

/// Builds a mode file from the live state.
pub fn from_current(backend: &dyn Backend, description: Option<String>) -> Result<ModeFile> {
    let mons = backend.monitors()?;
    let mut out = ModeFile {
        description,
        ..Default::default()
    };
    for m in &mons {
        // Prefer a stable, unique human match over the connector name.
        let matcher = [m.description.clone(), format!("{} {}", m.make, m.model)]
            .into_iter()
            .find(|q| !q.trim().is_empty() && resolve(&mons, q).is_ok_and(|r| r.name == m.name))
            .unwrap_or_else(|| m.name.clone());
        let s = MonitorSpec::from_monitor(m);
        out.monitors.push(if m.enabled {
            ModeMonitor {
                matcher,
                enabled: None,
                mode: Some(s.mode_string()),
                position: Some(format!("{}x{}", s.x, s.y)),
                scale: Some(s.scale),
                vrr: Some(s.vrr),
                transform: (s.transform != 0).then_some(s.transform),
                mirror: m.mirror_of.clone(),
            }
        } else {
            ModeMonitor {
                matcher,
                enabled: Some(false),
                mode: None,
                position: None,
                scale: None,
                vrr: None,
                transform: None,
                mirror: None,
            }
        });
    }
    let get = |k| backend.get_option(k).ok();
    let as_bool = |v: Option<OptValue>| match v {
        Some(OptValue::Bool(b)) => Some(b),
        _ => None,
    };
    out.hyprland = Some(HyprSection {
        animations: as_bool(get(OptKey::Animations)),
        blur: as_bool(get(OptKey::Blur)),
        allow_tearing: as_bool(get(OptKey::Tearing)),
        gaps_in: get(OptKey::GapsIn).and_then(Gaps::from_value),
        gaps_out: get(OptKey::GapsOut).and_then(Gaps::from_value),
    });
    out.power_profile = backend.power_get().ok();
    Ok(out)
}

// ---- hotplug rules ---------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Exact set of connected monitors (match strings) this rule is for.
    pub monitors: Vec<String>,
    pub mode: String,
    /// Also apply the mode's Hyprland options and power profile.
    #[serde(default)]
    pub allow_extras: bool,
    /// Only on mains (`ac`) or only on `battery`. A rule with a matching
    /// `on_power` wins over one without.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_power: Option<PowerSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerSource {
    Ac,
    Battery,
}

impl PowerSource {
    pub fn from_ac(ac: Option<bool>) -> Option<Self> {
        ac.map(|on| {
            if on {
                PowerSource::Ac
            } else {
                PowerSource::Battery
            }
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    #[serde(default)]
    pub schema: Schema,
    #[serde(default, rename = "rule")]
    pub rules: Vec<Rule>,
}

pub fn rules_path() -> Result<PathBuf> {
    Ok(store::config_dir()?.join("rules.toml"))
}

pub fn parse_rules(text: &str) -> Result<Rules> {
    let r: Rules = toml::from_str(text)
        .map_err(|e| parse_error(text, "rules.toml", "bad_rules", "invalid rules.toml", &e))?;
    if r.rules.len() > MAX_RULES {
        return Err(Error::new("bad_rules", "too many rules"));
    }
    for rule in &r.rules {
        check_name(&rule.mode)?;
        if rule.monitors.is_empty() || rule.monitors.len() > MAX_MONITORS {
            return Err(Error::new("bad_rules", "each rule needs 1-16 monitors"));
        }
    }
    Ok(r)
}

pub fn load_rules(path: &Path) -> Result<Rules> {
    if !path.exists() {
        return Ok(Rules::default());
    }
    parse_rules(&store::read_checked(path)?)
}

/// The rule for the connected monitor set (exact set equality). Among
/// those, one whose `on_power` equals the current source wins; otherwise
/// the first without `on_power`.
pub fn matching_rule<'a>(
    rules: &'a Rules,
    mons: &[Monitor],
    power: Option<PowerSource>,
) -> Option<&'a Rule> {
    let set_matches = |r: &&Rule| {
        if r.monitors.len() != mons.len() {
            return false;
        }
        let mut seen: Vec<&str> = Vec::new();
        r.monitors.iter().all(|q| match resolve(mons, q) {
            Ok(m) if !seen.contains(&m.name.as_str()) => {
                seen.push(&m.name);
                true
            }
            _ => false,
        })
    };
    let candidates: Vec<&Rule> = rules.rules.iter().filter(set_matches).collect();
    candidates
        .iter()
        .find(|r| r.on_power.is_some() && r.on_power == power)
        .or_else(|| candidates.iter().find(|r| r.on_power.is_none()))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::tests::{Fake, mon};

    const GAMING: &str = r#"
description = "External only, effects off"
power_profile = "performance"
workspaces = { "1-3" = "Desc A-1" }

[hyprland]
animations = false
blur = false
gaps_out = [1, 2, 3, 4]
gaps_in = 2

[[monitor]]
match = "Desc A-1"
mode = "1920x1080@60"
position = "0x0"

[[monitor]]
match = "B-1"
enabled = false

[[monitor]]
match = "Not Connected"
enabled = false
"#;

    #[test]
    fn parse_and_resolve() {
        let m = parse_mode(GAMING).unwrap();
        let mons = vec![mon("A-1", 0), mon("B-1", 1920)];
        let (st, warn) = resolve_mode(&m, &mons, true).unwrap();
        assert_eq!(st.monitors.len(), 2);
        assert!(!st.monitors[1].enabled);
        assert_eq!(
            st.workspaces,
            vec![(1, "A-1".into()), (2, "A-1".into()), (3, "A-1".into())]
        );
        assert_eq!(st.options[&OptKey::GapsOut], OptValue::Gaps([1, 2, 3, 4]));
        assert_eq!(st.options[&OptKey::GapsIn], OptValue::Gaps([2; 4]));
        assert_eq!(st.power, Some(PowerProfile::Performance));
        assert_eq!(warn.len(), 1);
        // Without extras: no options / power.
        let (st, _) = resolve_mode(&m, &mons, false).unwrap();
        assert!(st.options.is_empty() && st.power.is_none());
    }

    #[test]
    fn strictness() {
        assert!(parse_mode("command = \"rm -rf ~\"").is_err());
        assert!(parse_mode("[hyprland]\nexec = \"x\"").is_err());
        assert!(parse_mode("[[monitor]]\nmatch = \"A\"\nfoo = 1").is_err());
        assert!(parse_mode("workspaces = { \"5-1\" = \"A\" }").is_err());
        assert!(parse_mode("workspaces = { \"1-500\" = \"A\" }").is_err());
        assert!(parse_mode("power_profile = \"turbo\"").is_err());
        assert!(!valid_name("../x") && !valid_name("a b") && valid_name("work_2"));
    }

    #[test]
    fn quick_settings() {
        let m = parse_mode(
            "night_light = 3500\naudio_sink = \"Razer\"\n[brightness]\n\"Desc A-1\" = 40\n\"Gone\" = 10\n",
        )
        .unwrap();
        let mons = vec![mon("A-1", 0), mon("B-1", 1920)];
        let (q, w) = resolve_quick(&m, &mons, true);
        assert_eq!(q.night, Some(NightSetting::Kelvin(3500)));
        assert_eq!(q.audio.as_deref(), Some("Razer"));
        assert_eq!(q.brightness, vec![("A-1".to_string(), 40)]);
        assert_eq!(w.len(), 1);
        // Quick settings are extras.
        assert!(resolve_quick(&m, &mons, false).0.is_empty());
        assert!(parse_mode("night_light = true").is_ok());
        assert!(parse_mode("night_light = 500").is_err());
        assert!(parse_mode("night_light = \"warm\"").is_err());
        assert!(parse_mode("audio_sink = \"\"").is_err());
        assert!(parse_mode("[brightness]\n\"A\" = 101").is_err());
        assert!(parse_mode("[brightness]\n\"A\" = -1").is_err());
        assert!(parse_mode("[brightness]\n\"A\" = \"50; rm\"").is_err());
    }

    #[test]
    fn json_round_trip_is_strict() {
        let m = parse_mode(GAMING).unwrap();
        let j = serde_json::to_string(&m).unwrap();
        assert_eq!(from_json(&j).unwrap(), m);
        assert!(from_json("{\"exec\": \"x\"}").is_err());
        assert!(from_json("{\"workspaces\": {\"1-900\": \"A\"}}").is_err());
        assert!(from_json("{\"brightness\": {\"A\": 300}}").is_err());
        assert!(from_json("not json").is_err());
        assert!(rules_from_json("{\"rule\": [{\"monitors\": [], \"mode\": \"x\"}]}").is_err());
    }

    #[test]
    fn mode_that_turns_everything_off_is_refused() {
        let m = parse_mode("[[monitor]]\nmatch = \"A-1\"\nenabled = false\n[[monitor]]\nmatch = \"B-1\"\nenabled = false").unwrap();
        let mons = vec![mon("A-1", 0), mon("B-1", 1920)];
        assert_eq!(resolve_mode(&m, &mons, true).unwrap_err().code, "no_screen");
    }

    #[test]
    fn roundtrip_from_current() {
        let b = Fake::new();
        let m = from_current(&b, Some("now".into())).unwrap();
        let text = toml::to_string(&m).unwrap();
        let back = parse_mode(&text).unwrap();
        assert_eq!(back, m);
        let (st, w) = resolve_mode(&back, &b.monitors().unwrap(), true).unwrap();
        assert!(w.is_empty());
        assert_eq!(st.monitors.len(), 2);
    }

    #[test]
    fn shipped_examples_parse() {
        for text in [
            include_str!("../examples/modes/gaming.toml"),
            include_str!("../examples/modes/desk.toml"),
            include_str!("../examples/modes/laptop.toml"),
        ] {
            parse_mode(text).unwrap();
        }
        assert_eq!(
            parse_rules(include_str!("../examples/rules.toml"))
                .unwrap()
                .rules
                .len(),
            2
        );
    }

    #[test]
    fn rules_match_exact_set() {
        let r = parse_rules(
            "[[rule]]\nmonitors = [\"Desc A-1\"]\nmode = \"laptop\"\n\n[[rule]]\nmonitors = [\"Desc A-1\", \"B-1\"]\nmode = \"desk\"\nallow_extras = true\n",
        )
        .unwrap();
        let one = vec![mon("A-1", 0)];
        let two = vec![mon("A-1", 0), mon("B-1", 1920)];
        let three = vec![mon("A-1", 0), mon("B-1", 1920), mon("C-1", 3840)];
        assert_eq!(matching_rule(&r, &one, None).unwrap().mode, "laptop");
        assert_eq!(matching_rule(&r, &two, None).unwrap().mode, "desk");
        assert!(matching_rule(&r, &three, None).is_none());
        // Power-specific rules win when the source matches.
        let p = parse_rules(
            "[[rule]]\nmonitors = [\"Desc A-1\"]\nmode = \"laptop\"\n\n[[rule]]\nmonitors = [\"Desc A-1\"]\nmode = \"battery\"\non_power = \"battery\"\n",
        )
        .unwrap();
        assert_eq!(
            matching_rule(&p, &one, Some(PowerSource::Battery))
                .unwrap()
                .mode,
            "battery"
        );
        assert_eq!(
            matching_rule(&p, &one, Some(PowerSource::Ac)).unwrap().mode,
            "laptop"
        );
        assert_eq!(matching_rule(&p, &one, None).unwrap().mode, "laptop");
        assert!(
            parse_rules("[[rule]]\nmonitors = [\"a\"]\nmode = \"x\"\non_power = \"solar\"")
                .is_err()
        );
        assert!(parse_rules("[[rule]]\nmonitors = []\nmode = \"x\"").is_err());
        assert!(parse_rules("[[rule]]\nmonitors = [\"a\"]\nmode = \"../x\"").is_err());
    }

    #[test]
    fn old_fixtures_load_and_are_stamped() {
        for dir in ["config-0.2.0", "config-0.3.0"] {
            let base = format!("{}/tests/fixtures/{dir}", env!("CARGO_MANIFEST_DIR"));
            let m =
                parse_mode(&std::fs::read_to_string(format!("{base}/modes/desk.toml")).unwrap())
                    .unwrap();
            assert_eq!(m.schema, Schema(1), "{dir}");
            let r = parse_rules(&std::fs::read_to_string(format!("{base}/rules.toml")).unwrap())
                .unwrap();
            assert_eq!(r.schema, Schema(1), "{dir}");
            assert!(!r.rules.is_empty());
        }
    }

    #[test]
    fn schema_is_written_first_and_newer_is_refused() {
        let body = toml::to_string(&ModeFile::default()).unwrap();
        assert!(body.starts_with("schema = 1"), "{body}");
        let r = toml::to_string(&Rules::default()).unwrap();
        assert!(r.starts_with("schema = 1"), "{r}");
        // A newer file that still parses is read; one with unknown keys is
        // refused as newer, not as a typo.
        assert!(parse_mode("schema = 2\n").is_ok());
        let e = parse_mode("schema = 2\nfuture_key = 1\n").unwrap_err();
        assert_eq!(e.code, "newer_schema");
        let e = parse_rules("schema = 3\nfuture_key = 1\n").unwrap_err();
        assert_eq!(e.code, "newer_schema");
        let e = parse_mode("future_key = 1\n").unwrap_err();
        assert_eq!(e.code, "bad_mode");
    }
}
