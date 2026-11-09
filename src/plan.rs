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
            .unwrap(),
    };
    Ok((*chosen).clone())
}

pub fn plan_set(mons: &[Monitor], target: &str, a: &SetArgs) -> Result<Vec<MonitorSpec>> {
    let m = resolve(mons, target)?;
    if a.on == Some(false) {
        let mut s = MonitorSpec::from_monitor(m);
        s.enabled = false;
        return Ok(vec![s]);
    }
    let mut s = base_spec(m);
    if !m.enabled && a.pos.is_none() {
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
    let ti = specs.iter().position(|s| s.name == t.name).unwrap();

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
        _ => unreachable!(),
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
            let cur = mons.iter().find(|m| m.name == s.name).unwrap();
            !crate::apply::satisfies(cur, s)
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
        let s = plan_set(
            &m,
            "DP-1",
            &SetArgs {
                hz: Some(165.0),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!((s[0].width, s[0].refresh), (2560, 165.0));
        let s = plan_set(
            &m,
            "DP-1",
            &SetArgs {
                res: Some((1920, 1080)),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!((s[0].width, s[0].refresh), (1920, 119.88));
        let s = plan_set(
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
            plan_set(
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
        let s = plan_set(
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
        let s = plan_set(
            &m,
            "eDP-1",
            &SetArgs {
                pos: Some((2000, 0)),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(check_result(&m, &s).unwrap().len(), 1);
    }
}
