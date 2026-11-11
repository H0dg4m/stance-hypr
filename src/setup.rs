//! `stance setup [waybar|quickshell|end4] [--dry-run]`: tells you how to
//! wire stance into your desktop. It prints steps and only ever enables
//! stance's own user units; it never edits other programs' configs.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Waybar,
    Quickshell,
    End4,
}

impl Target {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "waybar" => Ok(Self::Waybar),
            "quickshell" => Ok(Self::Quickshell),
            "end4" | "ii" => Ok(Self::End4),
            _ => Err(Error::new(
                "usage",
                format!("unknown setup target `{s}` (waybar, quickshell, end4)"),
            )),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub target: Target,
    /// Why this target was picked when none was given.
    pub detected: Option<String>,
    /// stance's own user units this run enables (or would, with --dry-run).
    pub units: Vec<&'static str>,
    /// Steps for the user, in order. Commands are shown, never run.
    pub steps: Vec<String>,
}

/// Picks a target from running processes: end4/ii shell, else Waybar, else
/// the standalone Quickshell panel.
pub fn detect(cmdlines: &[Vec<String>]) -> (Target, String) {
    let base = |a: &str| a.rsplit('/').next().unwrap_or("").to_string();
    let qs_config = cmdlines.iter().find_map(|p| {
        let first = base(p.first()?);
        if first != "qs" && first != "quickshell" {
            return None;
        }
        let i = p.iter().position(|a| a == "-c" || a == "--config")?;
        p.get(i + 1).cloned()
    });
    if let Some(c) =
        qs_config.filter(|c| c == "ii" || c.starts_with("end4") || c.contains("illogical"))
    {
        let c: String = c.chars().filter(|ch| !ch.is_control()).take(64).collect();
        return (Target::End4, format!("Quickshell config `{c}` is running"));
    }
    if cmdlines
        .iter()
        .any(|p| p.first().is_some_and(|a| base(a) == "waybar"))
    {
        return (Target::Waybar, "Waybar is running".into());
    }
    (
        Target::Quickshell,
        "no supported bar found; using the standalone panel".into(),
    )
}

/// Where the standalone panel lives: the package path, else next to the
/// running binary in a source checkout.
pub fn panel_dir() -> Option<PathBuf> {
    let pkg = PathBuf::from("/usr/share/stance/quickshell");
    if pkg.join("shell.qml").is_file() {
        return Some(pkg);
    }
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .map(|a| a.join("quickshell/stance"))
        .find(|p| p.join("shell.qml").is_file())
}

fn integration_dir(panel: Option<&Path>) -> String {
    match panel {
        Some(p) if p.starts_with("/usr/share/stance") => "/usr/share/stance/integrations".into(),
        Some(p) => p
            .parent()
            .map(|q| q.join("integrations").display().to_string())
            .unwrap_or_else(|| "<stance>/quickshell/integrations".into()),
        None => "<stance>/quickshell/integrations".into(),
    }
}

pub fn plan(target: Target, detected: Option<String>, panel: Option<&Path>) -> Plan {
    let panel_s = panel
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<stance>/quickshell/stance".into());
    let integ = integration_dir(panel);
    let ipc = format!("qs -p {panel_s} ipc call stance");
    let mut steps = Vec::new();
    let mut units = Vec::new();
    match target {
        Target::Waybar | Target::Quickshell => {
            units.push("stance-panel.service");
            steps.push(format!(
                "The panel runs as a user service (stance-panel.service: qs -p {panel_s})."
            ));
            if target == Target::Waybar {
                steps.push(
                    "Waybar: add \"custom/stance\" to a modules list and the block from \
                     /usr/share/stance/waybar/config.jsonc (contrib/waybar in a checkout); \
                     style classes in style.css next to it."
                        .into(),
                );
            }
            steps.push(format!(
                "Hyprland keys (hyprland.lua): hl.bind(\"SUPER + SHIFT + D\", hl.dsp.exec_cmd(\"{ipc} toggle\")) \
                 and hl.bind(\"SUPER + SHIFT + V\", hl.dsp.exec_cmd(\"{ipc} clip\"))"
            ));
        }
        Target::End4 => {
            steps.push(format!(
                "Bar button: ln -s {integ}/end4-pC/StanceUtilButton.qml ~/.config/quickshell/<config>/modules/ii/bar/ \
                 and add `StanceUtilButton {{}}` to modules/ii/bar/UtilButtons.qml (stance doctor shows the exact line)."
            ));
            steps.push(format!(
                "Panel components: ln -s {panel_s} ~/.config/quickshell/<config>/modules/stance"
            ));
            steps.push(format!(
                "Clipboard shelf: ln -s {integ}/end4-pC/StanceClipShelf.qml ~/.config/quickshell/<config>/modules/ii/bar/ \
                 and add `PanelLoader {{ component: StanceClipShelf {{}} }}` to panelFamilies/IllogicalImpulseFamily.qml."
            ));
            steps.push(
                "Hyprland key: hl.bind(\"SUPER + SHIFT + V\", hl.dsp.exec_cmd(\"qs -c <config> ipc call stanceclip toggle\"))"
                    .into(),
            );
        }
    }
    steps.push(
        "Optional parts (each can be turned off later with `stance feature off NAME`): \
         stance clip config put --data '{\"enabled\":true}' (clipboard shelf), \
         stance deck pair (phone), ~/.config/stance/rules.toml (hotplug rules)."
            .into(),
    );
    steps.push("Check everything: stance doctor".into());
    Plan {
        target,
        detected,
        units,
        steps,
    }
}

/// Enables the plan's units. Returns notes for units that are not
/// installed or could not be enabled.
pub fn apply(p: &Plan) -> Vec<String> {
    let Some(sc) = crate::tool::find("systemctl") else {
        return vec!["systemctl not found; enable the units yourself".into()];
    };
    let mut notes = Vec::new();
    for u in &p.units {
        let installed = crate::feature::unit_installed(&sc, u);
        if !installed {
            notes.push(format!(
                "{u} is not installed (packages ship it; from source: install -Dm644 contrib/{u} ~/.config/systemd/user/{u}, fix ExecStart paths, systemctl --user daemon-reload)"
            ));
            continue;
        }
        if let Err(e) = crate::tool::run_tool(
            &sc,
            &["--user", "enable", "--now", u],
            std::time::Duration::from_secs(20),
        ) {
            notes.push(format!("{u}: {e}"));
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn detects_targets() {
        assert_eq!(
            detect(&[v(&["/usr/bin/qs", "-c", "end4-pC"])]).0,
            Target::End4
        );
        assert_eq!(detect(&[v(&["qs", "-c", "ii"])]).0, Target::End4);
        assert_eq!(detect(&[v(&["/usr/bin/waybar"])]).0, Target::Waybar);
        assert_eq!(detect(&[v(&["qs", "-c", "mine"])]).0, Target::Quickshell);
        assert_eq!(detect(&[]).0, Target::Quickshell);
    }

    #[test]
    fn plans() {
        let p = plan(
            Target::Waybar,
            None,
            Some(Path::new("/usr/share/stance/quickshell")),
        );
        assert_eq!(p.units, vec!["stance-panel.service"]);
        assert!(p.steps.iter().any(|s| s.contains("custom/stance")));
        let e = plan(
            Target::End4,
            None,
            Some(Path::new("/usr/share/stance/quickshell")),
        );
        assert!(e.units.is_empty(), "end4 runs its own shell");
        assert!(
            e.steps
                .iter()
                .any(|s| s.contains("/usr/share/stance/integrations/end4-pC"))
        );
        assert!(Target::parse("kde").is_err());
    }
}
