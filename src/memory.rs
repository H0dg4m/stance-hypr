//! Last known setup of each monitor, so a screen that is turned off (or
//! unplugged) and back on returns where it was instead of being appended to
//! the right. Keyed by a stable identity (description, else make + model,
//! else connector); stored as `memory.json` in the state dir.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::model::Monitor;
use crate::spec::MonitorSpec;
use crate::store;

const FILE: &str = "memory.json";
/// Old entries beyond this are dropped (oldest first).
const MAX_ENTRIES: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Entry {
    spec: MonitorSpec,
    at_ms: u64,
}

pub fn identity(m: &Monitor) -> String {
    let mm = format!("{} {}", m.make, m.model);
    if !m.description.trim().is_empty() {
        m.description.trim().to_string()
    } else if !mm.trim().is_empty() {
        mm.trim().to_string()
    } else {
        m.name.clone()
    }
}

/// Two identical screens (same model, no serial) cannot be told apart, so
/// they are neither recorded nor restored.
fn ambiguous(m: &Monitor, mons: &[Monitor]) -> bool {
    let id = identity(m);
    mons.iter().any(|o| o.name != m.name && identity(o) == id)
}

fn load(dir: &Path) -> BTreeMap<String, Entry> {
    store::read_checked(&dir.join(FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Remembers every active, non-mirrored monitor in `mons`. Best effort:
/// a failure here must never fail the change itself.
pub fn record(dir: &Path, mons: &[Monitor]) {
    record_with(dir, mons, true);
}

/// Like [`record`], but only for screens not remembered yet. Used before a
/// change: the live position may be Hyprland's default after a re-plug and
/// must not overwrite a known good one.
pub fn record_new(dir: &Path, mons: &[Monitor]) {
    record_with(dir, mons, false);
}

fn record_with(dir: &Path, mons: &[Monitor], overwrite: bool) {
    let mut map = load(dir);
    let now = crate::apply::now_ms();
    let mut changed = false;
    for m in mons
        .iter()
        .filter(|m| m.enabled && m.mirror_of.is_none() && !ambiguous(m, mons))
    {
        let spec = MonitorSpec::from_monitor(m);
        if spec.width == 0 || spec.height == 0 || spec.validate().is_err() {
            continue;
        }
        let key = identity(m);
        if map.get(&key).is_some_and(|e| e.spec == spec || !overwrite) {
            continue;
        }
        map.insert(key, Entry { spec, at_ms: now });
        changed = true;
    }
    while map.len() > MAX_ENTRIES {
        let Some(oldest) = map
            .iter()
            .min_by_key(|(_, e)| e.at_ms)
            .map(|(k, _)| k.clone())
        else {
            break;
        };
        map.remove(&oldest);
        changed = true;
    }
    if !changed || store::ensure_private_dir(dir).is_err() {
        return;
    }
    if let Ok(data) = serde_json::to_vec(&map) {
        let _ = store::atomic_write(&dir.join(FILE), &data, false);
    }
}

/// The remembered setup for `m`, renamed to its current connector, if it
/// still names one of its modes (or no mode list is known) and does not
/// overlap the other active monitors.
pub fn recall(dir: &Path, m: &Monitor, mons: &[Monitor]) -> Option<MonitorSpec> {
    if ambiguous(m, mons) {
        return None;
    }
    let mut spec = load(dir).remove(&identity(m))?.spec;
    spec.name = m.name.clone();
    spec.enabled = true;
    spec.mirror = None;
    let mode_ok = m.modes.is_empty()
        || m.modes.iter().any(|md| {
            md.width == spec.width && md.height == spec.height && (md.refresh - spec.refresh).abs() < 0.5
        });
    if !mode_ok || spec.validate().is_err() {
        return None;
    }
    let others: Vec<MonitorSpec> = mons
        .iter()
        .filter(|o| o.name != m.name && o.enabled && o.mirror_of.is_none())
        .map(MonitorSpec::from_monitor)
        .collect();
    if others.iter().any(|o| overlaps(o, &spec)) {
        return None;
    }
    Some(spec)
}

/// Do two monitors' logical rectangles intersect?
pub fn overlaps(a: &MonitorSpec, b: &MonitorSpec) -> bool {
    let (aw, ah) = a.logical_size();
    let (bw, bh) = b.logical_size();
    a.x < b.x + bw && b.x < a.x + aw && a.y < b.y + bh && b.y < a.y + ah
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::tests::mon;

    fn tmp() -> std::path::PathBuf {
        // Tests run in parallel: a counter keeps their dirs apart.
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!(
            "stance-mem-{}-{}-{n}",
            std::process::id(),
            crate::apply::now_ms()
        ));
        store::ensure_private_dir(&d).unwrap();
        d
    }

    #[test]
    fn remembers_and_restores_position() {
        let dir = tmp();
        let mut a = mon("A-1", 0);
        let mut b = mon("B-1", 1920);
        b.y = 300;
        record(&dir, &[a.clone(), b.clone()]);
        // B goes off, A stays.
        b.enabled = false;
        let got = recall(&dir, &b, &[a.clone(), b.clone()]).unwrap();
        assert_eq!((got.x, got.y), (1920, 300));
        assert_eq!(got.name, "B-1");
        // Something now sits where B was: no restore.
        a.x = 1920;
        a.y = 300;
        assert!(recall(&dir, &b, &[a, b.clone()]).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn identical_screens_are_skipped() {
        let dir = tmp();
        let a = mon("A-1", 0);
        let mut b = mon("B-1", 1920);
        b.description = a.description.clone();
        record(&dir, &[a.clone(), b.clone()]);
        assert!(load(&dir).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn identity_prefers_description() {
        let m = mon("A-1", 0);
        assert_eq!(identity(&m), m.description);
    }

    #[test]
    fn overlap_rules() {
        let a = MonitorSpec::from_monitor(&mon("A-1", 0));
        let mut b = MonitorSpec::from_monitor(&mon("B-1", 1920));
        assert!(!overlaps(&a, &b));
        b.x = 1919;
        assert!(overlaps(&a, &b));
    }
}
