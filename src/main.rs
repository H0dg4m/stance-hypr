mod apply;
mod audio;
mod backend;
mod brightness;
mod daemon;
mod doctor;
mod error;
mod hypr;
mod model;
mod modes;
mod night;
mod persist;
mod plan;
mod power;
mod spec;
mod store;
mod tool;

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
  stance confirm                       Keep the last change
  stance revert                        Undo the last change now
  stance persist [--print]             Save the current setup to your Hyprland config dir
  stance mode list
  stance mode show <name>
  stance mode apply <name> [--no-extras]
  stance mode save <name> [--description TEXT] [--force] [--with-quick]
      --with-quick also records night light and audio output
  stance mode rm <name>
  stance night [status|on [K]|off|toggle|set K]   Night light (hyprsunset)
  stance brightness [list]             Brightness of screens (backlight, DDC/CI)
  stance brightness <monitor> N|+N|-N  Set brightness in percent
  stance audio [list]                  Audio outputs
  stance audio set <sink>              Default output (id, node name or unique text)
  stance quick                         Night light, brightness and audio at once
  stance daemon                        Hotplug rules (optional, long-running)
  stance doctor                        Check the environment (read-only)

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
    let json = raw.iter().any(|a| a == "--json");
    let res = Args::parse(raw).and_then(|a| run(&a));
    match res {
        Ok(code) => code,
        Err(e) => {
            if json {
                print_json(
                    &json!({ "ok": false, "error": { "code": e.code, "message": e.message } }),
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
        return Ok(doctor_cmd(json));
    }

    if cmd == "audio" {
        return audio_cmd(a, json).map(|_| ExitCode::SUCCESS);
    }

    let hypr = Hyprland::connect()?;
    let ctx = Ctx {
        backend: &hypr,
        state_dir: state_dir()?,
    };

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
                ok_json(
                    &json!({ "pending": pending, "active_mode": active, "warnings": warnings }),
                );
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
            let specs = plan::plan_set(&mons, target, &args)?;
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

fn doctor_cmd(json: bool) -> ExitCode {
    let checks = doctor::run();
    let failed = checks.iter().any(|c| c.status == doctor::Status::Fail);
    if json {
        print_json(&json!({ "ok": !failed, "data": checks }));
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

fn ok_json(data: &impl Serialize) {
    print_json(&json!({ "ok": true, "data": data }));
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
