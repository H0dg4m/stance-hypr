//! In-memory compositor for unit tests and `--backend fake`.
//!
//! With a state file (`<state dir>/fake.json`, or `STANCE_FAKE_STATE`) the
//! screens persist between CLI runs, so integration tests can drive the real
//! binary without a compositor. The file is seeded by hand or created with
//! the default two screens on the first change.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{Backend, Caps, Notice, OptKey, OptValue};
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
    #[serde(default)]
    caps: Caps,
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
            caps: Caps::default(),
            path: None,
        }
    }

    /// Loads the state file (default seed when it does not exist yet);
    /// every change is written back.
    pub fn open(path: PathBuf) -> Result<Self> {
        let mut f = Fake::new();
        if path.exists() {
            let text = store::read_checked(&path)?;
            let s: Saved = serde_json::from_str(&text)
                .map_err(|e| Error::new("bad_config", format!("{}: {e}", path.display())))?;
            f.mons = RefCell::new(s.monitors);
            f.opts = RefCell::new(s.options);
            f.power = RefCell::new(s.power);
            f.ws = RefCell::new(s.workspaces);
            f.ignore = s.ignore;
            f.caps = s.caps;
        }
        f.path = Some(path);
        Ok(f)
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
        store::atomic_write(path, &data, false)
    }
}

impl Backend for Fake {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn capabilities(&self) -> Caps {
        self.caps
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
        Ok(self.mons.borrow().clone())
    }
    fn apply_monitor(&self, s: &MonitorSpec) -> Result<()> {
        s.validate()?;
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
        Ok(*self.opts.borrow().get(&k).unwrap_or(&OptValue::Bool(true)))
    }
    fn set_option(&self, k: OptKey, v: OptValue) -> Result<()> {
        self.opts.borrow_mut().insert(k, v);
        self.save()
    }
    fn workspaces(&self) -> Result<Vec<(u32, String)>> {
        Ok(self.ws.borrow().clone())
    }
    fn bind_workspace(&self, id: u32, mon: &str) -> Result<()> {
        if let Some(w) = self.ws.borrow_mut().iter_mut().find(|(w, _)| *w == id) {
            w.1 = mon.into();
        }
        self.save()
    }
    fn notify(&self, _: Notice) {}
    fn power_get(&self) -> Result<PowerProfile> {
        Ok(*self.power.borrow())
    }
    fn power_set(&self, p: PowerProfile) -> Result<()> {
        *self.power.borrow_mut() = p;
        self.save()
    }
}
