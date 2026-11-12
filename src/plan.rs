//! Turns user intent (match strings, `set` flags, layouts) into concrete
//! monitor specs, and checks the result is a usable desktop.

use crate::error::{Error, Result};
use crate::model::{Mode, Monitor, sanitize};
use crate::spec::MonitorSpec;

/// Resolves a user match string to one connected monitor. Order: exact
/// description, then "make model", then connector name.
pub fn resolve<'a>(mons: &'a [Monitor], q: &str) -> Result<&'a Monitor> {
    let q = q.trim();
    let tiers: [&dyn Fn(&Monitor) -> bool; 3] = [
        &|m| !m.description.is_empty() && m.description == q,
        &|m| !m.make.is_empty() && format!("{} {}", m.make, m.model) == q,
        &|m| m.name == q,
    ];
    for tier in tiers {
        let hits: Vec<&Monitor> = mons.iter().filter(|m| tier(m)).collect();
        match hits.len() {
            0 => continue,
            1 => return Ok(hits[0]),
            _ => {
                return Err(Error::new(
                    "ambiguous",
                    format!(
                        "`{}` matches several monitors; use the connector name ({})",
                        sanitize(q),
                        hits.iter()
                            .map(|m| m.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ));
            }
        }
    }
    Err(Error::new(
        "unknown_monitor",
        format!(
            "no connected monitor matches `{}` (known: {})",
            sanitize(q),
            mons.iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ))
}

/// The mode Hyprland would pick: first listed resolution, highest refresh.
pub fn preferred(m: &Monitor) -> Option<Mode> {
    let first = m.modes.first()?;
    m.modes
        .iter()
        .filter(|x| x.width == first.width && x.height == first.height)
        .max_by(|a, b| a.refresh.total_cmp(&b.refresh))
        .cloned()
}

/// Spec for `m`, enabling it with its preferred mode if it is off.
fn base_spec(m: &Monitor) -> MonitorSpec {
    let mut s = MonitorSpec::from_monitor(m);
    if !m.enabled || s.width == 0 || s.height == 0 {
        if let Some(p) = preferred(m) {
            (s.width, s.height, s.refresh) = (p.width, p.height, p.refresh);
        }
        if s.scale <= 0.0 {
            s.scale = 1.0;
        }
    }
    s.enabled = true;
    s
}

#[derive(Debug, Default, Clone)]
pub struct SetArgs {
    pub res: Option<(u32, u32)>,
    pub hz: Option<f64>,
    pub scale: Option<f64>,
    pub pos: Option<(i32, i32)>,
    pub vrr: Option<u8>,
    pub transform: Option<u8>,
    pub on: Option<bool>,
    /// Allow a mode the monitor does not advertise.
    pub custom: bool,
}

pub fn parse_res(s: &str) -> Result<(u32, u32)> {
    let (w, h) = s
        .split_once('x')
        .ok_or_else(|| Error::new("usage", "resolution must look like 2560x1440"))?;
    match (w.parse(), h.parse()) {
        (Ok(w), Ok(h)) => Ok((w, h)),
        _ => Err(Error::new("usage", "resolution must look like 2560x1440")),
    }
}

pub fn parse_pos(s: &str) -> Result<(i32, i32)> {
    let (x, y) = s
        .split_once('x')
        .ok_or_else(|| Error::new("usage", "position must look like 1920x0"))?;
    match (x.parse(), y.parse()) {
        (Ok(x), Ok(y)) => Ok((x, y)),
        _ => Err(Error::new("usage", "position must look like 1920x0")),
    }
}

/// Picks the advertised mode for a resolution / refresh request.
pub fn pick_mode(m: &Monitor, res: (u32, u32), hz: Option<f64>, custom: bool) -> Result<Mode> {
    let at_res: Vec<&Mode> = m
        .modes
        .iter()
        .filter(|x| (x.width, x.height) == res)
        .collect();
    if custom || m.modes.is_empty() {
        let refresh = hz
            .or_else(|| at_res.iter().map(|x| x.refresh).max_by(f64::total_cmp))
            .unwrap_or(m.refresh.max(60.0));
        return Ok(Mode {
            width: res.0,
            height: res.1,
            refresh,
        });
    }
    if at_res.is_empty() {
        return Err(Error::new(
            "unsupported_mode",
            format!(
                "{} does not advertise {}x{} (use --custom to force)",
                m.name, res.0, res.1
            ),
        ));
    }
    let chosen = match hz {
        Some(hz) => at_res
            .iter()
            .filter(|x| (x.refresh - hz).abs() < 0.6)
            .min_by(|a, b| (a.refresh - hz).abs().total_cmp(&(b.refresh - hz).abs()))
            .ok_or_else(|| {
                let list: Vec<String> = at_res
                    .iter()
                    .map(|x| crate::spec::fmt_num(x.refresh))
                    .collect();
                Error::new(
                    "unsupported_mode",
                    format!(
                        "{} supports {}x{} at {} Hz (use --custom to force)",
                        m.name,
                        res.0,
                        res.1,
                        list.join(", ")
                    ),
                )
            })?,
        None => at_res
            .iter()
            .max_by(|a, b| a.refresh.total_cmp(&b.refresh))
            .ok_or_else(|| Error::new("unsupported_mode", format!("{} has no modes", m.name)))?,
    };
    Ok((*chosen).clone())
}

/// `remembered`: the monitor's last known setup (see `memory`), used when a
/// disabled monitor is turned back on without an explicit position.
pub fn plan_set(
    mons: &[Monitor],
    target: &str,
    a: &SetArgs,
    remembered: Option<MonitorSpec>,
) -> Result<Vec<MonitorSpec>> {
    let m = resolve(mons, target)?;
    if a.on == Some(false) {
        let mut s = MonitorSpec::from_monitor(m);
        s.enabled = false;
        return Ok(vec![s]);
    }
    let mut s = base_spec(m);
    if let Some(r) = remembered.filter(|_| !m.enabled && a.pos.is_none()) {
        s = r;
    } else if !m.enabled && a.pos.is_none() {
        // Newly enabled: put it right of the rightmost active monitor.
        let right = mons
            .iter()
            .filter(|o| o.enabled && o.name != m.name && o.mirror_of.is_none())
            .map(|o| {
                let os = MonitorSpec::from_monitor(o);
                os.x + os.logical_size().0
            })
            .max()
            .unwrap_or(0);
        (s.x, s.y) = (right, 0);
    }
    if a.res.is_some() || a.hz.is_some() {
        let res = a.res.unwrap_or((s.width, s.height));
        let hz = a.hz.or(if a.res.is_some() {
            None
        } else {
            Some(s.refresh)
        });
        let mode = pick_mode(m, res, hz, a.custom)?;
        (s.width, s.height, s.refresh) = (mode.width, mode.height, mode.refresh);
    }
    if let Some(v) = a.scale {
        s.scale = v;
    }
    if let Some((x, y)) = a.pos {
        (s.x, s.y) = (x, y);
    }
    if let Some(v) = a.vrr {
        s.vrr = v;
    }
    if let Some(t) = a.transform {
        s.transform = t;
    }
    s.validate()?;
    Ok(vec![s])
}

/// Moves several monitors at once (`NAME=XxY` pairs, e.g. from dragging on
/// the panel map). The result must not overlap; gaps are allowed (Hyprland
/// copes) but reported, since they usually mean a snap was missed.
pub fn plan_arrange(
    mons: &[Monitor],
    pairs: &[(String, (i32, i32))],
) -> Result<(Vec<MonitorSpec>, Vec<String>)> {
    if pairs.is_empty() || pairs.len() > 16 {
        return Err(Error::new("usage", "arrange needs 1-16 NAME=XxY pairs"));
    }
    let mut specs: Vec<MonitorSpec> = Vec::new();
    for (q, (x, y)) in pairs {
        let m = resolve(mons, q)?;
        if !m.enabled || m.mirror_of.is_some() {
            return Err(Error::new(
                "usage",
                format!(
                    "{} is off or mirrored; it has no position of its own",
                    m.name
                ),
            ));
        }
        if specs.iter().any(|s| s.name == m.name) {
            return Err(Error::new("usage", format!("{} given twice", m.name)));
        }
        let mut s = MonitorSpec::from_monitor(m);
        (s.x, s.y) = (*x, *y);
        s.validate()?;
        specs.push(s);
    }
    let all: Vec<MonitorSpec> = mons
        .iter()
        .filter(|m| m.enabled && m.mirror_of.is_none())
        .map(|m| {
            specs
                .iter()
                .find(|s| s.name == m.name)
                .cloned()
                .unwrap_or_else(|| MonitorSpec::from_monitor(m))
        })
        .collect();
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            if crate::memory::overlaps(a, b) {
                return Err(Error::new(
                    "overlap",
                    format!("{} and {} would overlap", a.name, b.name),
                ));
            }
        }
    }
    let mut warnings = Vec::new();
    if all.len() > 1 {
        for a in &all {
            let touches = all.iter().any(|b| b.name != a.name && touching(a, b));
            if !touches {
                warnings.push(format!("{} does not touch any other screen", a.name));
            }
        }
    }
    specs.retain(|s| {
        let cur = mons.iter().find(|m| m.name == s.name);
        cur.is_some_and(|m| (m.x, m.y) != (s.x, s.y))
    });
    Ok((specs, warnings))
}

fn touching(a: &MonitorSpec, b: &MonitorSpec) -> bool {
    let (aw, ah) = a.logical_size();
    let (bw, bh) = b.logical_size();
    let v = a.y < b.y + bh && b.y < a.y + ah;
    let h = a.x < b.x + bw && b.x < a.x + aw;
    (v && (a.x + aw == b.x || b.x + bw == a.x)) || (h && (a.y + ah == b.y || b.y + bh == a.y))
}

/// Lid closed: turn the laptop panel(s) off, but only when another screen
/// stays on. Lid open: turn them back on (at their remembered place when
/// `recall` knows it). Returns the changes and a note for the log.
pub fn plan_lid(
    mons: &[Monitor],
    close: bool,
    recall: &dyn Fn(&Monitor) -> Option<MonitorSpec>,
) -> Result<(Vec<MonitorSpec>, Option<String>)> {
    let internal = |m: &&Monitor| crate::brightness::is_internal(&m.name);
    if close {
        let panels: Vec<&Monitor> = mons.iter().filter(internal).filter(|m| m.enabled).collect();
        if panels.is_empty() {
            return Ok((vec![], Some("no laptop panel is on".into())));
        }
        let others = mons
            .iter()
            .filter(|m| {
                !crate::brightness::is_internal(&m.name) && m.enabled && m.mirror_of.is_none()
            })
            .count();
        if others == 0 {
            return Ok((
                vec![],
                Some("kept the laptop panel on: it is the only screen".into()),
            ));
        }
        let specs = panels
            .into_iter()
            .map(|m| {
                let mut s = MonitorSpec::from_monitor(m);
                s.enabled = false;
                s
            })
            .collect();
        return Ok((specs, None));
    }
    let mut specs = Vec::new();
    for m in mons.iter().filter(internal).filter(|m| !m.enabled) {
        specs.extend(plan_set(
            mons,
            &m.name,
            &SetArgs {
                on: Some(true),
                ..Default::default()
            },
            recall(m),
        )?);
    }
    let note = specs
        .is_empty()
        .then(|| "laptop panel is already on".to_string());
    Ok((specs, note))
}

pub fn parse_pair(s: &str) -> Result<(String, (i32, i32))> {
    let (name, pos) = s.rsplit_once('=').ok_or_else(|| {
        Error::new(
            "usage",
            format!("`{}` is not NAME=XxY", crate::model::sanitize(s)),
        )
    })?;
    Ok((name.to_string(), parse_pos(pos)?))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    LeftOf,
    RightOf,
    Above,
    Below,
    Mirror,
    Single,
}

impl Layout {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "left-of" => Layout::LeftOf,
            "right-of" => Layout::RightOf,
            "above" => Layout::Above,
            "below" => Layout::Below,
            "mirror" => Layout::Mirror,
            "single" => Layout::Single,
            _ => return None,
        })
    }

    pub fn needs_reference(self) -> bool {
        self != Layout::Single
    }
}

pub fn plan_layout(
    mons: &[Monitor],
    kind: Layout,
    target: &str,
    reference: Option<&str>,
) -> Result<Vec<MonitorSpec>> {
    let t = resolve(mons, target)?;
    let mut specs: Vec<MonitorSpec> = mons.iter().map(MonitorSpec::from_monitor).collect();
    let ti = specs
        .iter()
        .position(|s| s.name == t.name)
        .ok_or_else(|| Error::new("internal", format!("{} vanished", t.name)))?;

    if kind == Layout::Single {
        for s in specs.iter_mut() {
            if s.name == t.name {
                *s = MonitorSpec {
                    x: 0,
                    y: 0,
                    mirror: None,
                    ..base_spec(t)
                };
            } else {
                s.enabled = false;
            }
        }
        return Ok(specs);
    }

    let r = resolve(
        mons,
        reference.ok_or_else(|| Error::new("usage", "this layout needs a reference monitor"))?,
    )?;
    if r.name == t.name {
        return Err(Error::new("usage", "target and reference must differ"));
    }
    if !r.enabled {
        return Err(Error::new("usage", format!("reference {} is off", r.name)));
    }
    let rs = MonitorSpec::from_monitor(r);
    let mut ts = MonitorSpec {
        mirror: None,
        ..base_spec(t)
    };
    if kind == Layout::Mirror {
        ts.mirror = Some(r.name.clone());
        specs[ti] = ts;
        return Ok(vec![specs[ti].clone()]);
    }
    let (tw, th) = ts.logical_size();
    let (rw, rh) = rs.logical_size();
    (ts.x, ts.y) = match kind {
        Layout::LeftOf => (rs.x - tw, rs.y),
        Layout::RightOf => (rs.x + rw, rs.y),
        Layout::Above => (rs.x, rs.y - th),
        Layout::Below => (rs.x, rs.y + rh),
        Layout::Single | Layout::Mirror => {
            return Err(Error::new("internal", "layout handled above"));
        }
    };
    specs[ti] = ts;

    // Keep the layout anchored at 0,0 (negative coordinates confuse some apps).
    let active = |s: &&mut MonitorSpec| s.enabled && s.mirror.is_none();
    let min_x = specs
        .iter_mut()
        .filter(active)
        .map(|s| s.x)
        .min()
        .unwrap_or(0);
    let min_y = specs
        .iter_mut()
        .filter(active)
        .map(|s| s.y)
        .min()
        .unwrap_or(0);
    for s in specs.iter_mut().filter(|s| s.enabled && s.mirror.is_none()) {
        s.x -= min_x;
        s.y -= min_y;
    }
    // Only return what actually changes.
    Ok(specs
        .into_iter()
        .filter(|s| {
            mons.iter()
                .find(|m| m.name == s.name)
                .is_none_or(|cur| !crate::apply::satisfies(cur, s))
        })
        .collect())
}

/// Refuses plans that would leave no usable screen; returns warnings for
/// overlapping screens (allowed, but usually a mistake).
pub fn check_result(mons: &[Monitor], changes: &[MonitorSpec]) -> Result<Vec<String>> {
    let final_specs: Vec<MonitorSpec> = mons
        .iter()
        .map(|m| {
            changes
                .iter()
                .find(|c| c.name == m.name)
                .cloned()
                .unwrap_or_else(|| MonitorSpec::from_monitor(m))
        })
        .collect();
    let active: Vec<&MonitorSpec> = final_specs
        .iter()
        .filter(|s| s.enabled && s.mirror.is_none())
        .collect();
    if active.is_empty() {
        return Err(Error::new("no_screen", "this would turn off every screen"));
    }
    for s in final_specs.iter().filter(|s| s.enabled) {
        if let Some(src) = &s.mirror
            && !active.iter().any(|a| &a.name == src)
        {
            return Err(Error::new(
                "bad_mirror",
                format!("{} would mirror a screen that is off", s.name),
            ));
        }
    }
    let mut warnings = Vec::new();
    for c in changes.iter().filter(|c| c.enabled && c.mirror.is_none()) {
        let clean = |px: u32| {
            let v = f64::from(px) / c.scale;
            (v - v.round()).abs() < 0.01
        };
        if !clean(c.width) || !clean(c.height) {
            warnings.push(format!(
                "scale {} does not divide {}x{} evenly; Hyprland will snap it to the nearest scale that does",
                crate::spec::fmt_num(c.scale),
                c.width,
                c.height
            ));
        }
    }
    for (i, a) in active.iter().enumerate() {
        for b in &active[i + 1..] {
            let (aw, ah) = a.logical_size();
            let (bw, bh) = b.logical_size();
            let overlap = a.x < b.x + bw && b.x < a.x + aw && a.y < b.y + bh && b.y < a.y + ah;
            if overlap {
                warnings.push(format!(
                    "{} and {} overlap; set a position or use `stance layout`",
                    a.name, b.name
                ));
            }
        }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set3(m: &[Monitor], t: &str, a: &SetArgs) -> Result<Vec<MonitorSpec>> {
        plan_set(m, t, a, None)
    }

    #[test]
    fn lid_rules() {
        use crate::apply::tests::mon;
        let mut lap = mon("eDP-1", 0);
        let ext = mon("DP-1", 1920);
        let none = |_: &Monitor| None;
        // Closing with an external screen turns the panel off.
        let (s, n) = plan_lid(&[lap.clone(), ext.clone()], true, &none).unwrap();
        assert!(n.is_none() && s.len() == 1 && !s[0].enabled && s[0].name == "eDP-1");
        // Alone: never turn off the last screen.
        let (s, n) = plan_lid(std::slice::from_ref(&lap), true, &none).unwrap();
        assert!(s.is_empty() && n.is_some());
        // Opening turns it back on, at the remembered place if known.
        lap.enabled = false;
        let mut at = MonitorSpec::from_monitor(&mon("eDP-1", -1920));
        at.enabled = true;
        let rec = move |_: &Monitor| Some(at.clone());
        let (s, _) = plan_lid(&[lap.clone(), ext.clone()], false, &rec).unwrap();
        assert!(s[0].enabled && s[0].x == -1920);
        let (s, n) = plan_lid(&[mon("eDP-1", 0), ext], false, &none).unwrap();
        assert!(s.is_empty() && n.is_some());
    }

    #[test]
    fn arrange_checks_overlap_and_touch() {
        use crate::apply::tests::mon;
        let mons = vec![mon("A-1", 0), mon("B-1", 1920)];
        let (s, w) = plan_arrange(&mons, &[("B-1".into(), (-1920, 0))]).unwrap();
        assert_eq!((s[0].x, s.len()), (-1920, 1));
        assert!(w.is_empty());
        assert_eq!(
            plan_arrange(&mons, &[("B-1".into(), (100, 0))])
                .unwrap_err()
                .code,
            "overlap"
        );
        let (_, w) = plan_arrange(&mons, &[("B-1".into(), (3000, 0))]).unwrap();
        assert_eq!(w.len(), 2);
        // Unchanged positions are dropped.
        let (s, _) = plan_arrange(&mons, &[("A-1".into(), (0, 0))]).unwrap();
        assert!(s.is_empty());
        assert_eq!(parse_pair("DP-1=10x-5").unwrap(), ("DP-1".into(), (10, -5)));
        assert!(parse_pair("DP-1").is_err());
    }
    use crate::apply::tests::mon;

    fn two() -> Vec<Monitor> {
        let mut a = mon("DP-1", 1920);
        a.description = "JDR EPIC Q40".into();
        a.make = "JDR".into();
        a.model = "EPIC Q40".into();
        a.width = 2560;
        a.height = 1440;
        a.refresh = 180.0;
        a.modes = vec![
            Mode {
                width: 2560,
                height: 1440,
                refresh: 59.95,
            },
            Mode {
                width: 2560,
                height: 1440,
                refresh: 180.0,
            },
            Mode {
                width: 2560,
                height: 1440,
                refresh: 165.0,
            },
            Mode {
                width: 1920,
                height: 1080,
                refresh: 119.88,
            },
            Mode {
                width: 1920,
                height: 1080,
                refresh: 60.0,
            },
        ];
        let mut b = mon("eDP-1", 0);
        b.description = "AU Optronics 0x7EAD 0x00007EAD".into();
        b.make = "AU Optronics".into();
        b.model = "0x7EAD".into();
        b.y = 360;
        vec![a, b]
    }

    #[test]
    fn resolve_tiers() {
        let m = two();
        assert_eq!(resolve(&m, "JDR EPIC Q40").unwrap().name, "DP-1");
        assert_eq!(resolve(&m, "AU Optronics 0x7EAD").unwrap().name, "eDP-1");
        assert_eq!(resolve(&m, "eDP-1").unwrap().name, "eDP-1");
        assert_eq!(resolve(&m, "nope").unwrap_err().code, "unknown_monitor");
    }

    #[test]
    fn set_picks_advertised_modes() {
        let m = two();
        let s = set3(
            &m,
            "DP-1",
            &SetArgs {
                hz: Some(165.0),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!((s[0].width, s[0].refresh), (2560, 165.0));
        let s = set3(
            &m,
            "DP-1",
            &SetArgs {
                res: Some((1920, 1080)),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!((s[0].width, s[0].refresh), (1920, 119.88));
        let s = set3(
            &m,
            "DP-1",
            &SetArgs {
                hz: Some(60.0),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(s[0].refresh, 59.95);
        assert_eq!(
            set3(
                &m,
                "DP-1",
                &SetArgs {
                    hz: Some(144.0),
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            "unsupported_mode"
        );
        let s = set3(
            &m,
            "DP-1",
            &SetArgs {
                hz: Some(144.0),
                custom: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(s[0].refresh, 144.0);
    }

    #[test]
    fn layouts() {
        let m = two();
        // eDP-1 right of DP-1 → DP-1 shifts to 0, eDP-1 at 2560.
        let s = plan_layout(&m, Layout::RightOf, "eDP-1", Some("DP-1")).unwrap();
        let dp = s.iter().find(|s| s.name == "DP-1").unwrap();
        let ed = s.iter().find(|s| s.name == "eDP-1").unwrap();
        assert_eq!((dp.x, dp.y), (0, 0));
        assert_eq!((ed.x, ed.y), (2560, 0));
        check_result(&m, &s).unwrap();

        let s = plan_layout(&m, Layout::Above, "eDP-1", Some("DP-1")).unwrap();
        let ed = s.iter().find(|s| s.name == "eDP-1").unwrap();
        let dp = s.iter().find(|s| s.name == "DP-1").unwrap();
        assert_eq!((ed.x, ed.y, dp.y), (0, 0, 1080));

        let s = plan_layout(&m, Layout::Single, "DP-1", None).unwrap();
        assert!(
            s.iter()
                .find(|s| s.name == "eDP-1")
                .is_some_and(|s| !s.enabled)
        );
        check_result(&m, &s).unwrap();

        let s = plan_layout(&m, Layout::Mirror, "eDP-1", Some("DP-1")).unwrap();
        assert_eq!(s[0].mirror.as_deref(), Some("DP-1"));
        check_result(&m, &s).unwrap();
    }

    #[test]
    fn safety_checks() {
        let m = two();
        let off: Vec<MonitorSpec> = m
            .iter()
            .map(|x| MonitorSpec {
                enabled: false,
                ..MonitorSpec::from_monitor(x)
            })
            .collect();
        assert_eq!(check_result(&m, &off).unwrap_err().code, "no_screen");
        let s = set3(
            &m,
            "eDP-1",
            &SetArgs {
                pos: Some((2000, 0)),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(check_result(&m, &s).unwrap().len(), 1);
        // 2560x1440 at 1.5 → 1706.67: not clean.
        let s = set3(
            &m,
            "DP-1",
            &SetArgs {
                scale: Some(1.5),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            check_result(&m, &s)
                .unwrap()
                .iter()
                .any(|w| w.contains("snap"))
        );
        let s = set3(
            &m,
            "DP-1",
            &SetArgs {
                scale: Some(1.25),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(check_result(&m, &s).unwrap().is_empty());
    }
}
