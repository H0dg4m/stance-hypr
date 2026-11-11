//! `stance daemon`: optional. Blocks on Hyprland's event socket (no
//! polling) and applies hotplug rules; re-applies the active mode after a
//! config reload (which wipes runtime changes).

use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::{BufRead, ErrorKind};
use std::os::unix::fs::OpenOptionsExt;
use std::time::{Duration, Instant};

use crate::apply::Ctx;
use crate::backend::{Backend, Notice};
use crate::error::{Error, Result};
use crate::modes;

const DEBOUNCE: Duration = Duration::from_millis(1500);
const POWER_POLL: Duration = Duration::from_secs(1);
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(400);
const BURST: usize = 3;
const BURST_WINDOW: Duration = Duration::from_secs(30);
const PAUSE: Duration = Duration::from_secs(60);

fn log(msg: &str) {
    eprintln!("stance daemon: {msg}");
}

/// Which events we care about.
#[derive(Debug, PartialEq)]
pub enum Event {
    Hotplug,
    Reload,
    Other,
}

pub fn classify(line: &str) -> Event {
    let name = line.split_once(">>").map_or(line, |(n, _)| n);
    match name {
        "monitoradded" | "monitoraddedv2" | "monitorremoved" | "monitorremovedv2" => Event::Hotplug,
        "configreloaded" => Event::Reload,
        _ => Event::Other,
    }
}

/// Is a stance daemon holding the lock in `state_dir`? (Read-only probe:
/// takes the lock for an instant only if nobody has it.)
pub fn running(state_dir: &std::path::Path) -> bool {
    let Ok(f) = OpenOptions::new()
        .write(true)
        .open(state_dir.join("daemon.lock"))
    else {
        return false;
    };
    f.try_lock().is_err()
}

/// Connector of a `monitoradded` / `monitoraddedv2` event.
pub fn added_name(line: &str) -> Option<String> {
    let (ev, data) = line.split_once(">>")?;
    let name = match ev {
        "monitoradded" => data,
        "monitoraddedv2" => data.split(',').nth(1)?,
        _ => return None,
    };
    crate::spec::valid_connector(name).then(|| name.to_string())
}

pub fn run(hypr: &dyn Backend, state_dir: std::path::PathBuf) -> Result<()> {
    let ctx = Ctx {
        backend: hypr,
        state_dir: state_dir.clone(),
    };
    crate::store::ensure_private_dir(&state_dir)?;
    // One daemon per user.
    let lockf = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(state_dir.join("daemon.lock"))
        .map_err(|e| Error::new("io", format!("daemon lock: {e}")))?;
    if lockf.try_lock().is_err() {
        return Err(Error::new(
            "running",
            "another stance daemon is already running",
        ));
    }

    let night = crate::night::Night::open(hypr, &state_dir).ok();
    let mut events = hypr.raw_events()?;
    log(&format!("watching Hyprland {}", hypr.session_id()));

    let mut hotplug_at: Option<Instant> = None;
    let mut reload_at: Option<Instant> = None;
    let mut history: VecDeque<Instant> = VecDeque::new();
    let mut paused_until: Option<Instant> = None;
    let mut added: Vec<String> = Vec::new();
    // Charger in/out re-runs the rules (rules may have `on_power`).
    let power_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let power_watch = crate::uevent::watch_power(power_flag.clone());
    let mut last_ac = crate::power::on_ac();
    if !power_watch {
        log("power-supply events unavailable; power rules apply on monitor changes only");
    }
    // Apply the matching rule once at start, like a hotplug.
    hotplug_at = hotplug_at.or(Some(Instant::now()));

    let mut line = String::new();
    loop {
        let next = [hotplug_at, reload_at].into_iter().flatten().min();
        let mut wait = next.map(|t| {
            t.saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1))
        });
        // Wake now and then to look at the power flag (set by a thread).
        if power_watch {
            wait = Some(wait.map_or(POWER_POLL, |w| w.min(POWER_POLL)));
        }
        events
            .get_ref()
            .set_read_timeout(wait)
            .map_err(|e| Error::new("io", e.to_string()))?;

        line.clear();
        match events.read_line(&mut line) {
            Ok(0) => {
                log("Hyprland closed the event socket, exiting");
                return Ok(());
            }
            Ok(_) => match classify(line.trim_end()) {
                Event::Hotplug => {
                    if let Some(n) = added_name(line.trim_end())
                        && !added.contains(&n)
                    {
                        added.push(n);
                    }
                    hotplug_at = Some(Instant::now() + DEBOUNCE)
                }
                Event::Reload => reload_at = Some(Instant::now() + RELOAD_DEBOUNCE),
                Event::Other => {}
            },
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(e) => return Err(Error::new("ipc", format!("event socket: {e}"))),
        }

        if power_flag.swap(false, std::sync::atomic::Ordering::Relaxed) {
            let ac = crate::power::on_ac();
            if ac != last_ac {
                last_ac = ac;
                log(&format!(
                    "power: now on {}",
                    if ac == Some(true) { "ac" } else { "battery" }
                ));
                hotplug_at = Some(Instant::now() + DEBOUNCE);
            }
        }

        let now = Instant::now();
        if hotplug_at.is_some_and(|t| t <= now) {
            hotplug_at = None;
            let added = std::mem::take(&mut added);
            if paused_until.is_some_and(|p| p > now) {
                log("hotplug ignored (paused after a burst of changes)");
            } else if let Some(changed) = on_hotplug(&ctx, night.as_ref(), &added)
                && changed
            {
                history.push_back(now);
                while history
                    .front()
                    .is_some_and(|t| now.duration_since(*t) > BURST_WINDOW)
                {
                    history.pop_front();
                }
                if history.len() >= BURST {
                    paused_until = Some(now + PAUSE);
                    history.clear();
                    hypr.notify(Notice::AutoPaused);
                    log("too many automatic changes, pausing rules for 60 s");
                }
            }
        }
        if reload_at.is_some_and(|t| t <= now) {
            reload_at = None;
            on_reload(&ctx);
        }
    }
}

/// Returns Some(changed?) if a rule was applied.
fn on_hotplug(ctx: &Ctx, night: Option<&crate::night::Night>, added: &[String]) -> Option<bool> {
    if ctx.pending_info().is_some() {
        log("hotplug: a user change is awaiting confirmation, rules skipped");
        return None;
    }
    let rules = match modes::rules_path().and_then(|p| modes::load_rules(&p)) {
        Ok(r) => r,
        Err(e) => {
            log(&format!("rules: {}", e.message));
            return None;
        }
    };
    let mons = ctx.backend.monitors().map_err(|e| log(&e.message)).ok()?;
    let power = modes::PowerSource::from_ac(crate::power::on_ac());
    let Some(rule) = modes::matching_rule(&rules, &mons, power) else {
        return restore_remembered(ctx, &mons, added);
    };
    apply_mode(ctx, &rule.mode, rule.allow_extras, "rule", night)
}

/// No rule matched: put re-plugged monitors back where they last were.
fn restore_remembered(ctx: &Ctx, mons: &[crate::model::Monitor], added: &[String]) -> Option<bool> {
    let specs: Vec<_> = mons
        .iter()
        .filter(|m| added.contains(&m.name) && m.enabled && m.mirror_of.is_none())
        .filter_map(|m| {
            let others: Vec<_> = mons.iter().filter(|o| o.name != m.name).cloned().collect();
            let mut off = m.clone();
            off.enabled = false;
            let mut all = others;
            all.push(off.clone());
            crate::memory::recall(&ctx.state_dir, &off, &all)
                .filter(|s| !crate::apply::satisfies(m, s))
        })
        .collect();
    if specs.is_empty() {
        log("hotplug: no rule matches the connected monitors, doing nothing");
        return None;
    }
    let names: Vec<_> = specs.iter().map(|s| s.name.clone()).collect();
    let st = crate::apply::State {
        monitors: specs,
        ..Default::default()
    };
    match ctx.apply("restore", &st, 0, None) {
        Ok(o) => {
            log(&format!(
                "hotplug: restored last position of {}",
                names.join(", ")
            ));
            Some(!o.changed.is_empty())
        }
        Err(e) => {
            log(&format!(
                "hotplug: restoring {} failed: {}",
                names.join(", "),
                e.message
            ));
            None
        }
    }
}

fn on_reload(ctx: &Ctx) {
    // Another tool (nwg-displays, a hand edit) may have moved screens.
    if let Ok(mons) = ctx.backend.monitors() {
        crate::memory::record(&ctx.state_dir, &mons);
    }
    let Some((mode, extras)) = ctx.active() else {
        return;
    };
    log(&format!("config reloaded, re-applying mode `{mode}`"));
    // A config reload resets compositor options, not night light, audio or
    // brightness: re-apply only what Hyprland forgot.
    apply_mode(ctx, &mode, extras, "reload", None);
}

fn apply_mode(
    ctx: &Ctx,
    name: &str,
    extras: bool,
    why: &str,
    night: Option<&crate::night::Night>,
) -> Option<bool> {
    let res = modes::load(name).and_then(|m| {
        let mons = ctx.backend.monitors()?;
        let (st, warnings) = modes::resolve_mode(&m, &mons, extras)?;
        for w in warnings {
            log(&w);
        }
        let out = ctx.apply(
            &format!("{why}: {name}"),
            &st,
            0,
            Some((Some(name.to_string()), extras)),
        )?;
        // Night light, audio and brightness follow a real display change
        // only, so a manual slider is not overridden on every event. They
        // run on their own thread: a slow ddcutil must not stall events.
        if let Some(n) = night
            && !out.changed.is_empty()
        {
            let (q, w) = modes::resolve_quick(&m, &ctx.backend.monitors()?, extras);
            w.iter().for_each(|w| log(w));
            if !q.is_empty() {
                let n = n.clone();
                std::thread::spawn(move || {
                    let (changed, w) = modes::apply_quick(&q, Some(&n));
                    w.iter().for_each(|w| log(w));
                    if !changed.is_empty() {
                        log(&format!("quick settings: {}", changed.join(", ")));
                    }
                });
            }
        }
        Ok(out)
    });
    match res {
        Ok(o) => {
            if !o.changed.is_empty() {
                log(&format!(
                    "{why}: applied `{name}` ({})",
                    o.changed.join(", ")
                ));
            }
            Some(!o.changed.is_empty())
        }
        Err(e) => {
            log(&format!("{why}: `{name}` failed: {}", e.message));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_events() {
        assert_eq!(classify("monitoraddedv2>>2,DP-2,desc"), Event::Hotplug);
        assert_eq!(classify("monitorremoved>>DP-2"), Event::Hotplug);
        assert_eq!(classify("configreloaded>>"), Event::Reload);
        assert_eq!(classify("workspace>>2"), Event::Other);
        assert_eq!(classify("monitoraddedfake>>x"), Event::Other);
        assert_eq!(
            added_name("monitoraddedv2>>2,DP-2,JDR EPIC Q40").as_deref(),
            Some("DP-2")
        );
        assert_eq!(
            added_name("monitoradded>>HDMI-A-1").as_deref(),
            Some("HDMI-A-1")
        );
        assert_eq!(added_name("monitorremoved>>DP-2"), None);
        assert_eq!(added_name("monitoradded>>../x"), None);
    }
}
