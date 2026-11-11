//! Core commands: each takes plain arguments and returns a serializable
//! result; nothing here prints or touches argv. `main.rs` parses, calls and
//! hands the result to the one renderer ([`Render`]).

use serde::Serialize;

use crate::apply::{Ctx, Outcome, State};
use crate::backend::{Backend, Caps};
use crate::error::Result;
use crate::memory;
use crate::model::Monitor;
use crate::plan::{self, Layout, SetArgs};
use crate::spec;

/// A result with a human form. JSON output is the `Serialize` form.
pub trait Render: Serialize {
    /// Plain text, lines joined by `\n`, no trailing newline.
    fn human(&self) -> String;
}

#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct Monitors(pub Vec<Monitor>);

pub fn monitors(b: &dyn Backend) -> Result<Monitors> {
    Ok(Monitors(b.monitors()?))
}

impl Render for Monitors {
    fn human(&self) -> String {
        self.0
            .iter()
            .map(|m| {
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
                format!("{focus} {:<8} {:<32} {state}", m.name, m.description)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Field order is the JSON key order (kept alphabetical, as before).
#[derive(Debug, Serialize)]
pub struct StatusData {
    pub active_mode: Option<String>,
    pub daemon: bool,
    pub pending: Option<crate::apply::PendingInfo>,
    pub warnings: Vec<String>,
}

pub fn status(ctx: &Ctx) -> Result<StatusData> {
    let warnings = {
        let _l = ctx.lock()?;
        ctx.sweep()
    };
    Ok(StatusData {
        pending: ctx.pending_info(),
        active_mode: ctx.active_mode(),
        daemon: crate::daemon::running(&ctx.state_dir),
        warnings,
    })
}

impl Render for StatusData {
    fn human(&self) -> String {
        let mut out = match &self.pending {
            Some(p) => format!(
                "pending: {} (reverts in {:.0} s unless `stance confirm`)",
                p.label,
                p.remaining_ms as f64 / 1000.0
            ),
            None => "pending: none".to_string(),
        };
        out += &format!(
            "\nactive mode: {}\ndaemon: {}",
            self.active_mode.as_deref().unwrap_or("none"),
            if self.daemon {
                "running"
            } else {
                "not running"
            }
        );
        for w in &self.warnings {
            out += &format!("\nnote: {w}");
        }
        out
    }
}

#[derive(Debug, Serialize)]
pub struct CapsData {
    pub backend: String,
    pub caps: Caps,
}

pub fn caps(b: &dyn Backend) -> CapsData {
    CapsData {
        backend: b.name().to_string(),
        caps: b.capabilities(),
    }
}

impl Render for CapsData {
    fn human(&self) -> String {
        let mut out = format!("backend {}", self.backend);
        let v = serde_json::to_value(self.caps).unwrap_or_default();
        for (k, on) in v.as_object().into_iter().flatten() {
            out += &format!("\n  {k:<19} {}", if on == true { "yes" } else { "no" });
        }
        out
    }
}

impl Render for Outcome {
    fn human(&self) -> String {
        let mut out = if self.changed.is_empty() {
            format!("{}: already in place, nothing changed", self.label)
        } else {
            format!("{}: changed {}", self.label, self.changed.join(", "))
        };
        if let Some(p) = &self.pending {
            out += &format!(
                "\nkeep it with `stance confirm` within {:.0} s, otherwise it reverts",
                p.remaining_ms as f64 / 1000.0
            );
        }
        for w in &self.warnings {
            out += &format!("\nnote: {w}");
        }
        out
    }
}

/// `{done, message}` of confirm / revert.
#[derive(Debug, Serialize)]
pub struct Done {
    pub done: bool,
    pub message: String,
}

impl Render for Done {
    fn human(&self) -> String {
        self.message.clone()
    }
}

pub fn confirm(ctx: &Ctx) -> Result<Done> {
    let done = ctx.confirm()?;
    Ok(Done {
        done,
        message: if done {
            "confirmed"
        } else {
            "nothing to confirm"
        }
        .into(),
    })
}

pub fn revert(ctx: &Ctx) -> Result<Done> {
    let done = ctx.revert()?;
    Ok(Done {
        done,
        message: if done {
            "reverted"
        } else {
            "nothing to revert"
        }
        .into(),
    })
}

fn only_monitors(specs: Vec<spec::MonitorSpec>) -> State {
    State {
        monitors: specs,
        ..Default::default()
    }
}

pub fn set(ctx: &Ctx, target: &str, args: &SetArgs, timeout: u64) -> Result<Outcome> {
    let mons = ctx.backend.monitors()?;
    let remembered = plan::resolve(&mons, target)
        .ok()
        .filter(|m| !m.enabled)
        .and_then(|m| memory::recall(&ctx.state_dir, m, &mons));
    let specs = plan::plan_set(&mons, target, args, remembered)?;
    let warnings = plan::check_result(&mons, &specs)?;
    let label = format!("set {}", specs[0].name);
    let mut out = ctx.apply(&label, &only_monitors(specs), timeout, Some((None, false)))?;
    out.warnings.extend(warnings);
    Ok(out)
}

pub fn layout(
    ctx: &Ctx,
    kind: Layout,
    kind_name: &str,
    target: &str,
    reference: Option<&str>,
    timeout: u64,
) -> Result<Outcome> {
    let mons = ctx.backend.monitors()?;
    let specs = plan::plan_layout(&mons, kind, target, reference)?;
    let warnings = plan::check_result(&mons, &specs)?;
    let mut out = ctx.apply(
        &format!("layout {kind_name}"),
        &only_monitors(specs),
        timeout,
        Some((None, false)),
    )?;
    out.warnings.extend(warnings);
    Ok(out)
}

pub fn arrange(ctx: &Ctx, pairs: &[(String, (i32, i32))], timeout: u64) -> Result<Outcome> {
    let mons = ctx.backend.monitors()?;
    let (specs, warnings) = plan::plan_arrange(&mons, pairs)?;
    let mut out = ctx.apply(
        "arrange",
        &only_monitors(specs),
        timeout,
        Some((None, false)),
    )?;
    out.warnings.extend(warnings);
    Ok(out)
}

pub fn lid(ctx: &Ctx, close: bool) -> Result<Outcome> {
    let mons = ctx.backend.monitors()?;
    let (specs, note) =
        plan::plan_lid(&mons, close, &|m| memory::recall(&ctx.state_dir, m, &mons))?;
    // Nobody can confirm with the lid shut: no timer, mode kept.
    let what = if close { "close" } else { "open" };
    let mut out = ctx.apply(&format!("lid {what}"), &only_monitors(specs), 0, None)?;
    out.warnings.extend(note);
    Ok(out)
}
