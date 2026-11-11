//! `stance daemon`: optional. Blocks on Hyprland's event socket (no
//! polling) and applies hotplug rules; re-applies the active mode after a
//! config reload (which wipes runtime changes).

use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::time::{Duration, Instant};

use crate::apply::Ctx;
use crate::backend::{Backend, Event, EventSource, Next, Notice};
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

/// What the event loop needs from the outside: a clock, the power state
/// and the three things it may do. Tests script all of it.
trait Host {
    fn now(&self) -> Instant;
    /// Charger plugged or unplugged since last asked.
    fn power_changed(&mut self) -> bool;
    /// Returns Some(changed?) if a rule was applied.
    fn hotplug(&mut self, added: &[String]) -> Option<bool>;
    fn reload(&mut self);
    /// Too many automatic changes: tell the user.
    fn paused(&mut self);
}

/// Debounces hotplug and reload events, remembers new screens, and pauses
/// the rules after a burst of automatic changes. Blocks in `events.next`
/// only; no polling beyond the optional power check.
fn event_loop(events: &mut dyn EventSource, host: &mut dyn Host, power_poll: bool) -> Result<()> {
    let mut hotplug_at: Option<Instant> = None;
    let mut reload_at: Option<Instant> = None;
    let mut history: VecDeque<Instant> = VecDeque::new();
    let mut paused_until: Option<Instant> = None;
    let mut added: Vec<String> = Vec::new();
    // Apply the matching rule once at start, like a hotplug.
    hotplug_at = hotplug_at.or(Some(host.now()));

    loop {
        let next = [hotplug_at, reload_at].into_iter().flatten().min();
        let mut wait = next.map(|t| {
            t.saturating_duration_since(host.now())
                .max(Duration::from_millis(1))
        });
        // Wake now and then to look at the power flag (set by a thread).
        if power_poll {
            wait = Some(wait.map_or(POWER_POLL, |w| w.min(POWER_POLL)));
        }

        match events.next(wait)? {
            Next::Closed => {
                log("the compositor closed the event socket, exiting");
                return Ok(());
            }
            Next::Event(Event::MonitorAdded { name }) => {
                if !name.is_empty() && !added.contains(&name) {
                    added.push(name);
                }
                hotplug_at = Some(host.now() + DEBOUNCE);
            }
            Next::Event(Event::MonitorRemoved { .. }) => {
                hotplug_at = Some(host.now() + DEBOUNCE);
            }
            Next::Event(Event::ConfigReloaded) => reload_at = Some(host.now() + RELOAD_DEBOUNCE),
            Next::Tick => {}
        }

        if host.power_changed() {
            hotplug_at = Some(host.now() + DEBOUNCE);
        }

        let now = host.now();
        if hotplug_at.is_some_and(|t| t <= now) {
            hotplug_at = None;
            let added = std::mem::take(&mut added);
            if paused_until.is_some_and(|p| p > now) {
                log("hotplug ignored (paused after a burst of changes)");
            } else if host.hotplug(&added) == Some(true) {
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
                    host.paused();
                    log("too many automatic changes, pausing rules for 60 s");
                }
            }
        }
        if reload_at.is_some_and(|t| t <= now) {
            reload_at = None;
            host.reload();
        }
    }
}

struct Live<'a> {
    ctx: Ctx<'a>,
    night: Option<crate::night::Night>,
    power_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    last_ac: Option<bool>,
}

impl Host for Live<'_> {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn power_changed(&mut self) -> bool {
        if !self
            .power_flag
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            return false;
        }
        let ac = crate::power::on_ac();
        if ac == self.last_ac {
            return false;
        }
        self.last_ac = ac;
        log(&format!(
            "power: now on {}",
            if ac == Some(true) { "ac" } else { "battery" }
        ));
        true
    }

    fn hotplug(&mut self, added: &[String]) -> Option<bool> {
        on_hotplug(&self.ctx, self.night.as_ref(), added)
    }

    fn reload(&mut self) {
        on_reload(&self.ctx);
    }

    fn paused(&mut self) {
        self.ctx.backend.notify(Notice::AutoPaused);
    }
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

    let mut events = hypr.events()?;
    log(&format!("watching {} {}", hypr.name(), hypr.session_id()));

    // Charger in/out re-runs the rules (rules may have `on_power`).
    let power_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let power_watch = crate::uevent::watch_power(power_flag.clone());
    if !power_watch {
        log("power-supply events unavailable; power rules apply on monitor changes only");
    }
    let mut host = Live {
        night: crate::night::Night::open(hypr, &state_dir).ok(),
        ctx,
        power_flag,
        last_ac: crate::power::on_ac(),
    };
    event_loop(events.as_mut(), &mut host, power_watch)
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
    use std::cell::Cell;
    use std::rc::Rc;

    /// Virtual time: the source advances it, the host reads it.
    #[derive(Clone)]
    struct Clock(Rc<Cell<Instant>>);

    impl Clock {
        fn advance(&self, d: Duration) {
            self.0.set(self.0.get() + d);
        }
    }

    /// Events at given delays (after the previous one); a wait shorter
    /// than the next delay times out. An empty script means closed.
    struct Script {
        clock: Clock,
        items: VecDeque<(Duration, Next)>,
    }

    impl EventSource for Script {
        fn next(&mut self, timeout: Option<Duration>) -> Result<Next> {
            let Some((delay, _)) = self.items.front_mut() else {
                return Ok(Next::Closed);
            };
            match timeout {
                Some(t) if t < *delay => {
                    *delay -= t;
                    self.clock.advance(t);
                    Ok(Next::Tick)
                }
                _ => {
                    let (d, n) = self.items.pop_front().unwrap();
                    self.clock.advance(d);
                    Ok(n)
                }
            }
        }
    }

    struct Rec {
        clock: Clock,
        start: Instant,
        /// (seconds since start, names) per hotplug call.
        hotplugs: Vec<(u64, Vec<String>)>,
        reloads: Vec<u64>,
        pauses: usize,
        result: Option<bool>,
    }

    impl Host for Rec {
        fn now(&self) -> Instant {
            self.clock.0.get()
        }
        fn power_changed(&mut self) -> bool {
            false
        }
        fn hotplug(&mut self, added: &[String]) -> Option<bool> {
            let t = self.now().duration_since(self.start).as_secs();
            self.hotplugs.push((t, added.to_vec()));
            self.result
        }
        fn reload(&mut self) {
            self.reloads
                .push(self.now().duration_since(self.start).as_secs());
        }
        fn paused(&mut self) {
            self.pauses += 1;
        }
    }

    fn run_script(script: Vec<(u64, Next)>, result: Option<bool>) -> Rec {
        let start = Instant::now();
        let clock = Clock(Rc::new(Cell::new(start)));
        let mut src = Script {
            clock: clock.clone(),
            items: script
                .into_iter()
                .map(|(ms, n)| (Duration::from_millis(ms), n))
                .collect(),
        };
        let mut rec = Rec {
            clock,
            start,
            hotplugs: vec![],
            reloads: vec![],
            pauses: 0,
            result,
        };
        event_loop(&mut src, &mut rec, false).unwrap();
        rec
    }

    fn added(n: &str) -> Next {
        Next::Event(Event::MonitorAdded { name: n.into() })
    }

    #[test]
    fn applies_at_start_and_after_debounce() {
        let r = run_script(vec![(5000, added("DP-2")), (60_000, Next::Closed)], None);
        assert_eq!(r.hotplugs.len(), 2);
        assert_eq!(r.hotplugs[0], (0, vec![]));
        // 1.5 s debounce after the event at 5 s.
        assert_eq!(r.hotplugs[1].1, vec!["DP-2".to_string()]);
        assert_eq!(r.hotplugs[1].0, 6);
    }

    #[test]
    fn bursts_coalesce_into_one_call() {
        let r = run_script(
            vec![
                (5000, added("DP-2")),
                (500, added("DP-3")),
                (
                    500,
                    Next::Event(Event::MonitorRemoved {
                        name: "HDMI-A-1".into(),
                    }),
                ),
                (60_000, Next::Closed),
            ],
            None,
        );
        assert_eq!(r.hotplugs.len(), 2);
        assert_eq!(
            r.hotplugs[1].1,
            vec!["DP-2".to_string(), "DP-3".to_string()]
        );
    }

    #[test]
    fn reload_is_debounced() {
        let reload = || Next::Event(Event::ConfigReloaded);
        let r = run_script(
            vec![(5000, reload()), (100, reload()), (60_000, Next::Closed)],
            None,
        );
        assert_eq!(r.reloads, vec![5]);
    }

    #[test]
    fn too_many_automatic_changes_pause_the_rules() {
        // Every call changes something: after BURST (3) within 30 s the
        // rules stop for 60 s; a later one works again.
        let r = run_script(
            vec![
                (5000, added("A-1")),
                (5000, added("A-2")),
                (5000, added("A-3")),
                (5000, added("A-4")),
                (100_000, added("A-5")),
                (60_000, Next::Closed),
            ],
            Some(true),
        );
        assert_eq!(r.pauses, 1);
        // start + A-1 + A-2 hit the burst limit; A-3 and A-4 are ignored.
        let called: Vec<_> = r.hotplugs.iter().map(|h| h.1.clone()).collect();
        assert!(called.contains(&vec!["A-2".to_string()]));
        assert!(!called.contains(&vec!["A-3".to_string()]));
        assert!(called.contains(&vec!["A-5".to_string()]));
    }
}
