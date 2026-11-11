//! `stance doctor`: read-only environment checks. Never changes anything.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::backend::{Backend, Hyprland};
use crate::hypr::{Instance, check_private_dir, current_uid};
use crate::model::Parser;

/// Hyprland releases stance has been tested against.
pub const TESTED: &[&str] = &["0.55", "0.56", "0.57"];

/// Is `version` (e.g. "0.57.1") newer than every tested major.minor?
/// Older or unparsable versions are not "newer": the parser checks catch
/// what really matters there.
pub fn newer_than_tested(version: &str) -> bool {
    let mm = |s: &str| -> Option<(u32, u32)> {
        let mut it = s.trim().trim_start_matches('v').split('.');
        Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
    };
    let newest = TESTED.iter().filter_map(|t| mm(t)).max();
    matches!((mm(version), newest), (Some(v), Some(n)) if v > n)
}
const PERSIST_NAME: &str = "stance-monitors";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Info,
    Warn,
    Fail,
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub id: &'static str,
    pub status: Status,
    pub message: String,
}

fn check(id: &'static str, status: Status, message: impl Into<String>) -> Check {
    Check {
        id,
        status,
        message: message.into(),
    }
}

pub fn run() -> Vec<Check> {
    let mut out = Vec::new();

    let instance = match Instance::from_env() {
        Ok(i) => {
            out.push(check(
                "instance",
                Status::Ok,
                format!("{} instance {}", hypr_name(), i.signature),
            ));
            i
        }
        Err(e) => {
            out.push(check("instance", Status::Fail, e.message));
            return out;
        }
    };

    for (id, path) in [
        ("socket1", instance.socket1()),
        ("socket2", instance.socket2()),
    ] {
        match instance.check_socket(&path) {
            Ok(()) => out.push(check(
                id,
                Status::Ok,
                format!("{} (owned by you, private dir)", path.display()),
            )),
            Err(e) => out.push(check(id, Status::Fail, e.message)),
        }
    }

    let hypr = Hyprland::new(instance);
    match hypr.version() {
        Ok(v) => {
            let tested = TESTED
                .iter()
                .any(|t| v == *t || v.starts_with(&format!("{t}.")));
            let status = if tested { Status::Ok } else { Status::Warn };
            let note = if tested {
                ""
            } else {
                " (untested version, please report issues)"
            };
            out.push(check("version", status, format!("Hyprland {v}{note}")));
        }
        Err(e) => {
            out.push(check(
                "version",
                Status::Fail,
                format!("cannot talk to Hyprland: {}", e.message),
            ));
            return out;
        }
    }

    let parser = match hypr.parser() {
        Ok(p) => {
            let msg = match p {
                Parser::Lua => "Lua config: runtime changes via eval",
                Parser::Legacy => "legacy hyprlang config: runtime changes via keyword",
            };
            out.push(check("parser", Status::Ok, msg));
            Some(p)
        }
        Err(e) => {
            out.push(check("parser", Status::Fail, e.message));
            None
        }
    };

    match hypr.monitors() {
        Ok(m) => {
            let names: Vec<_> = m.iter().map(|m| m.name.as_str()).collect();
            out.push(check(
                "monitors",
                Status::Ok,
                format!("{} monitor(s): {}", m.len(), names.join(", ")),
            ));
            for mon in m.iter().filter(|m| m.serial.is_empty()) {
                out.push(check(
                    "serial",
                    Status::Info,
                    format!(
                        "{} reports no serial; matching uses description/make+model",
                        mon.name
                    ),
                ));
            }
        }
        Err(e) => out.push(check("monitors", Status::Fail, e.message)),
    }

    if let Some(p) = parser {
        out.extend(config_checks(&hypr_config_dir(), p));
    }
    out.extend(dir_checks());
    out
}

fn hypr_name() -> &'static str {
    // Instance::from_env only succeeds for Hyprland today.
    "Hyprland"
}

fn hypr_config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_default()
        .join("hypr")
}

/// True if any non-comment line of `text` loads the persist file.
fn loads_persist(text: &str, parser: Parser) -> bool {
    text.lines().map(str::trim).any(|l| match parser {
        Parser::Lua => {
            !l.starts_with("--")
                && (l.contains(&format!("require(\"{PERSIST_NAME}\")"))
                    || l.contains(&format!("require('{PERSIST_NAME}')")))
        }
        Parser::Legacy => {
            !l.starts_with('#')
                && l.starts_with("source")
                && l.contains(&format!("{PERSIST_NAME}.conf"))
        }
    })
}

/// Line index of the first non-comment line matching `needle`.
fn line_of(text: &str, needle: &str, comment: &str) -> Option<usize> {
    text.lines().position(|l| {
        let l = l.trim();
        !l.starts_with(comment) && l.contains(needle)
    })
}

fn config_checks(dir: &Path, parser: Parser) -> Vec<Check> {
    let mut out = Vec::new();
    let (main, ext, comment) = match parser {
        Parser::Lua => (dir.join("hyprland.lua"), "lua", "--"),
        Parser::Legacy => (dir.join("hyprland.conf"), "conf", "#"),
    };
    let Ok(text) = fs::read_to_string(&main) else {
        out.push(check(
            "config",
            Status::Warn,
            format!("cannot read {}", main.display()),
        ));
        return out;
    };
    out.push(check(
        "config",
        Status::Ok,
        format!("main config {}", main.display()),
    ));

    let persist = dir.join(format!("{PERSIST_NAME}.{ext}"));
    let loaded = loads_persist(&text, parser);
    let line = match parser {
        Parser::Lua => format!("require(\"{PERSIST_NAME}\")"),
        Parser::Legacy => format!("source = ~/.config/hypr/{PERSIST_NAME}.conf"),
    };
    match (persist.exists(), loaded) {
        (_, true) => out.push(check(
            "persist",
            Status::Ok,
            format!("{} is loaded by your config", persist.display()),
        )),
        (true, false) => out.push(check(
            "persist",
            Status::Warn,
            format!(
                "{} exists but is not loaded; add `{line}` to {}",
                persist.display(),
                main.display()
            ),
        )),
        (false, false) => out.push(check(
            "persist",
            Status::Info,
            format!(
                "not set up yet; `stance persist` will write {} and you add `{line}` once",
                persist.display()
            ),
        )),
    }

    // nwg-displays writes monitors.{lua,conf}; whichever loads last wins.
    let nwg = dir.join(format!("monitors.{ext}"));
    let is_nwg = fs::read_to_string(&nwg)
        .is_ok_and(|t| t.lines().next().is_some_and(|l| l.contains("nwg-displays")));
    if is_nwg {
        let nwg_needle = match parser {
            Parser::Lua => "require(\"monitors\")",
            Parser::Legacy => "monitors.conf",
        };
        let nwg_at = line_of(&text, nwg_needle, comment);
        let stance_at = line_of(&text, PERSIST_NAME, comment);
        let msg = match (nwg_at, stance_at) {
            (Some(n), Some(s)) if s > n => {
                "nwg-displays config found; stance loads after it, so stance wins".to_string()
            }
            (Some(_), Some(_)) => {
                "nwg-displays config loads after stance and will override persisted settings"
                    .to_string()
            }
            _ => format!(
                "nwg-displays config {} found; load stance after it so persisted settings win",
                nwg.display()
            ),
        };
        let status = if matches!((nwg_at, stance_at), (Some(n), Some(s)) if s < n) {
            Status::Warn
        } else {
            Status::Info
        };
        out.push(check("nwg_displays", status, msg));
    }
    out
}

fn dir_checks() -> Vec<Check> {
    let mut out = Vec::new();
    let Ok(uid) = current_uid() else { return out };
    if let Some(rt) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        let status = match check_private_dir(&rt, uid) {
            Ok(()) if fs::metadata(&rt).is_ok_and(|m| m.permissions().mode() & 0o077 == 0) => {
                Status::Ok
            }
            Ok(()) => Status::Warn,
            Err(_) => Status::Fail,
        };
        out.push(check("runtime_dir", status, format!("{}", rt.display())));
        let own = rt.join("stance");
        if own.exists() {
            match check_private_dir(&own, uid) {
                Ok(()) => out.push(check(
                    "stance_runtime",
                    Status::Ok,
                    format!("{}", own.display()),
                )),
                Err(e) => out.push(check("stance_runtime", Status::Fail, e.message)),
            }
        }
    }
    let pp = crate::power::available();
    out.push(check(
        "power_profiles",
        if pp { Status::Ok } else { Status::Info },
        if pp {
            "powerprofilesctl found (power profile in modes available)"
        } else {
            "powerprofilesctl not found; modes cannot switch power profile"
        },
    ));
    let tool_check = |id, found: bool, yes: &str, no: &str| {
        check(
            id,
            if found { Status::Ok } else { Status::Info },
            if found { yes } else { no },
        )
    };
    out.push(tool_check(
        "night_light",
        crate::tool::find("hyprsunset").is_some(),
        "hyprsunset found (night light available)",
        "hyprsunset not found; night light unavailable",
    ));
    out.push(tool_check(
        "audio",
        crate::audio::available(),
        "pw-dump and wpctl found (audio output switching available)",
        "pw-dump / wpctl not found; audio output switching unavailable",
    ));
    let ddc = crate::tool::find("ddcutil").is_some();
    let bl = crate::tool::find("brightnessctl").is_some();
    out.push(check(
        "brightness",
        if crate::brightness::available() {
            Status::Ok
        } else {
            Status::Info
        },
        match (bl, ddc) {
            (true, true) => "brightnessctl (laptop panel) and ddcutil (external, DDC/CI) found",
            (true, false) => "brightnessctl found; install ddcutil for external screens",
            (false, true) => "ddcutil found; install brightnessctl for laptop panels",
            (false, false) => "brightnessctl / ddcutil not found; brightness unavailable",
        },
    ));
    out.push(deck_check());
    if let Ok(dir) = crate::store::state_dir() {
        out.push(clip_check(&dir));
    }
    out.extend(shell_checks());
    if ddc && !Path::new("/dev/i2c-0").exists() {
        out.push(check(
            "i2c",
            Status::Info,
            "no /dev/i2c-* devices: load the i2c-dev module for DDC/CI brightness",
        ));
    }
    out
}

/// end-4 illogical-impulse style shells: is the stance button installed?
/// Prints the exact commands when not; never edits anything itself.
fn shell_checks() -> Vec<Check> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return vec![];
    };
    let qs = home.join(".config/quickshell");
    let Ok(rd) = fs::read_dir(&qs) else {
        return vec![];
    };
    // Package layout vs. a source checkout.
    let (comp, integ) = if Path::new("/usr/share/stance/quickshell").is_dir() {
        (
            "/usr/share/stance/quickshell",
            "/usr/share/stance/integrations",
        )
    } else {
        (
            "<stance checkout>/quickshell/stance",
            "<stance checkout>/quickshell/integrations",
        )
    };
    let mut out = Vec::new();
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    dirs.sort();
    // Only talk about the shell config that is actually running, if any.
    let running = running_qs_configs();
    if !running.is_empty() {
        dirs.retain(|d| {
            d.file_name()
                .is_some_and(|n| running.iter().any(|r| r == &n.to_string_lossy()))
        });
    }
    for dir in dirs {
        let bar = dir.join("modules/ii/bar");
        let util = bar.join("UtilButtons.qml");
        let Ok(text) = fs::read_to_string(&util) else {
            continue;
        };
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let flavor = if name.contains("end4") || text.contains("Flow {") {
            "end4-pC"
        } else {
            "ii"
        };
        let linked =
            dir.join("modules/stance").exists() && bar.join("StanceUtilButton.qml").exists();
        let used = text.contains("StanceUtilButton");
        if linked && used {
            out.push(check(
                "shell",
                Status::Ok,
                format!("{name}: stance button installed"),
            ));
            continue;
        }
        let line = if flavor == "end4-pC" {
            "StanceUtilButton { contentColor: root.contentColor }"
        } else {
            "StanceUtilButton {}"
        };
        let mut steps = Vec::new();
        if !dir.join("modules/stance").exists() {
            steps.push(format!("ln -s {comp} {}/modules/stance", dir.display()));
        }
        if !bar.join("StanceUtilButton.qml").exists() {
            steps.push(format!(
                "ln -s {integ}/{flavor}/StanceUtilButton.qml {}/",
                bar.display()
            ));
        }
        if !used {
            steps.push(format!(
                "add `{line}` as the first child in {}",
                util.display()
            ));
        }
        out.push(check(
            "shell",
            Status::Info,
            format!(
                "{name} ({flavor}) has no stance button yet: {}",
                steps.join("; ")
            ),
        ));
    }
    out
}

/// Command lines of running processes (read from /proc, nothing is executed).
pub fn cmdlines() -> Vec<Vec<String>> {
    let Ok(rd) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    rd.flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .bytes()
                .all(|b| b.is_ascii_digit())
        })
        .filter_map(|e| fs::read(e.path().join("cmdline")).ok())
        .map(|raw| {
            raw.split(|b| *b == 0)
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect()
        })
        .collect()
}

/// Clipboard shelf: enabled, both watchers running, and the cliphist caveat.
pub fn clip_check(state_dir: &Path) -> Check {
    if !crate::feature::is_on("clip") {
        return check(
            "clip",
            Status::Info,
            "clipboard shelf turned off (stance feature on clip)",
        );
    }
    let enabled = crate::clip::Clip::open(state_dir)
        .and_then(|c| c.config())
        .map(|c| c.enabled);
    let procs = cmdlines();
    let base = |a: &String| a.rsplit('/').next().unwrap_or("").to_string();
    let watching = |kind: &str| {
        procs.iter().any(|p| {
            p.first().is_some_and(|a| base(a) == "wl-paste")
                && p.iter().any(|a| a == kind)
                && p.iter().any(|a| base(a) == "stance")
        })
    };
    let cliphist = procs.iter().any(|p| {
        p.iter().any(|a| a.contains("cliphist store"))
            || p.first().is_some_and(|a| base(a) == "cliphist")
    });
    let note = if cliphist {
        "; cliphist also runs and stores what stance skips (password-manager copies)"
    } else {
        ""
    };
    if let Ok(c) = crate::clip::Clip::open(state_dir)
        && let Ok(rd) = std::fs::read_dir(&c.dir)
        && let Some(d) = rd.flatten().find(|d| {
            d.file_name()
                .to_string_lossy()
                .starts_with(crate::clip::CORRUPT)
        })
    {
        return check(
            "clip",
            Status::Warn,
            format!(
                "a damaged clipboard index was set aside with its content in {}; delete that folder once you no longer need it",
                d.path().display()
            ),
        );
    }
    match enabled {
        Err(e) => check("clip", Status::Warn, format!("clip config: {}", e.message)),
        Ok(false) => check(
            "clip",
            Status::Info,
            "clipboard shelf off (optional: stance clip config put --data '{\"enabled\":true}' and systemctl --user enable --now stance-clip-text stance-clip-image)",
        ),
        Ok(true)
            if crate::clip::Clip::open(state_dir)
                .and_then(|c| c.config())
                .is_ok_and(|c| c.paused(crate::clip::now()).is_some()) =>
        {
            check(
                "clip",
                Status::Info,
                format!("clipboard shelf paused (stance clip resume){note}"),
            )
        }
        Ok(true) => match (watching("text"), watching("image")) {
            (true, true) => check(
                "clip",
                Status::Ok,
                format!("clipboard shelf on, text and image capture running{note}"),
            ),
            (t, i) => check(
                "clip",
                Status::Warn,
                format!(
                    "clipboard shelf on but {} capture is not running: systemctl --user enable --now {} (the units need WAYLAND_DISPLAY in the systemd user environment: dbus-update-activation-environment --systemd WAYLAND_DISPLAY){note}",
                    if t {
                        "image"
                    } else if i {
                        "text"
                    } else {
                        "text and image"
                    },
                    if t {
                        "stance-clip-image"
                    } else if i {
                        "stance-clip-text"
                    } else {
                        "stance-clip-text stance-clip-image"
                    },
                ),
            ),
        },
    }
}

/// Names passed to `qs -c NAME` / `quickshell -c NAME` by running
/// processes of this user.
fn running_qs_configs() -> Vec<String> {
    let mut out = Vec::new();
    for args in cmdlines() {
        let is_qs = args.first().is_some_and(|a| {
            a.rsplit('/')
                .next()
                .is_some_and(|b| b == "qs" || b == "quickshell")
        });
        if !is_qs {
            continue;
        }
        if let Some(i) = args.iter().position(|a| a == "-c" || a == "--config")
            && let Some(name) = args.get(i + 1)
            && !out.contains(name)
        {
            out.push(name.clone());
        }
    }
    out
}

/// Does a ufw `user.rules` file accept TCP on `port`? Looks for the
/// `### tuple ### allow tcp <port> …` comments ufw writes for each rule.
pub fn ufw_allows(rules: &str, port: u64) -> bool {
    rules.lines().any(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        f.len() >= 6
            && f[0] == "###"
            && f[1] == "tuple"
            && f[3] == "allow"
            && (f[4] == "tcp" || f[4] == "any")
            && f[5].split(',').any(|p| {
                p == port.to_string()
                    || p.split_once(':').is_some_and(|(a, b)| {
                        a.parse::<u64>().is_ok_and(|a| a <= port)
                            && b.parse::<u64>().is_ok_and(|b| port <= b)
                    })
            })
    })
}

/// Phone deck: is the server running, and would a firewall drop phones?
fn deck_check() -> Check {
    let port = crate::deck::pair::DEFAULT_PORT;
    let ufw_on = fs::read_to_string("/etc/ufw/ufw.conf")
        .is_ok_and(|t| t.lines().any(|l| l.trim() == "ENABLED=yes"));
    match crate::deck::net::ask("status") {
        Ok(s) => {
            let port = s["port"].as_u64().unwrap_or(u64::from(port));
            let n = s["devices"].as_array().map_or(0, |v| v.len());
            // user.rules is world-readable on Arch; when it already allows
            // the port, say nothing. Unreadable: remind, but only as info.
            let rules = fs::read_to_string("/etc/ufw/user.rules").ok();
            let allowed = rules.as_deref().is_some_and(|r| ufw_allows(r, port));
            if ufw_on && s["loopback"] != true && !allowed {
                check(
                    "deck",
                    if rules.is_some() {
                        Status::Warn
                    } else {
                        Status::Info
                    },
                    format!(
                        "deck server on port {port}, {n} paired phone(s); ufw is enabled, allow your LAN: sudo ufw allow from 192.168.0.0/16 to any port {port} proto tcp"
                    ),
                )
            } else {
                check(
                    "deck",
                    Status::Ok,
                    format!("deck server on port {port}, {n} paired phone(s)"),
                )
            }
        }
        Err(_) => check(
            "deck",
            Status::Info,
            "phone deck server not running (optional: stance deck serve, or contrib/stance-deck.service)",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ufw_rule_detection() {
        let r = "### tuple ### allow tcp 47800 0.0.0.0/0 any 192.168.0.0/24 in\n-A ufw-user-input -p tcp --dport 47800 -s 192.168.0.0/24 -j ACCEPT\n";
        assert!(ufw_allows(r, 47800));
        assert!(!ufw_allows(r, 47801));
        assert!(ufw_allows(
            "### tuple ### allow tcp 47000:48000 0.0.0.0/0 any 0.0.0.0/0 in",
            47800
        ));
        assert!(!ufw_allows(
            "### tuple ### deny tcp 47800 0.0.0.0/0 any 0.0.0.0/0 in",
            47800
        ));
    }

    #[test]
    fn version_guard() {
        assert!(!newer_than_tested("0.57.1"));
        assert!(!newer_than_tested("0.55.0"));
        assert!(!newer_than_tested("0.49.0"));
        assert!(newer_than_tested("0.58.0"));
        assert!(newer_than_tested("1.0.0"));
        assert!(!newer_than_tested("git"));
    }

    #[test]
    fn detects_lua_require() {
        assert!(loads_persist(
            "require(\"monitors\")\nrequire(\"stance-monitors\")",
            Parser::Lua
        ));
        assert!(loads_persist("  require('stance-monitors')", Parser::Lua));
        assert!(!loads_persist(
            "-- require(\"stance-monitors\")",
            Parser::Lua
        ));
        assert!(!loads_persist("require(\"monitors\")", Parser::Lua));
    }

    #[test]
    fn detects_legacy_source() {
        assert!(loads_persist(
            "source = ~/.config/hypr/stance-monitors.conf",
            Parser::Legacy
        ));
        assert!(!loads_persist(
            "# source = ~/.config/hypr/stance-monitors.conf",
            Parser::Legacy
        ));
    }

    #[test]
    fn nwg_order() {
        let dir = std::env::temp_dir().join(format!("stance-doctor-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("monitors.lua"), "-- Generated by nwg-displays\n").unwrap();
        fs::write(
            dir.join("hyprland.lua"),
            "require(\"stance-monitors\")\nrequire(\"monitors\")\n",
        )
        .unwrap();
        let c = config_checks(&dir, Parser::Lua);
        let nwg = c.iter().find(|c| c.id == "nwg_displays").unwrap();
        assert_eq!(nwg.status, Status::Warn);
        fs::write(
            dir.join("hyprland.lua"),
            "require(\"monitors\")\nrequire(\"stance-monitors\")\n",
        )
        .unwrap();
        let c = config_checks(&dir, Parser::Lua);
        assert_eq!(
            c.iter().find(|c| c.id == "nwg_displays").unwrap().status,
            Status::Info
        );
        assert_eq!(
            c.iter().find(|c| c.id == "persist").unwrap().status,
            Status::Ok
        );
        fs::remove_dir_all(&dir).ok();
    }
}
