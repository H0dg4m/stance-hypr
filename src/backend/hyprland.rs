//! The Hyprland backend: socket1 requests, hyprlang paths for options, Lua
//! or legacy config rendering. Nothing outside this module knows about
//! hyprlang.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::Arc;

use serde::Deserialize;

use super::{Backend, Caps, Event, Next, Notice, OptKey, OptValue};
use crate::error::{Error, Result};
use crate::hypr::Instance;
use crate::model::{Mode, Monitor, Parser, sanitize};
use crate::spec::{MonitorSpec, check_connector};

/// Hyprland's event socket (`name>>data` lines) as neutral events.
struct Socket2(std::io::BufReader<std::os::unix::net::UnixStream>);

impl super::EventSource for Socket2 {
    fn next(&mut self, timeout: Option<std::time::Duration>) -> Result<Next> {
        use std::io::{BufRead, ErrorKind};
        self.0
            .get_ref()
            .set_read_timeout(timeout)
            .map_err(|e| Error::new("io", e.to_string()))?;
        let mut line = String::new();
        match self.0.read_line(&mut line) {
            Ok(0) => Ok(Next::Closed),
            Ok(_) => Ok(parse_event(line.trim_end())),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                Ok(Next::Tick)
            }
            Err(e) => Err(Error::new("ipc", format!("event socket: {e}"))),
        }
    }
}

/// Connector of a `monitoradded(v2)` / `monitorremoved(v2)` payload.
fn event_name(v2: bool, data: &str) -> String {
    let name = if v2 {
        data.split(',').nth(1).unwrap_or_default()
    } else {
        data
    };
    if crate::spec::valid_connector(name) {
        name.to_string()
    } else {
        String::new()
    }
}

fn parse_event(line: &str) -> Next {
    let (ev, data) = line.split_once(">>").unwrap_or((line, ""));
    match ev {
        "monitoradded" | "monitoraddedv2" => Next::Event(Event::MonitorAdded {
            name: event_name(ev.ends_with("v2"), data),
        }),
        "monitorremoved" | "monitorremovedv2" => Next::Event(Event::MonitorRemoved {
            name: event_name(ev.ends_with("v2"), data),
        }),
        "configreloaded" => Next::Event(Event::ConfigReloaded),
        _ => Next::Tick,
    }
}

/// hyprsunset answers on a socket in the Hyprland instance dir.
struct HyprsunsetLink(Instance);

impl crate::night::Link for HyprsunsetLink {
    fn ask(&self, req: &str) -> Result<String> {
        self.0.request_on(&self.0.dir.join(".hyprsunset.sock"), req)
    }

    fn env(&self) -> Vec<(&'static str, String)> {
        vec![("HYPRLAND_INSTANCE_SIGNATURE", self.0.signature.clone())]
    }
}

/// hyprlang / getoption path of a neutral option.
fn path(key: OptKey) -> &'static str {
    match key {
        OptKey::Animations => "animations:enabled",
        OptKey::Blur => "decoration:blur:enabled",
        OptKey::GapsIn => "general:gaps_in",
        OptKey::GapsOut => "general:gaps_out",
        OptKey::Tearing => "general:allow_tearing",
    }
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
        Ok(Self::new(Instance::discover()?))
    }

    /// `eval` replies `ok` or `error: …`. Its `ok` is not proof the change
    /// happened (callers verify), but an error is always an error.
    fn eval(&self, lua: &str) -> Result<()> {
        expect_ok(&self.instance.request(&format!("eval {lua}"))?)
    }

    fn keyword(&self, key: &str, value: &str) -> Result<()> {
        expect_ok(&self.instance.request(&format!("keyword {key} {value}"))?)
    }

    fn dispatch_legacy(&self, args: &str) -> Result<()> {
        expect_ok(&self.instance.request(&format!("dispatch {args}"))?)
    }
}

fn expect_ok(reply: &str) -> Result<()> {
    if reply.trim() == "ok" {
        Ok(())
    } else {
        Err(Error::new(
            "compositor",
            format!("Hyprland: {}", sanitize(reply.trim())),
        ))
    }
}

fn lua_opt(key: OptKey, value: OptValue) -> String {
    let v = match value {
        OptValue::Bool(b) => b.to_string(),
        OptValue::Gaps([t, r, b, l]) => {
            format!("{{ top = {t}, right = {r}, bottom = {b}, left = {l} }}")
        }
    };
    match key {
        OptKey::Animations => format!("hl.config({{ animations = {{ enabled = {v} }} }})"),
        OptKey::Blur => format!("hl.config({{ decoration = {{ blur = {{ enabled = {v} }} }} }})"),
        OptKey::GapsIn => format!("hl.config({{ general = {{ gaps_in = {v} }} }})"),
        OptKey::GapsOut => format!("hl.config({{ general = {{ gaps_out = {v} }} }})"),
        OptKey::Tearing => format!("hl.config({{ general = {{ allow_tearing = {v} }} }})"),
    }
}

fn legacy_opt(value: OptValue) -> String {
    match value {
        OptValue::Bool(b) => u8::from(b).to_string(),
        OptValue::Gaps([t, r, b, l]) => format!("{t} {r} {b} {l}"),
    }
}

/// Parses a `getoption -j` reply (bool / int / css variants across versions).
fn parse_option(key: OptKey, v: &serde_json::Value) -> Result<OptValue> {
    let bad = || {
        Error::new(
            "compositor",
            format!("unexpected getoption reply for {}", path(key)),
        )
    };
    if key.is_gap() {
        if let Some(css) = v.get("css").and_then(|c| c.as_str()) {
            let n: Vec<u16> = css
                .split_whitespace()
                .filter_map(|p| p.parse().ok())
                .collect();
            return match n.as_slice() {
                [a] => Ok(OptValue::Gaps([*a; 4])),
                [t, r, b, l] => Ok(OptValue::Gaps([*t, *r, *b, *l])),
                _ => Err(bad()),
            };
        }
        let i = v.get("int").and_then(|i| i.as_u64()).ok_or_else(bad)?;
        let i = u16::try_from(i).map_err(|_| bad())?;
        return Ok(OptValue::Gaps([i; 4]));
    }
    if let Some(b) = v.get("bool").and_then(|b| b.as_bool()) {
        return Ok(OptValue::Bool(b));
    }
    v.get("int")
        .and_then(|i| i.as_i64())
        .map(|i| OptValue::Bool(i != 0))
        .ok_or_else(bad)
}

impl Backend for Hyprland {
    fn name(&self) -> &'static str {
        "hyprland"
    }

    fn capabilities(&self) -> Caps {
        Caps {
            persist: true,
            events: true,
            ..Caps::default()
        }
    }

    fn option_name(&self, key: OptKey) -> String {
        path(key).to_string()
    }

    fn session_id(&self) -> &str {
        &self.instance.signature
    }

    fn night_link(&self) -> Option<Arc<dyn crate::night::Link>> {
        Some(Arc::new(HyprsunsetLink(self.instance.clone())))
    }

    fn events(&self) -> Result<Box<dyn super::EventSource>> {
        Ok(Box::new(Socket2(self.instance.events()?)))
    }

    fn compat_check(&self) -> Result<()> {
        // A Hyprland newer than any tested one may have changed the IPC or
        // the config API under us.
        if let Ok(v) = self.version()
            && crate::doctor::newer_than_tested(&v)
        {
            return Err(Error::new(
                "untested_version",
                format!(
                    "Hyprland {v} is newer than the versions stance was tested with ({}); \
                     check for a stance update, or pass --force-version (STANCE_FORCE_VERSION=1 for the daemon) to try anyway",
                    crate::doctor::TESTED.join(", ")
                ),
            ));
        }
        Ok(())
    }

    fn version(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct V {
            version: String,
        }
        Ok(self.instance.request_json::<V>("version")?.version)
    }

    /// Lazily detected and cached per process. `status` names the config
    /// provider (0.56+). Older builds: `eval return 1` has no side effects;
    /// Lua answers `ok`, hyprlang rejects the request.
    fn parser(&self) -> Result<Parser> {
        if let Some(p) = self.parser.get() {
            return Ok(*p);
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Status {
            config_provider: String,
        }
        let p = match self.instance.request_json::<Status>("status") {
            Ok(s) if s.config_provider == "lua" => Parser::Lua,
            Ok(_) => Parser::Legacy,
            Err(_) => {
                let reply = self.instance.request("eval return 1")?;
                if reply.trim() == "ok" {
                    Parser::Lua
                } else {
                    Parser::Legacy
                }
            }
        };
        Ok(*self.parser.get_or_init(|| p))
    }

    fn monitors(&self) -> Result<Vec<Monitor>> {
        let raw: Vec<RawMonitor> = self.instance.request_json("monitors all")?;
        Ok(convert(raw))
    }

    fn apply_monitor(&self, spec: &MonitorSpec) -> Result<()> {
        match self.parser()? {
            Parser::Lua => self.eval(&spec.to_lua()?),
            Parser::Legacy => self.keyword("monitor", &spec.to_legacy()?),
        }
    }

    fn get_option(&self, key: OptKey) -> Result<OptValue> {
        let v: serde_json::Value = self
            .instance
            .request_json(&format!("getoption {}", path(key)))?;
        parse_option(key, &v)
    }

    fn set_option(&self, key: OptKey, value: OptValue) -> Result<()> {
        value.check(key)?;
        match self.parser()? {
            Parser::Lua => self.eval(&lua_opt(key, value)),
            Parser::Legacy => self.keyword(path(key), &legacy_opt(value)),
        }
    }

    fn workspaces(&self) -> Result<Vec<(u32, String)>> {
        #[derive(Deserialize)]
        struct W {
            id: i64,
            monitor: String,
        }
        let ws: Vec<W> = self.instance.request_json("workspaces")?;
        Ok(ws
            .into_iter()
            .filter_map(|w| Some((u32::try_from(w.id).ok().filter(|i| *i > 0)?, w.monitor)))
            .collect())
    }

    /// Adds a workspace rule (new workspaces open there) and moves the
    /// workspace now if it already exists.
    fn bind_workspace(&self, id: u32, monitor: &str) -> Result<()> {
        check_connector(monitor)?;
        let exists = self.workspaces()?.iter().any(|(w, _)| *w == id);
        match self.parser()? {
            Parser::Lua => {
                self.eval(&format!(
                    "hl.workspace_rule({{ workspace = \"{id}\", monitor = \"{monitor}\" }})"
                ))?;
                if exists {
                    self.eval(&format!(
                        "hl.dispatch(hl.dsp.workspace.move({{ workspace = {id}, monitor = \"{monitor}\" }}))"
                    ))?;
                }
            }
            Parser::Legacy => {
                self.keyword("workspace", &format!("{id},monitor:{monitor}"))?;
                if exists {
                    self.dispatch_legacy(&format!("moveworkspacetomonitor {id} {monitor}"))?;
                }
            }
        }
        Ok(())
    }

    fn notify(&self, notice: Notice) {
        // icon 0 = warning, 6 s, default color. Text is a fixed constant.
        let _ = self
            .instance
            .request(&format!("notify 0 6000 0 {}", notice.text()));
    }
}

/// Subset of `monitors -j` we rely on. Unknown fields are ignored on
/// purpose: Hyprland adds fields between releases.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawMonitor {
    #[serde(default)]
    id: i64,
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
    /// Monitor id (as a string) or "none".
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

pub(crate) fn convert(raw: Vec<RawMonitor>) -> Vec<Monitor> {
    let by_id: HashMap<String, String> = raw
        .iter()
        .map(|r| (r.id.to_string(), r.name.clone()))
        .collect();
    raw.into_iter()
        .map(|r| {
            let mirror_of = r
                .mirror_of
                .filter(|m| !m.is_empty() && m != "none")
                .map(|m| by_id.get(&m).cloned().unwrap_or(m))
                .filter(|m| crate::spec::valid_connector(m));
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
                mirror_of,
                modes: r
                    .available_modes
                    .iter()
                    .filter_map(|m| Mode::parse(m))
                    .collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_0_57_reply() {
        let raw: Vec<RawMonitor> =
            serde_json::from_str(include_str!("../../tests/fixtures/monitors-0.57.1.json"))
                .unwrap();
        let mons = convert(raw);
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

    #[test]
    fn mirror_id_maps_to_name() {
        let j = r#"[{"id":0,"name":"A-1","width":1,"height":1,"refreshRate":60,"x":0,"y":0,"scale":1,"transform":0,"mirrorOf":"none"},
                   {"id":1,"name":"B-1","width":1,"height":1,"refreshRate":60,"x":0,"y":0,"scale":1,"transform":0,"mirrorOf":"0"}]"#;
        let mons = convert(serde_json::from_str(j).unwrap());
        assert_eq!(mons[1].mirror_of.as_deref(), Some("A-1"));
        assert_eq!(mons[0].mirror_of, None);
    }

    #[test]
    fn option_parsing() {
        let v = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();
        assert_eq!(
            parse_option(OptKey::Blur, &v(r#"{"bool":false}"#)).unwrap(),
            OptValue::Bool(false)
        );
        assert_eq!(
            parse_option(OptKey::Blur, &v(r#"{"int":1}"#)).unwrap(),
            OptValue::Bool(true)
        );
        assert_eq!(
            parse_option(OptKey::GapsOut, &v(r#"{"css":"1 2 3 4"}"#)).unwrap(),
            OptValue::Gaps([1, 2, 3, 4])
        );
        assert_eq!(
            parse_option(OptKey::GapsIn, &v(r#"{"int":5}"#)).unwrap(),
            OptValue::Gaps([5; 4])
        );
        assert!(parse_option(OptKey::GapsIn, &v(r#"{"css":"1 2"}"#)).is_err());
    }

    #[test]
    fn option_rendering() {
        assert_eq!(
            lua_opt(OptKey::GapsOut, OptValue::Gaps([1, 2, 3, 4])),
            "hl.config({ general = { gaps_out = { top = 1, right = 2, bottom = 3, left = 4 } } })"
        );
        assert_eq!(
            lua_opt(OptKey::Blur, OptValue::Bool(false)),
            "hl.config({ decoration = { blur = { enabled = false } } })"
        );
        assert_eq!(legacy_opt(OptValue::Bool(true)), "1");
    }

    #[test]
    fn translates_events() {
        let added = |n: &str| Next::Event(Event::MonitorAdded { name: n.into() });
        assert_eq!(
            parse_event("monitoraddedv2>>2,DP-2,JDR EPIC Q40"),
            added("DP-2")
        );
        assert_eq!(parse_event("monitoradded>>HDMI-A-1"), added("HDMI-A-1"));
        assert_eq!(parse_event("monitoradded>>../x"), added(""));
        assert_eq!(
            parse_event("monitorremoved>>DP-2"),
            Next::Event(Event::MonitorRemoved {
                name: "DP-2".into()
            })
        );
        assert_eq!(
            parse_event("configreloaded>>"),
            Next::Event(Event::ConfigReloaded)
        );
        assert_eq!(parse_event("workspace>>2"), Next::Tick);
        assert_eq!(parse_event("monitoraddedfake>>x"), Next::Tick);
    }
}
