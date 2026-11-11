//! In-memory compositor for unit tests and `--backend fake`.
//!
//! With a state file (`<state dir>/fake.json`, or `STANCE_FAKE_STATE`) the
//! screens persist between CLI runs, so integration tests can drive the real
//! binary without a compositor. The file is seeded by hand or created with
//! the default two screens on the first change.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{Backend, Caps, Event, EventSource, Next, Notice, OptKey, OptValue};
use crate::error::{Error, Result};
use crate::model::{Mode, Monitor, Parser};
use crate::power::PowerProfile;
use crate::spec::MonitorSpec;
use crate::store;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    monitors: Vec<Monitor>,
    #[serde(default)]
    options: BTreeMap<OptKey, OptValue>,
    #[serde(default = "balanced")]
    power: PowerProfile,
    #[serde(default)]
    workspaces: Vec<(u32, String)>,
    /// Connectors whose changes are accepted but silently not applied
    /// (like Hyprland's `eval`).
    #[serde(default)]
    ignore: Vec<String>,
    /// Capability overrides (to exercise `unsupported`).
    #[serde(default = "fake_caps")]
    caps: Caps,
}

fn fake_caps() -> Caps {
    Caps {
        events: true,
        ..Caps::default()
    }
}

fn balanced() -> PowerProfile {
    PowerProfile::Balanced
}

pub struct Fake {
    pub mons: RefCell<Vec<Monitor>>,
    pub opts: RefCell<BTreeMap<OptKey, OptValue>>,
    pub power: RefCell<PowerProfile>,
    pub ignore: Vec<String>,
    pub ws: RefCell<Vec<(u32, String)>>,
    pub caps: Caps,
    path: Option<PathBuf>,
    /// State file as last read or written (other processes change it).
    seen: Cell<Option<(std::time::SystemTime, u64)>>,
}

pub fn mon(name: &str, x: i32) -> Monitor {
    Monitor {
        name: name.into(),
        description: format!("Desc {name}"),
        make: "Make".into(),
        model: name.into(),
        serial: String::new(),
        enabled: true,
        focused: false,
        dpms: true,
        width: 1920,
        height: 1080,
        refresh: 60.0,
        x,
        y: 0,
        scale: 1.0,
        transform: 0,
        vrr: false,
        vrr_capable: false,
        mirror_of: None,
        modes: vec![Mode {
            width: 1920,
            height: 1080,
            refresh: 60.0,
        }],
    }
}

impl Fake {
    /// Two screens, blur on, one workspace; not backed by a file.
    pub fn new() -> Self {
        Fake {
            mons: RefCell::new(vec![mon("A-1", 0), mon("B-1", 1920)]),
            opts: RefCell::new(BTreeMap::from([(OptKey::Blur, OptValue::Bool(true))])),
            power: RefCell::new(PowerProfile::Balanced),
            ignore: vec![],
            ws: RefCell::new(vec![(1, "A-1".into())]),
            caps: fake_caps(),
            path: None,
            seen: Cell::new(None),
        }
    }

    /// Loads the state file (default seed when it does not exist yet);
    /// every change is written back, and changes by other processes are
    /// picked up before each read.
    pub fn open(path: PathBuf) -> Result<Self> {
        let mut f = Fake::new();
        if path.exists() {
            let s: Saved = serde_json::from_str(&store::read_checked(&path)?)
                .map_err(|e| Error::new("bad_config", format!("{}: {e}", path.display())))?;
            f.ignore = s.ignore;
            f.caps = s.caps;
        }
        f.path = Some(path);
        f.sync()?;
        Ok(f)
    }

    fn sync(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let now = stamp(path);
        if now.is_none() || now == self.seen.get() {
            return Ok(());
        }
        let text = store::read_checked(path)?;
        let s: Saved = serde_json::from_str(&text)
            .map_err(|e| Error::new("bad_config", format!("{}: {e}", path.display())))?;
        *self.mons.borrow_mut() = s.monitors;
        *self.opts.borrow_mut() = s.options;
        *self.power.borrow_mut() = s.power;
        *self.ws.borrow_mut() = s.workspaces;
        self.seen.set(now);
        Ok(())
    }

    fn save(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let s = Saved {
            monitors: self.mons.borrow().clone(),
            options: self.opts.borrow().clone(),
            power: *self.power.borrow(),
            workspaces: self.ws.borrow().clone(),
            ignore: self.ignore.clone(),
            caps: self.caps,
        };
        if let Some(dir) = path.parent() {
            store::ensure_private_dir(dir)?;
        }
        let data = serde_json::to_vec(&s).map_err(|e| Error::new("io", e.to_string()))?;
        store::atomic_write(path, &data, false)?;
        self.seen.set(stamp(path));
        Ok(())
    }
}

impl Backend for Fake {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn capabilities(&self) -> Caps {
        self.caps
    }
    fn events(&self) -> Result<Box<dyn EventSource>> {
        let path = self.path.clone().ok_or_else(|| {
            Error::new(
                "unsupported",
                "the fake backend needs a state file for events",
            )
        })?;
        Ok(Box::new(Watch {
            names: names_in(&path),
            stamp: stamp(&path),
            path,
            queue: Default::default(),
        }))
    }
    fn session_id(&self) -> &str {
        "fake"
    }
    fn version(&self) -> Result<String> {
        Ok("0.57.1".into())
    }
    fn parser(&self) -> Result<Parser> {
        Ok(Parser::Lua)
    }
    fn monitors(&self) -> Result<Vec<Monitor>> {
        self.sync()?;
        Ok(self.mons.borrow().clone())
    }
    fn apply_monitor(&self, s: &MonitorSpec) -> Result<()> {
        s.validate()?;
        self.sync()?;
        if self.ignore.contains(&s.name) {
            return Ok(()); // like eval: "ok" but nothing happens
        }
        {
            let mut mons = self.mons.borrow_mut();
            let m = mons
                .iter_mut()
                .find(|m| m.name == s.name)
                .ok_or_else(|| Error::new("compositor", format!("no monitor {}", s.name)))?;
            m.enabled = s.enabled;
            if s.enabled {
                (m.width, m.height, m.refresh) = (s.width, s.height, s.refresh);
                (m.x, m.y, m.scale, m.transform) = (s.x, s.y, s.scale, s.transform);
                m.mirror_of = s.mirror.clone();
            }
        }
        self.save()
    }
    fn get_option(&self, k: OptKey) -> Result<OptValue> {
        self.sync()?;
        Ok(*self.opts.borrow().get(&k).unwrap_or(&OptValue::Bool(true)))
    }
    fn set_option(&self, k: OptKey, v: OptValue) -> Result<()> {
        self.sync()?;
        self.opts.borrow_mut().insert(k, v);
        self.save()
    }
    fn workspaces(&self) -> Result<Vec<(u32, String)>> {
        self.sync()?;
        Ok(self.ws.borrow().clone())
    }
    fn bind_workspace(&self, id: u32, mon: &str) -> Result<()> {
        self.sync()?;
        if let Some(w) = self.ws.borrow_mut().iter_mut().find(|(w, _)| *w == id) {
            w.1 = mon.into();
        }
        self.save()
    }
    fn notify(&self, _: Notice) {}
    fn power_get(&self) -> Result<PowerProfile> {
        self.sync()?;
        Ok(*self.power.borrow())
    }
    fn power_set(&self, p: PowerProfile) -> Result<()> {
        self.sync()?;
        *self.power.borrow_mut() = p;
        self.save()
    }
}

fn stamp(path: &std::path::Path) -> Option<(std::time::SystemTime, u64)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

fn names_in(path: &std::path::Path) -> Vec<String> {
    fs_read(path)
        .and_then(|t| serde_json::from_str::<Saved>(&t).ok())
        .map(|s| s.monitors.into_iter().map(|m| m.name).collect())
        .unwrap_or_default()
}

fn fs_read(path: &std::path::Path) -> Option<String> {
    store::read_checked(path).ok()
}

/// Polls the state file (tests only, so 100 ms is fine): only screens that
/// appear or vanish become events.
struct Watch {
    path: PathBuf,
    names: Vec<String>,
    stamp: Option<(std::time::SystemTime, u64)>,
    queue: std::collections::VecDeque<Event>,
}

impl EventSource for Watch {
    fn next(&mut self, timeout: Option<std::time::Duration>) -> Result<Next> {
        let t0 = std::time::Instant::now();
        loop {
            if let Some(e) = self.queue.pop_front() {
                return Ok(Next::Event(e));
            }
            let now = stamp(&self.path);
            if now != self.stamp {
                self.stamp = now;
                let names = names_in(&self.path);
                for n in names.iter().filter(|n| !self.names.contains(n)) {
                    self.queue
                        .push_back(Event::MonitorAdded { name: n.clone() });
                }
                for n in self.names.iter().filter(|n| !names.contains(n)) {
                    self.queue
                        .push_back(Event::MonitorRemoved { name: n.clone() });
                }
                self.names = names;
                continue;
            }
            let step = std::time::Duration::from_millis(100);
            match timeout {
                Some(t) if t0.elapsed() >= t => return Ok(Next::Tick),
                Some(t) => std::thread::sleep(step.min(t.saturating_sub(t0.elapsed()))),
                None => std::thread::sleep(step),
            }
        }
    }
}
