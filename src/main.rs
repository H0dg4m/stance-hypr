mod apply;
mod audio;
mod backend;
mod brightness;
mod clip;
mod daemon;
mod deck;
mod doctor;
mod error;
mod feature;
mod hypr;
mod memory;
mod migrate;
mod model;
mod modes;
mod night;
mod persist;
mod plan;
mod power;
mod setup;
mod spec;
mod store;
mod theme;
mod tool;
mod uevent;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use serde::Serialize;
use serde_json::json;

use apply::{Ctx, DEFAULT_TIMEOUT, Outcome, State};
use backend::{Backend, Hyprland};
use error::{Error, Result};
use model::Monitor;
use plan::{Layout, SetArgs};

const USAGE: &str = "\
stance — bar-first display settings and modes for Hyprland

Usage:
  stance monitors                      List monitors
  stance status                        Pending change and active mode
  stance set <monitor> [options]       Change one monitor
      --res WxH  --hz N  --scale S  --pos XxY  --vrr 0|1|2
      --transform 0-7  --on | --off  --custom (allow unadvertised modes)
  stance layout <kind> <monitor> [<reference>]
      kind: left-of | right-of | above | below | mirror | single
  stance arrange NAME=XxY [NAME=XxY…]  Move several monitors at once (panel drag)
  stance lid close | open             Laptop lid (bind to Hyprland's lid switch; no confirm timer)
  stance power                         AC or battery, and the hotplug rule that applies
  stance waybar                        One status line for a Waybar custom module
  stance confirm                       Keep the last change
  stance revert                        Undo the last change now
  stance persist [--print]             Save the current setup to your Hyprland config dir
  stance mode list
  stance mode show <name>
  stance mode apply <name> [--no-extras]
  stance mode save <name> [--description TEXT] [--force] [--with-quick]
      --with-quick also records night light and audio output
  stance mode put <name> --data JSON [--force]   Write a mode from JSON (panel editor)
  stance mode rm <name>
  stance rules [show]                  Hotplug rules as JSON
  stance rules put --data JSON         Replace the hotplug rules
  stance night [status|on [K]|off|toggle|set K]   Night light (hyprsunset)
  stance brightness [list]             Brightness of screens (backlight, DDC/CI)
  stance brightness <monitor> N|+N|-N  Set brightness in percent
  stance audio [list]                  Audio outputs
  stance audio set <sink>              Default output (id, node name or unique text)
  stance quick                         Night light, brightness and audio at once
  stance deck serve [--port N] [--loopback]   Phone deck server (Wi-Fi; --loopback for USB/adb)
  stance deck pair                     Show a pairing QR (valid 120 s, one phone)
  stance deck status | devices | unpair NAME   Server state, paired phones
  stance deck buttons | put --data JSON | reset   The phone's buttons (deck.toml)
  stance deck test …                   Reference client (pair PAYLOAD, buttons, press ID)
  stance clip list [--kind K] [--query Q] [--limit N]   Clipboard shelf (text, link, image)
  stance clip copy | get | pin | unpin | rm ID…   stance clip undo | prune | clear [--all]
  stance clip config [show | put --data JSON]   enabled, text, image, retention_days
  stance clip purge                    Delete removed items now (otherwise kept a day for undo)
  stance clip pause [15m|1h|30s] | resume   Stop capturing for a while (at most 24h)
  stance clip ingest [--image]         Store one copy (run by `wl-paste --watch`)
  stance theme list | show ID          Panel themes (built-in and yours)
  stance theme put --data JSON | import FILE | export ID [--out FILE] | rm ID   [--force]
  stance feature list | on NAME | off NAME   Optional parts: clip, deck, rules, quick
  stance daemon                        Hotplug rules (optional, long-running)
  stance doctor [--report]             Check the environment (read-only); --report = block for bug reports
  stance setup [waybar|quickshell|end4] [--dry-run]   How to wire stance into your desktop

On a Hyprland newer than the tested versions, commands that change screens
refuse to run unless --force-version is given.

<monitor> is a description (\"JDR EPIC Q40\"), \"make model\" or a connector (DP-1).
Changes revert after 12 s unless confirmed: --timeout S to change, --no-confirm to skip.
Every command accepts --json.";

const VALUE_FLAGS: &[&str] = &[
    "--res",
    "--hz",
    "--scale",
    "--pos",
    "--vrr",
    "--transform",
    "--timeout",
    "--description",
    "--data",
    "--port",
    "--name",
    "--kind",
    "--query",
    "--limit",
    "--out",
];
const BOOL_FLAGS: &[&str] = &[
    "--json",
    "--on",
    "--off",
    "--custom",
    "--no-confirm",
    "--print",
    "--force",
    "--no-extras",
    "--with-quick",
    "--loopback",
    "--force-version",
    "--image",
    "--dry-run",
    "--report",
    "--all",
    "--help",
    "-h",
    "--version",
    "-V",
];

struct Args {
    pos: Vec<String>,
    flags: HashMap<String, Option<String>>,
}

impl Args {
    fn parse(raw: Vec<String>) -> Result<Self> {
        let mut pos = Vec::new();
        let mut flags = HashMap::new();
        let mut it = raw.into_iter();
        while let Some(a) = it.next() {
            if let Some((k, v)) = a.split_once('=').filter(|(k, _)| VALUE_FLAGS.contains(k)) {
                flags.insert(k.to_string(), Some(v.to_string()));
            } else if VALUE_FLAGS.contains(&a.as_str()) {
                let v = it
                    .next()
                    .ok_or_else(|| Error::new("usage", format!("{a} needs a value")))?;
                flags.insert(a, Some(v));
            } else if BOOL_FLAGS.contains(&a.as_str()) {
                flags.insert(a, None);
            } else if a.starts_with("--") {
                return Err(Error::new("usage", format!("unknown option `{a}`")));
            } else {
                pos.push(a);
            }
        }
        Ok(Self { pos, flags })
    }

    fn has(&self, f: &str) -> bool {
        self.flags.contains_key(f)
    }

    fn val(&self, f: &str) -> Option<&str> {
        self.flags.get(f).and_then(|v| v.as_deref())
    }

    fn num<T: std::str::FromStr>(&self, f: &str) -> Result<Option<T>> {
        self.val(f)
            .map(|v| {
                v.parse()
                    .map_err(|_| Error::new("usage", format!("{f}: `{v}` is not a valid number")))
            })
            .transpose()
    }

    fn arg(&self, i: usize, what: &str) -> Result<&str> {
        self.pos
            .get(i)
            .map(String::as_str)
            .ok_or_else(|| Error::new("usage", format!("missing {what}; see `stance --help`")))
    }

    fn max_pos(&self, n: usize) -> Result<()> {
        match self.pos.get(n) {
            Some(a) => Err(Error::new("usage", format!("unexpected argument `{a}`"))),
            None => Ok(()),
        }
    }

    fn timeout(&self) -> Result<u64> {
        if self.has("--no-confirm") {
            return Ok(0);
        }
        let t = self.num::<u64>("--timeout")?.unwrap_or(DEFAULT_TIMEOUT);
        if t > 600 {
            return Err(Error::new("usage", "--timeout is at most 600 seconds"));
        }
        Ok(t)
    }
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // `stance theme list | head` should end quietly like other CLI tools,
    // not panic. Long-running servers keep Rust's default (ignored SIGPIPE).
    if !matches!(
        raw.first().map(String::as_str),
        Some("deck" | "daemon" | "guardian")
    ) {
        // SAFETY: called before any thread starts; restores the default action.
        unsafe {
            libc::signal(libc::SIGPIPE, libc::SIG_DFL);
        }
    }
    let json = raw.iter().any(|a| a == "--json");
    let res = Args::parse(raw).and_then(|a| run(&a));
    match res {
        Ok(code) => code,
        Err(e) => {
            if json {
                print_json(
                    &json!({ "ok": false, "schema": SCHEMA, "error": { "code": e.code, "message": e.message } }),
                );
            } else {
                eprintln!("stance: {}", e.message);
            }
            ExitCode::from(2)
        }
    }
}

fn state_dir() -> Result<PathBuf> {
    match std::env::var_os("STANCE_STATE_DIR").map(PathBuf::from) {
        Some(p) if p.is_absolute() => Ok(p),
        _ => store::state_dir(),
    }
}

fn run(a: &Args) -> Result<ExitCode> {
    let json = a.has("--json");
    if a.has("--version") || a.has("-V") {
        println!("stance {}", env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }
    let cmd = a.pos.first().map(String::as_str);
    if a.has("--help") || a.has("-h") || matches!(cmd, None | Some("help")) {
        println!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }
    let cmd = cmd.unwrap_or_default();

    if cmd == "doctor" {
        a.max_pos(1)?;
        if a.has("--report") {
            print!("{}", bug_report());
            return Ok(ExitCode::SUCCESS);
        }
        return Ok(doctor_cmd(json));
    }
    if cmd == "setup" {
        a.max_pos(2)?;
        return setup_cmd(a, json).map(|_| ExitCode::SUCCESS);
    }

    if cmd == "deck" {
        return deck_cmd(a, json).map(|_| ExitCode::SUCCESS);
    }
    if cmd == "rules" {
        return rules_cmd(a, json).map(|_| ExitCode::SUCCESS);
    }
    if cmd == "theme" {
        return theme_cmd(a, json).map(|_| ExitCode::SUCCESS);
    }
    if cmd == "feature" {
        return feature_cmd(a, json).map(|_| ExitCode::SUCCESS);
    }
    if cmd == "clip" {
        return clip_cmd(a, json).map(|_| ExitCode::SUCCESS);
    }
    if cmd == "audio" {
        return audio_cmd(a, json).map(|_| ExitCode::SUCCESS);
    }

    let hypr = Hyprland::connect()?;
    let ctx = Ctx {
        backend: &hypr,
        state_dir: state_dir()?,
    };

    // A Hyprland newer than any tested one may have changed the IPC or the
    // config API under us: refuse to change the screens unless asked to.
    // Reading, confirming and reverting always work.
    let mutating = matches!(cmd, "set" | "layout" | "arrange" | "lid" | "daemon")
        || (cmd == "mode" && a.pos.get(1).map(String::as_str) == Some("apply"));
    if mutating
        && !a.has("--force-version")
        && std::env::var_os("STANCE_FORCE_VERSION").is_none()
        && let Ok(v) = hypr.version()
        && doctor::newer_than_tested(&v)
    {
        return Err(Error::new(
            "untested_version",
            format!(
                "Hyprland {v} is newer than the versions stance was tested with ({}); \
                 check for a stance update, or pass --force-version (STANCE_FORCE_VERSION=1 for the daemon) to try anyway",
                doctor::TESTED.join(", ")
            ),
        ));
    }

    match cmd {
        "monitors" => {
            a.max_pos(1)?;
            let mons = hypr.monitors()?;
            if json {
                ok_json(&mons);
            } else {
                print_monitors(&mons);
            }
        }
        "status" => {
            a.max_pos(1)?;
            let warnings = {
                let _l = ctx.lock()?;
                ctx.sweep()
            };
            let pending = ctx.pending_info();
            let active = ctx.active_mode();
            if json {
                ok_json(&json!({
                    "pending": pending,
                    "active_mode": active,
                    "daemon": daemon::running(&ctx.state_dir),
                    "warnings": warnings
                }));
            } else {
                match &pending {
                    Some(p) => println!(
                        "pending: {} (reverts in {:.0} s unless `stance confirm`)",
                        p.label,
                        p.remaining_ms as f64 / 1000.0
                    ),
                    None => println!("pending: none"),
                }
                println!("active mode: {}", active.as_deref().unwrap_or("none"));
                println!(
                    "daemon: {}",
                    if daemon::running(&ctx.state_dir) {
                        "running"
                    } else {
                        "not running"
                    }
                );
                for w in warnings {
                    println!("note: {w}");
                }
            }
        }
        "set" => {
            let target = a.arg(1, "monitor")?;
            a.max_pos(2)?;
            if a.has("--on") && a.has("--off") {
                return Err(Error::new("usage", "--on and --off together"));
            }
            let args = SetArgs {
                res: a.val("--res").map(plan::parse_res).transpose()?,
                hz: a.num("--hz")?,
                scale: a.num("--scale")?,
                pos: a.val("--pos").map(plan::parse_pos).transpose()?,
                vrr: a.num("--vrr")?,
                transform: a.num("--transform")?,
                on: if a.has("--on") {
                    Some(true)
                } else if a.has("--off") {
                    Some(false)
                } else {
                    None
                },
                custom: a.has("--custom"),
            };
            let mons = hypr.monitors()?;
            let remembered = plan::resolve(&mons, target)
                .ok()
                .filter(|m| !m.enabled)
                .and_then(|m| memory::recall(&ctx.state_dir, m, &mons));
            let specs = plan::plan_set(&mons, target, &args, remembered)?;
            let warnings = plan::check_result(&mons, &specs)?;
            let label = format!("set {}", specs[0].name);
            let st = State {
                monitors: specs,
                ..Default::default()
            };
            let mut out = ctx.apply(&label, &st, a.timeout()?, Some((None, false)))?;
            out.warnings.extend(warnings);
            report(json, out);
        }
        "layout" => {
            let kind_s = a.arg(1, "layout kind")?;
            let kind = Layout::parse(kind_s)
                .ok_or_else(|| Error::new("usage", format!("unknown layout `{kind_s}`")))?;
            let target = a.arg(2, "monitor")?;
            let reference = if kind.needs_reference() {
                a.max_pos(4)?;
                Some(a.arg(3, "reference monitor")?)
            } else {
                a.max_pos(3)?;
                None
            };
            let mons = hypr.monitors()?;
            let specs = plan::plan_layout(&mons, kind, target, reference)?;
            let warnings = plan::check_result(&mons, &specs)?;
            let st = State {
                monitors: specs,
                ..Default::default()
            };
            let mut out = ctx.apply(
                &format!("layout {kind_s}"),
                &st,
                a.timeout()?,
                Some((None, false)),
            )?;
            out.warnings.extend(warnings);
            report(json, out);
        }
        "arrange" => {
            let pairs = a.pos[1..]
                .iter()
                .map(|p| plan::parse_pair(p))
                .collect::<Result<Vec<_>>>()?;
            let mons = hypr.monitors()?;
            let (specs, warnings) = plan::plan_arrange(&mons, &pairs)?;
            let st = State {
                monitors: specs,
                ..Default::default()
            };
            let mut out = ctx.apply("arrange", &st, a.timeout()?, Some((None, false)))?;
            out.warnings.extend(warnings);
            report(json, out);
        }
        "lid" => {
            let what = a.arg(1, "close or open")?;
            a.max_pos(2)?;
            let close = match what {
                "close" => true,
                "open" => false,
                other => {
                    return Err(Error::new(
                        "usage",
                        format!("lid {other}: use close or open"),
                    ));
                }
            };
            let mons = hypr.monitors()?;
            let (specs, note) =
                plan::plan_lid(&mons, close, &|m| memory::recall(&ctx.state_dir, m, &mons))?;
            let st = State {
                monitors: specs,
                ..Default::default()
            };
            // Nobody can confirm with the lid shut: no timer, mode kept.
            let mut out = ctx.apply(&format!("lid {what}"), &st, 0, None)?;
            out.warnings.extend(note);
            report(json, out);
        }
        "power" => {
            a.max_pos(1)?;
            let source = modes::PowerSource::from_ac(power::on_ac());
            let rules = modes::load_rules(&modes::rules_path()?)?;
            let mons = hypr.monitors()?;
            let rule = modes::matching_rule(&rules, &mons, source).map(|r| r.mode.clone());
            if json {
                ok_json(&json!({ "source": source, "rule_mode": rule }));
            } else {
                println!(
                    "power: {}",
                    match source {
                        Some(modes::PowerSource::Ac) => "ac",
                        Some(modes::PowerSource::Battery) => "battery",
                        None => "no battery (power rules never apply)",
                    }
                );
                println!("matching rule: {}", rule.as_deref().unwrap_or("none"));
            }
        }
        "waybar" => {
            a.max_pos(1)?;
            waybar_line(&ctx, &hypr);
        }
        "confirm" => {
            a.max_pos(1)?;
            let had = ctx.confirm()?;
            simple(
                json,
                if had {
                    "confirmed"
                } else {
                    "nothing to confirm"
                },
                had,
            );
        }
        "revert" => {
            a.max_pos(1)?;
            let had = ctx.revert()?;
            simple(
                json,
                if had { "reverted" } else { "nothing to revert" },
                had,
            );
        }
        "persist" => {
            a.max_pos(1)?;
            persist_cmd(&ctx, &hypr, a.has("--print"), json)?;
        }
        "mode" => mode_cmd(a, &ctx, &hypr, json)?,
        "night" => night_cmd(a, &ctx, &hypr, json)?,
        "brightness" => brightness_cmd(a, &hypr, json)?,
        "quick" => {
            a.max_pos(1)?;
            quick_cmd(&ctx, &hypr, json)?;
        }
        "daemon" => {
            a.max_pos(1)?;
            daemon::run(&hypr, ctx.state_dir.clone())?;
        }
        "guardian" => {
            let generation: u64 = a
                .arg(1, "generation")?
                .parse()
                .map_err(|_| Error::new("usage", "bad generation"))?;
            ctx.guard(generation)?;
        }
        other => {
            return Err(Error::new(
                "usage",
                format!("unknown command `{other}`; see `stance --help`"),
            ));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn mode_cmd(a: &Args, ctx: &Ctx, hypr: &Hyprland, json: bool) -> Result<()> {
    match a.arg(1, "mode subcommand (list, show, apply, save, rm)")? {
        "list" => {
            a.max_pos(2)?;
            let list = modes::list()?;
            let active = ctx.active_mode();
            if json {
                ok_json(&json!({ "modes": list, "active": active }));
            } else if list.is_empty() {
                println!("no modes yet; create one with `stance mode save <name>`");
            } else {
                for m in list {
                    let mark = if active.as_deref() == Some(m.name.as_str()) {
                        "*"
                    } else {
                        " "
                    };
                    match (m.error, m.description) {
                        (Some(e), _) => println!("{mark} {:<16} (invalid: {e})", m.name),
                        (None, d) => println!("{mark} {:<16} {}", m.name, d.unwrap_or_default()),
                    }
                }
            }
        }
        "show" => {
            let name = a.arg(2, "mode name")?;
            a.max_pos(3)?;
            let m = modes::load(name)?;
            if json {
                ok_json(&m);
            } else {
                print!(
                    "{}",
                    toml::to_string(&m).map_err(|e| Error::new("io", e.to_string()))?
                );
            }
        }
        "apply" => {
            let name = a.arg(2, "mode name")?;
            a.max_pos(3)?;
            let extras = !a.has("--no-extras");
            let m = modes::load(name)?;
            let mons = hypr.monitors()?;
            let (st, warnings) = modes::resolve_mode(&m, &mons, extras)?;
            let (quick, qwarn) = modes::resolve_quick(&m, &mons, extras);
            let mut out = ctx.apply(
                &format!("mode {name}"),
                &st,
                a.timeout()?,
                Some((Some(name.to_string()), extras)),
            )?;
            out.warnings.extend(warnings);
            out.warnings.extend(qwarn);
            if !quick.is_empty() {
                let n = night::Night::new(&hypr.instance, &ctx.state_dir);
                let (changed, w) = modes::apply_quick(&quick, Some(&n));
                out.changed.extend(changed);
                out.warnings.extend(w);
            }
            report(json, out);
        }
        "save" => {
            let name = a.arg(2, "mode name")?;
            a.max_pos(3)?;
            let desc = a.val("--description").map(str::to_string);
            let mut m = modes::from_current(hypr, desc)?;
            // Opt-in: a display mode that silently switches the audio
            // output on the next hotplug would be a surprise.
            if a.has("--with-quick") {
                let ns = night::Night::new(&hypr.instance, &ctx.state_dir).status();
                if ns.running {
                    m.night_light = Some(match ns.temperature {
                        Some(k) if ns.on => night::NightSetting::Kelvin(k),
                        _ => night::NightSetting::Switch(false),
                    });
                }
                m.audio_sink = audio::get_default().ok().flatten().map(|s| s.name);
            }
            let path = modes::save(name, &m, a.has("--force"))?;
            if json {
                ok_json(&json!({ "saved": name, "path": path }));
            } else {
                println!("saved mode `{name}` to {}", path.display());
            }
        }
        "put" => {
            let name = a.arg(2, "mode name")?;
            a.max_pos(3)?;
            let m = modes::from_json(
                a.val("--data")
                    .ok_or_else(|| Error::new("usage", "mode put needs --data JSON"))?,
            )?;
            let path = modes::save(name, &m, a.has("--force"))?;
            if json {
                ok_json(&json!({ "saved": name, "path": path }));
            } else {
                println!("saved mode `{name}` to {}", path.display());
            }
        }
        "rm" => {
            let name = a.arg(2, "mode name")?;
            a.max_pos(3)?;
            modes::remove(name)?;
            simple(json, "removed", true);
        }
        other => {
            return Err(Error::new(
                "usage",
                format!("unknown mode subcommand `{other}`"),
            ));
        }
    }
    Ok(())
}

/// `{"text","tooltip","class","alt"}` for a Waybar custom module
/// (`return-type: json`). Never fails: a broken line would blank the bar.
fn waybar_line(ctx: &Ctx, hypr: &Hyprland) {
    let mons = hypr.monitors().unwrap_or_default();
    let active: Vec<&Monitor> = mons
        .iter()
        .filter(|m| m.enabled && m.mirror_of.is_none())
        .collect();
    let mode = ctx.active_mode();
    let pending = ctx.pending_info();
    let text = match (&pending, &mode) {
        (Some(p), _) => format!("keep? {}s", p.remaining_ms.div_ceil(1000)),
        (None, Some(m)) => m.clone(),
        (None, None) => format!(
            "{} screen{}",
            active.len(),
            if active.len() == 1 { "" } else { "s" }
        ),
    };
    let tooltip = active
        .iter()
        .map(|m| {
            format!(
                "{} {}x{}@{}",
                m.name,
                m.width,
                m.height,
                spec::fmt_num(m.refresh.round())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let class = if pending.is_some() {
        "pending"
    } else if mode.is_some() {
        "mode"
    } else {
        "idle"
    };
    print_json(&json!({ "text": text, "tooltip": tooltip, "class": class, "alt": class }));
}

fn deck_cmd(a: &Args, json: bool) -> Result<()> {
    match a.arg(1, "deck subcommand (serve, pair, devices, unpair, test)")? {
        "serve" => {
            a.max_pos(2)?;
            deck::net::serve(deck::net::ServeOpts {
                port: a.num("--port")?.unwrap_or(deck::pair::DEFAULT_PORT),
                loopback: a.has("--loopback"),
            })
        }
        "pair" => {
            a.max_pos(2)?;
            let r = deck::net::ask("pair")?;
            if r["ok"] != true {
                return Err(Error::new(
                    "deck",
                    r["error"].as_str().unwrap_or("pairing failed").to_string(),
                ));
            }
            let payload = r["payload"].as_str().unwrap_or_default();
            if json {
                ok_json(&r);
            } else {
                println!("{}", deck::pair::render_text(payload)?);
                println!("Scan with the stance deck app within 120 s (one phone).");
                println!(
                    "PC fingerprint: {}",
                    r["fingerprint"].as_str().unwrap_or("?")
                );
                println!("Code: {payload}");
            }
            Ok(())
        }
        "devices" => {
            a.max_pos(2)?;
            // Works without a running server (then nothing shows as connected).
            let live = deck::net::ask("status").ok();
            let list = deck::pair::load_devices()?;
            if json {
                ok_json(&json!({ "devices": list.devices, "server": live }));
            } else {
                match &live {
                    Some(s) => println!(
                        "server: port {} on {}{}",
                        s["port"],
                        s["hosts"]
                            .as_array()
                            .map(|h| h
                                .iter()
                                .filter_map(|x| x.as_str())
                                .collect::<Vec<_>>()
                                .join(", "))
                            .unwrap_or_default(),
                        if s["loopback"] == true {
                            " (loopback only)"
                        } else {
                            ""
                        }
                    ),
                    None => println!("server: not running"),
                }
                if list.devices.is_empty() {
                    println!("no paired phones; run `stance deck pair`");
                }
                for d in &list.devices {
                    let on = live
                        .as_ref()
                        .and_then(|s| s["devices"].as_array())
                        .is_some_and(|v| {
                            v.iter()
                                .any(|x| x["name"] == d.name.as_str() && x["connected"] == true)
                        });
                    println!("{} {}", if on { "*" } else { " " }, d.name);
                }
            }
            Ok(())
        }
        "status" => {
            a.max_pos(2)?;
            let s = deck::net::ask("status")?;
            if json {
                ok_json(&s);
            } else {
                println!(
                    "listening on port {} ({}), PC key {}{}",
                    s["port"],
                    s["hosts"]
                        .as_array()
                        .map(|h| h
                            .iter()
                            .filter_map(|x| x.as_str())
                            .collect::<Vec<_>>()
                            .join(", "))
                        .unwrap_or_default(),
                    s["fingerprint"].as_str().unwrap_or("?"),
                    if s["pairing"] == true {
                        ", pairing open"
                    } else {
                        ""
                    }
                );
                for d in s["devices"].as_array().into_iter().flatten() {
                    println!(
                        "{} {}",
                        if d["connected"] == true { "*" } else { " " },
                        d["name"].as_str().unwrap_or("?")
                    );
                }
            }
            Ok(())
        }
        "buttons" => {
            a.max_pos(2)?;
            let d = deck::actions::Deck::load()?;
            let (buttons, custom) = d.configs();
            if json {
                ok_json(&json!({ "custom": custom, "button": buttons }));
            } else {
                println!(
                    "{} buttons ({})",
                    buttons.len(),
                    if custom { "deck.toml" } else { "default set" }
                );
                for b in &buttons {
                    println!("  {:<16} {:<20} {}", b.id, b.label, b.action);
                }
            }
            Ok(())
        }
        "put" => {
            a.max_pos(2)?;
            let text = deck::config::from_json(
                a.val("--data")
                    .ok_or_else(|| Error::new("usage", "deck put needs --data JSON"))?,
            )?;
            store::ensure_private_dir(&store::config_dir()?)?;
            let path = store::config_dir()?.join("deck.toml");
            store::atomic_write(&path, text.as_bytes(), true)?;
            simple(json, "saved", true);
            Ok(())
        }
        "reset" => {
            a.max_pos(2)?;
            let path = store::config_dir()?.join("deck.toml");
            let had = path.exists();
            if had {
                std::fs::rename(&path, path.with_extension("toml.bak"))
                    .map_err(|e| Error::new("io", e.to_string()))?;
            }
            simple(
                json,
                if had {
                    "back to the default buttons (old file kept as deck.toml.bak)"
                } else {
                    "already the default buttons"
                },
                had,
            );
            Ok(())
        }
        "unpair" => {
            let name = a.arg(2, "device name")?;
            a.max_pos(3)?;
            let removed = deck::pair::remove_device(name)?;
            simple(
                json,
                if removed {
                    "unpaired"
                } else {
                    "no such device"
                },
                removed,
            );
            Ok(())
        }
        "test" => deck::client::run(&a.pos[2..], a.val("--name"), json),
        other => Err(Error::new(
            "usage",
            format!("unknown deck subcommand `{other}`"),
        )),
    }
}

fn theme_cmd(a: &Args, json: bool) -> Result<()> {
    let th = theme::Themes::open()?;
    let force = a.has("--force");
    match a.pos.get(1).map(String::as_str).unwrap_or("list") {
        "list" => {
            a.max_pos(2)?;
            let l = th.list();
            if json {
                ok_json(&json!({ "themes": l }));
            } else {
                for t in &l {
                    let tag = if t.builtin { "" } else { " (yours)" };
                    println!("{:<22} {}{tag}", t.theme.id, t.theme.name);
                }
            }
        }
        "show" | "export" => {
            a.max_pos(3)?;
            let t = th.get(a.arg(2, "theme id")?)?;
            match a.val("--out") {
                Some(out) => {
                    theme::export_to(&t, std::path::Path::new(out), force)?;
                    if json {
                        ok_json(&json!({ "path": out }));
                    } else {
                        println!("exported {} to {out}", t.id);
                    }
                }
                None if json => ok_json(&t),
                None => println!("{}", serde_json::to_string_pretty(&t).unwrap_or_default()),
            }
        }
        "put" => {
            a.max_pos(2)?;
            let data = a
                .val("--data")
                .ok_or_else(|| Error::new("usage", "theme put needs --data JSON"))?;
            let t = theme::Theme::parse(data)?;
            th.put(&t, force)?;
            if json {
                ok_json(&t);
            } else {
                println!("saved theme {}", t.id);
            }
        }
        "import" => {
            a.max_pos(3)?;
            let t = th.import(std::path::Path::new(a.arg(2, "theme file")?), force)?;
            if json {
                ok_json(&t);
            } else {
                println!("imported theme {} ({})", t.id, t.name);
            }
        }
        "rm" => {
            a.max_pos(3)?;
            let id = a.arg(2, "theme id")?;
            th.rm(id)?;
            if json {
                ok_json(&json!({ "removed": id }));
            } else {
                println!("removed theme {id}");
            }
        }
        other => {
            return Err(Error::new(
                "usage",
                format!("unknown theme command `{other}` (list, show, put, import, export, rm)"),
            ));
        }
    }
    Ok(())
}

fn feature_cmd(a: &Args, json: bool) -> Result<()> {
    match a.pos.get(1).map(String::as_str).unwrap_or("list") {
        "list" => {
            a.max_pos(2)?;
            let l = feature::list()?;
            if json {
                ok_json(&json!({ "features": l }));
            } else {
                for f in &l {
                    let unit = match (f.unit, f.unit_active) {
                        (Some(u), Some(true)) => format!("{u} running"),
                        (Some(u), _) => format!("{u} stopped"),
                        (None, _) => String::new(),
                    };
                    println!(
                        "{:<6} {:<4} {unit}",
                        f.name,
                        if f.on { "on" } else { "off" }
                    );
                }
            }
        }
        verb @ ("on" | "off") => {
            a.max_pos(3)?;
            let name = a.arg(2, "feature name")?;
            let notes = feature::set(name, verb == "on", &state_dir()?)?;
            if json {
                ok_json(&json!({ "name": name, "on": verb == "on", "notes": notes }));
            } else {
                println!("{name}: {verb}");
                for n in notes {
                    eprintln!("note: {n}");
                }
            }
        }
        other => {
            return Err(Error::new(
                "usage",
                format!("unknown feature command `{other}` (list, on, off)"),
            ));
        }
    }
    Ok(())
}

fn clip_cmd(a: &Args, json: bool) -> Result<()> {
    let c = clip::Clip::open(&state_dir()?)?;
    let sub = a.pos.get(1).map(String::as_str).unwrap_or("list");
    let ids = || -> Result<Vec<u64>> {
        let v = a.pos[2..]
            .iter()
            .map(|s| clip::parse_id(s))
            .collect::<Result<Vec<_>>>()?;
        if v.is_empty() {
            return Err(Error::new("usage", "missing clip id"));
        }
        Ok(v)
    };
    let done = |rev: u64, msg: &str| {
        if json {
            ok_json(&json!({ "rev": rev }));
        } else if !msg.is_empty() {
            println!("{msg}");
        }
    };
    match sub {
        "ingest" => {
            a.max_pos(2)?;
            // Must never break the wl-paste --watch loop: report, exit 0.
            if let Err(e) = clip::ingest(&c, a.has("--image"), &mut std::io::stdin().lock()) {
                eprintln!("stance clip ingest: {e}");
            }
        }
        "list" => {
            a.max_pos(2)?;
            let kind = a.val("--kind").map(clip::Kind::parse).transpose()?;
            let limit = a.num::<usize>("--limit")?.unwrap_or(200).min(2000);
            let l = c.list(kind, a.val("--query"), limit)?;
            if json {
                ok_json(&l);
            } else {
                if !l.enabled {
                    println!(
                        "clip shelf is off (stance clip config put --data '{{\"enabled\":true}}')"
                    );
                }
                for i in &l.items {
                    let pin = if i.pinned { "*" } else { " " };
                    let kind = format!("{:?}", i.kind).to_lowercase();
                    let what = match (&i.path, i.w, i.h) {
                        (Some(_), Some(w), Some(h)) => format!("{} {w}x{h}", i.mime),
                        (Some(_), _, _) => i.mime.clone(),
                        _ => i.preview.chars().take(70).collect(),
                    };
                    println!("{pin}{:>6} {kind:<5} {what}", i.id);
                }
            }
        }
        "get" => {
            a.max_pos(3)?;
            let e = c.entry(clip::parse_id(a.arg(2, "clip id")?)?)?;
            if e.kind == clip::Kind::Image {
                let p = c.blob_path(&e).to_string_lossy().into_owned();
                if json {
                    ok_json(&json!({ "path": p, "mime": e.mime }));
                } else {
                    println!("{p}");
                }
            } else {
                let mut s = String::new();
                use std::io::Read;
                c.open_blob(&e)?
                    .take(clip::MAX_TEXT)
                    .read_to_string(&mut s)
                    .map_err(|e| Error::new("io", format!("clip blob: {e}")))?;
                if json {
                    ok_json(&json!({ "text": s, "mime": e.mime }));
                } else if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
                    // Copied text may carry escape sequences; a terminal
                    // must not execute them. Pipes get the exact bytes.
                    let safe: String = s
                        .chars()
                        .map(|ch| {
                            if ch.is_control() && ch != '\n' && ch != '\t' {
                                '\u{FFFD}'
                            } else {
                                ch
                            }
                        })
                        .collect();
                    print!("{safe}");
                } else {
                    print!("{s}");
                }
            }
        }
        "copy" => {
            a.max_pos(3)?;
            let rev = c.copy(clip::parse_id(a.arg(2, "clip id")?)?)?;
            done(rev, "copied");
        }
        "pin" | "unpin" => done(c.set_pinned(&ids()?, sub == "pin")?, ""),
        "rm" => done(c.remove(&ids()?)?, "removed (stance clip undo restores)"),
        "purge" => {
            a.max_pos(2)?;
            let (n, rev) = c.purge()?;
            done(rev, &format!("deleted {n} removed item(s) for good"));
        }
        "undo" => {
            a.max_pos(2)?;
            let (n, rev) = c.undo()?;
            done(rev, &format!("restored {n}"));
        }
        "prune" => {
            a.max_pos(2)?;
            done(c.prune()?, "");
        }
        "clear" => {
            a.max_pos(2)?;
            let all = a.has("--all");
            done(
                c.clear(all)?,
                if all {
                    "clip store wiped"
                } else {
                    "cleared (pinned kept; stance clip undo restores)"
                },
            );
        }
        "pause" => {
            a.max_pos(3)?;
            let secs = clip::parse_duration(a.pos.get(2).map(String::as_str).unwrap_or("15m"))?;
            if secs == 0 {
                return Err(Error::new("usage", "use `stance clip resume` to resume"));
            }
            let until = c.pause(secs)?;
            if json {
                ok_json(&json!({ "paused_until": until }));
            } else {
                println!(
                    "capture paused for {}m (stance clip resume to resume)",
                    secs.div_ceil(60)
                );
            }
        }
        "resume" => {
            a.max_pos(2)?;
            c.pause(0)?;
            if json {
                ok_json(&json!({ "paused_until": null }));
            } else {
                println!("capture resumed");
            }
        }
        "config" => match a.pos.get(2).map(String::as_str) {
            None | Some("show") => {
                let cfg = c.config()?;
                if json {
                    ok_json(&cfg);
                } else {
                    println!(
                        "enabled: {}\ntext: {}\nimage: {}\nretention_days: {}",
                        cfg.enabled, cfg.text, cfg.image, cfg.retention_days
                    );
                    if let Some(t) = cfg.paused(clip::now()) {
                        println!(
                            "paused: {}m left",
                            t.saturating_sub(clip::now()).div_ceil(60)
                        );
                    }
                }
            }
            Some("put") => {
                let data = a
                    .val("--data")
                    .ok_or_else(|| Error::new("usage", "clip config put needs --data JSON"))?;
                let cfg = c.put_config(data)?;
                if json {
                    ok_json(&cfg);
                }
            }
            Some(other) => {
                return Err(Error::new(
                    "usage",
                    format!("unknown clip config command `{other}` (show, put)"),
                ));
            }
        },
        other => {
            return Err(Error::new(
                "usage",
                format!(
                    "unknown clip command `{other}` (list, get, copy, pin, unpin, rm, undo, purge, prune, clear, pause, resume, config, ingest)"
                ),
            ));
        }
    }
    Ok(())
}

fn rules_cmd(a: &Args, json: bool) -> Result<()> {
    let path = modes::rules_path()?;
    match a.pos.get(1).map(String::as_str) {
        None | Some("show") => {
            a.max_pos(2)?;
            let r = modes::load_rules(&path)?;
            if json {
                ok_json(&r);
            } else {
                print!(
                    "{}",
                    toml::to_string(&r).map_err(|e| Error::new("io", e.to_string()))?
                );
            }
        }
        Some("put") => {
            a.max_pos(2)?;
            let r = modes::rules_from_json(
                a.val("--data")
                    .ok_or_else(|| Error::new("usage", "rules put needs --data JSON"))?,
            )?;
            modes::save_rules(&r)?;
            if json {
                ok_json(&json!({ "saved": r.rules.len(), "path": path }));
            } else {
                println!("saved {} rule(s) to {}", r.rules.len(), path.display());
            }
        }
        Some(other) => {
            return Err(Error::new(
                "usage",
                format!("unknown rules subcommand `{other}` (show, put)"),
            ));
        }
    }
    Ok(())
}

fn night_cmd(a: &Args, ctx: &Ctx, hypr: &Hyprland, json: bool) -> Result<()> {
    let n = night::Night::new(&hypr.instance, &ctx.state_dir);
    let kelvin = |i: usize| -> Result<u32> {
        let v = a.arg(i, "temperature in K")?;
        v.parse()
            .map_err(|_| Error::new("usage", format!("`{v}` is not a temperature")))
    };
    let st = match a.pos.get(1).map(String::as_str).unwrap_or("status") {
        "status" => {
            a.max_pos(2)?;
            n.status()
        }
        "on" => {
            a.max_pos(3)?;
            n.on(if a.pos.len() > 2 {
                Some(kelvin(2)?)
            } else {
                None
            })?
        }
        "off" => {
            a.max_pos(2)?;
            n.off()?
        }
        "toggle" => {
            a.max_pos(2)?;
            n.toggle()?
        }
        "set" => {
            a.max_pos(3)?;
            n.set(kelvin(2)?)?
        }
        other => {
            return Err(Error::new(
                "usage",
                format!("unknown night subcommand `{other}` (status, on, off, toggle, set)"),
            ));
        }
    };
    if json {
        ok_json(&st);
    } else if !st.running {
        println!("night light: off (hyprsunset not running)");
    } else {
        match st.temperature {
            Some(k) if st.on => println!("night light: on, {k} K"),
            _ => println!("night light: off"),
        }
    }
    Ok(())
}

fn connectors(hypr: &Hyprland) -> Result<Vec<String>> {
    Ok(hypr
        .monitors()?
        .into_iter()
        .filter(|m| m.enabled)
        .map(|m| m.name)
        .collect())
}

fn print_brightness(d: &brightness::Display) {
    println!("{:<8} {:>3}%  ({})", d.monitor, d.percent, d.kind);
}

fn brightness_cmd(a: &Args, hypr: &Hyprland, json: bool) -> Result<()> {
    match a.pos.get(1).map(String::as_str) {
        None | Some("list") => {
            a.max_pos(2)?;
            let list = brightness::list(&connectors(hypr)?)?;
            if json {
                ok_json(&list);
            } else if list.is_empty() {
                println!("no screen with controllable brightness (see `stance doctor`)");
            } else {
                list.iter().for_each(print_brightness);
            }
        }
        Some(q) => {
            let mons = hypr.monitors()?;
            let mon = plan::resolve(&mons, q)?.name.clone();
            match a.pos.get(2) {
                None => {
                    let d = brightness::get(&mon)?;
                    if json {
                        ok_json(&d);
                    } else {
                        print_brightness(&d);
                    }
                }
                Some(v) => {
                    a.max_pos(3)?;
                    let r = brightness::set(&mon, brightness::parse_change(v)?)?;
                    if json {
                        ok_json(&r);
                    } else {
                        print_brightness(&r.display);
                        for w in &r.warnings {
                            println!("note: {w}");
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn audio_cmd(a: &Args, json: bool) -> Result<()> {
    match a.pos.get(1).map(String::as_str) {
        None | Some("list") => {
            a.max_pos(2)?;
            let sinks = audio::list()?;
            if json {
                ok_json(&sinks);
            } else {
                for s in &sinks {
                    println!(
                        "{} {:>4}  {}{}",
                        if s.default { "*" } else { " " },
                        s.id,
                        s.description,
                        if s.virtual_ { "  (virtual)" } else { "" }
                    );
                }
            }
        }
        Some("set") => {
            let q = a.arg(2, "sink")?;
            a.max_pos(3)?;
            let s = audio::set_default(q)?;
            if json {
                ok_json(&s);
            } else {
                println!("default output: {}", s.description);
            }
        }
        Some(other) => {
            return Err(Error::new(
                "usage",
                format!("unknown audio subcommand `{other}` (list, set)"),
            ));
        }
    }
    Ok(())
}

/// Everything the panel's quick section shows, in one call. Each part
/// fails on its own: a missing ddcutil does not hide the night light.
fn quick_cmd(ctx: &Ctx, hypr: &Hyprland, json: bool) -> Result<()> {
    let mut errors = serde_json::Map::new();
    let night = night::Night::new(&hypr.instance, &ctx.state_dir).status();
    let bright = match connectors(hypr).and_then(|c| brightness::list(&c)) {
        Ok(l) => l,
        Err(e) => {
            errors.insert("brightness".into(), e.message.into());
            vec![]
        }
    };
    let sinks = match audio::list() {
        Ok(l) => l,
        Err(e) => {
            errors.insert("audio".into(), e.message.into());
            vec![]
        }
    };
    if json {
        ok_json(&json!({ "night": night, "brightness": bright, "sinks": sinks, "errors": errors }));
    } else {
        match night.temperature {
            Some(k) if night.on => println!("night light: on, {k} K"),
            _ => println!("night light: off"),
        }
        bright.iter().for_each(print_brightness);
        if let Some(s) = sinks.iter().find(|s| s.default) {
            println!("audio: {}", s.description);
        }
        for (k, v) in &errors {
            println!("note: {k}: {}", v.as_str().unwrap_or_default());
        }
    }
    Ok(())
}

fn persist_cmd(ctx: &Ctx, hypr: &Hyprland, print: bool, json: bool) -> Result<()> {
    let parser = hypr.parser()?;
    let _lock = ctx.lock()?;
    ctx.sweep();
    if !print && ctx.pending_info().is_some() {
        return Err(Error::new(
            "pending",
            "a change is waiting for confirmation; run `stance confirm` first",
        ));
    }
    let text = persist::render(&hypr.monitors()?, parser)?;
    if print {
        if json {
            ok_json(&json!({ "text": text }));
        } else {
            print!("{text}");
        }
        return Ok(());
    }
    let path = persist::write(&text, parser)?;
    let line = persist::load_line(parser);
    if json {
        ok_json(&json!({ "path": path, "load_line": line }));
    } else {
        println!("wrote {}", path.display());
        println!(
            "make sure your Hyprland config loads it once (after nwg-displays' monitors file, if any):"
        );
        println!("  {line}");
    }
    Ok(())
}

fn setup_cmd(a: &Args, json: bool) -> Result<()> {
    let (target, detected) = match a.pos.get(1) {
        Some(t) => (setup::Target::parse(t)?, None),
        None => {
            let (t, why) = setup::detect(&doctor::cmdlines());
            (t, Some(why))
        }
    };
    let panel = setup::panel_dir();
    let plan = setup::plan(target, detected, panel.as_deref());
    let dry = a.has("--dry-run");
    let notes = if dry { Vec::new() } else { setup::apply(&plan) };
    if json {
        ok_json(&json!({ "plan": plan, "dry_run": dry, "notes": notes }));
        return Ok(());
    }
    let name = format!("{:?}", plan.target).to_lowercase();
    match &plan.detected {
        Some(why) => println!("Setup for {name} ({why}; pass a target to choose)\n"),
        None => println!("Setup for {name}\n"),
    }
    for u in &plan.units {
        let verb = if dry { "would enable" } else { "enabled" };
        if !notes.iter().any(|n| n.starts_with(u)) {
            println!("  {verb} {u}");
        }
    }
    for n in &notes {
        println!("  note: {n}");
    }
    for (i, s) in plan.steps.iter().enumerate() {
        println!("{}. {s}", i + 1);
    }
    Ok(())
}

/// A copy-pasteable block for bug reports: versions, session, features
/// and the doctor checks. Nothing personal beyond what doctor prints.
fn bug_report() -> String {
    let first_line = |prog: &str, args: &[&str]| -> String {
        tool::find(prog)
            .and_then(|p| tool::run_tool(&p, args, std::time::Duration::from_secs(3)).ok())
            .and_then(|o| o.lines().next().map(str::trim).map(String::from))
            .unwrap_or_else(|| "not found".into())
    };
    let os = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|s| {
            s.lines().find_map(|l| {
                l.strip_prefix("PRETTY_NAME=")
                    .map(|v| v.trim_matches('"').to_string())
            })
        })
        .unwrap_or_else(|| "unknown".into());
    let hypr = Hyprland::connect()
        .and_then(|h| h.version())
        .unwrap_or_else(|e| format!("not reachable ({})", e.code));
    let mut out = String::from("```\n");
    out += &format!("stance      {}\n", env!("CARGO_PKG_VERSION"));
    out += &format!("hyprland    {hypr}\n");
    out += &format!("quickshell  {}\n", first_line("qs", &["--version"]));
    out += &format!("os          {os}\n");
    out += &format!("kernel      {}\n", first_line("uname", &["-r"]));
    if let Ok(fs) = feature::list() {
        let f: Vec<String> = fs
            .iter()
            .map(|f| format!("{}={}", f.name, if f.on { "on" } else { "off" }))
            .collect();
        out += &format!("features    {}\n", f.join(" "));
    }
    out += "\n";
    // Instance and socket paths add nothing to a report.
    for c in doctor::run()
        .into_iter()
        .filter(|c| !matches!(c.id, "instance" | "socket1" | "socket2"))
    {
        out += &format!(
            "{:<5} {:<15} {}\n",
            format!("{:?}", c.status).to_lowercase(),
            c.id,
            c.message
        );
    }
    out += "```\n";
    // Home path and user name are not needed to debug stance.
    if let Some(home) = std::env::var_os("HOME").and_then(|h| h.into_string().ok())
        && home.len() > 1
    {
        out = out.replace(&home, "~");
    }
    if let Ok(uid) = hypr::current_uid() {
        out = out.replace(&format!("/run/user/{uid}"), "/run/user/<uid>");
    }
    out
}

fn doctor_cmd(json: bool) -> ExitCode {
    let checks = doctor::run();
    let failed = checks.iter().any(|c| c.status == doctor::Status::Fail);
    if json {
        print_json(&json!({ "ok": !failed, "schema": SCHEMA, "data": checks }));
    } else {
        for c in &checks {
            let tag = match c.status {
                doctor::Status::Ok => "  ok ",
                doctor::Status::Info => "info ",
                doctor::Status::Warn => "warn ",
                doctor::Status::Fail => "FAIL ",
            };
            println!("{tag} {:<15} {}", c.id, c.message);
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn report(json: bool, o: Outcome) {
    if json {
        ok_json(&o);
        return;
    }
    if o.changed.is_empty() {
        println!("{}: already in place, nothing changed", o.label);
    } else {
        println!("{}: changed {}", o.label, o.changed.join(", "));
    }
    if let Some(p) = &o.pending {
        println!(
            "keep it with `stance confirm` within {:.0} s, otherwise it reverts",
            p.remaining_ms as f64 / 1000.0
        );
    }
    for w in &o.warnings {
        println!("note: {w}");
    }
}

fn simple(json: bool, msg: &str, did: bool) {
    if json {
        ok_json(&json!({ "message": msg, "done": did }));
    } else {
        println!("{msg}");
    }
}

fn print_json(v: &impl Serialize) {
    // Serialization of our own types cannot fail.
    println!("{}", serde_json::to_string(v).unwrap_or_default());
}

/// JSON envelope version (see docs/protocol.md); bumped on breaking changes.
const SCHEMA: u32 = 1;

fn ok_json(data: &impl Serialize) {
    print_json(&json!({ "ok": true, "schema": SCHEMA, "data": data }));
}

fn print_monitors(mons: &[Monitor]) {
    for m in mons {
        let state = if !m.enabled {
            "off".to_string()
        } else if let Some(src) = &m.mirror_of {
            format!("mirror of {src}")
        } else {
            format!(
                "{}x{}@{} at {},{} scale {}{}{}",
                m.width,
                m.height,
                spec::fmt_num(m.refresh),
                m.x,
                m.y,
                spec::fmt_num(m.scale),
                if m.transform != 0 {
                    format!(" rot {}", m.transform)
                } else {
                    String::new()
                },
                if m.vrr { " vrr" } else { "" },
            )
        };
        let focus = if m.focused { "*" } else { " " };
        println!("{focus} {:<8} {:<32} {state}", m.name, m.description);
    }
}
