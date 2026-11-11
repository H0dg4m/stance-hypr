//! Panel themes (docs/themes.md): built-ins compiled in from `themes/`,
//! user themes in `~/.config/stance/themes/<id>.json`. One strict format
//! for built-ins, the editor, import and export.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::store;

pub const MAX_USER_THEMES: usize = 200;

pub const COLOR_KEYS: &[&str] = &[
    "background",
    "card",
    "surface",
    "surfaceHover",
    "border",
    "text",
    "textDim",
    "accent",
    "accentText",
    "danger",
    "dangerText",
    "scrim",
];

macro_rules! builtins {
    ($($id:literal),* $(,)?) => {
        &[$(($id, include_str!(concat!("../themes/", $id, ".json")))),*]
    };
}

/// Built-in themes in display order.
const BUILTIN: &[(&str, &str)] = builtins!(
    "stance-dark",
    "stance-light",
    "catppuccin-mocha",
    "catppuccin-macchiato",
    "catppuccin-frappe",
    "catppuccin-latte",
    "nord",
    "gruvbox-dark",
    "gruvbox-light",
    "dracula",
    "tokyo-night",
    "rose-pine",
    "rose-pine-dawn",
    "everforest-dark",
    "kanagawa",
    "one-dark",
    "solarized-dark",
);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Metrics {
    #[serde(default = "d_radius")]
    pub radius: f64,
    #[serde(default = "d_chip")]
    pub chip_radius: f64,
    #[serde(default)]
    pub font_family: String,
    #[serde(default = "d_font")]
    pub font_size: f64,
    #[serde(default)]
    pub icon_font: String,
    #[serde(default = "d_anim")]
    pub anim_ms: u32,
    #[serde(default = "d_pad")]
    pub padding: f64,
    #[serde(default = "d_spacing")]
    pub spacing: f64,
    #[serde(default = "d_opacity")]
    pub opacity: f64,
}

fn d_radius() -> f64 {
    12.0
}
fn d_chip() -> f64 {
    8.0
}
fn d_font() -> f64 {
    13.0
}
fn d_anim() -> u32 {
    120
}
fn d_pad() -> f64 {
    12.0
}
fn d_spacing() -> f64 {
    8.0
}
fn d_opacity() -> f64 {
    1.0
}

impl Default for Metrics {
    fn default() -> Self {
        serde_json::from_str("{}").expect("metric defaults")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub stance_theme: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub dark: bool,
    pub colors: BTreeMap<String, String>,
    #[serde(default)]
    pub metrics: Metrics,
}

#[derive(Debug, Serialize)]
pub struct Listed {
    pub builtin: bool,
    pub theme: Theme,
}

fn invalid(msg: impl Into<String>) -> Error {
    Error::new("invalid", msg)
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn valid_color(c: &str) -> bool {
    let Some(h) = c.strip_prefix('#') else {
        return false;
    };
    matches!(h.len(), 6 | 8) && h.bytes().all(|b| b.is_ascii_hexdigit())
}

fn range(name: &str, v: f64, lo: f64, hi: f64) -> Result<()> {
    if v.is_finite() && (lo..=hi).contains(&v) {
        Ok(())
    } else {
        Err(invalid(format!("{name} must be between {lo} and {hi}")))
    }
}

fn font_name(name: &str, v: &str) -> Result<()> {
    if v.chars().count() > 64
        || v.chars()
            .any(|c| c.is_control() || crate::clip::is_invisible(c))
    {
        return Err(invalid(format!(
            "{name}: at most 64 characters, no control characters"
        )));
    }
    Ok(())
}

impl Theme {
    pub fn validate(&self) -> Result<()> {
        if self.stance_theme != 1 {
            return Err(invalid("stance_theme must be 1"));
        }
        if !valid_id(&self.id) {
            return Err(invalid("id: 1-40 characters of a-z, 0-9 and -"));
        }
        let n = self.name.chars().count();
        if n == 0
            || n > 60
            || self
                .name
                .chars()
                .any(|c| c.is_control() || crate::clip::is_invisible(c))
        {
            return Err(invalid("name: 1-60 characters, no control characters"));
        }
        for k in self.colors.keys() {
            if !COLOR_KEYS.contains(&k.as_str()) {
                return Err(invalid(format!("unknown colour `{k}`")));
            }
        }
        for k in COLOR_KEYS {
            match self.colors.get(*k) {
                None => return Err(invalid(format!("colour `{k}` is missing"))),
                Some(c) if !valid_color(c) => {
                    return Err(invalid(format!("colour `{k}`: use #rrggbb or #aarrggbb")));
                }
                _ => {}
            }
        }
        let m = &self.metrics;
        range("radius", m.radius, 0.0, 40.0)?;
        range("chipRadius", m.chip_radius, 0.0, 30.0)?;
        range("fontSize", m.font_size, 8.0, 32.0)?;
        range("animMs", f64::from(m.anim_ms), 0.0, 1000.0)?;
        range("padding", m.padding, 0.0, 40.0)?;
        range("spacing", m.spacing, 0.0, 32.0)?;
        range("opacity", m.opacity, 0.5, 1.0)?;
        font_name("fontFamily", &m.font_family)?;
        font_name("iconFont", &m.icon_font)?;
        Ok(())
    }

    pub fn parse(json: &str) -> Result<Self> {
        let t: Theme = serde_json::from_str(json).map_err(|e| invalid(format!("theme: {e}")))?;
        t.validate()?;
        Ok(t)
    }
}

/// WCAG 2.x relative luminance of `#rrggbb` / `#aarrggbb` (alpha ignored).
fn luminance(c: &str) -> Option<f64> {
    let h = c.strip_prefix('#')?;
    let h = match h.len() {
        6 => h,
        8 => &h[2..],
        _ => return None,
    };
    let ch = |i: usize| -> Option<f64> {
        let v = f64::from(u8::from_str_radix(h.get(i..i + 2)?, 16).ok()?) / 255.0;
        Some(if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        })
    };
    Some(0.2126 * ch(0)? + 0.7152 * ch(2)? + 0.0722 * ch(4)?)
}

/// WCAG contrast ratio (1.0 to 21.0) between two colours.
pub fn contrast_ratio(a: &str, b: &str) -> Option<f64> {
    let (la, lb) = (luminance(a)?, luminance(b)?);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    Some((hi + 0.05) / (lo + 0.05))
}

/// (foreground, background, minimum ratio) pairs that matter for legibility.
pub const CONTRAST_PAIRS: &[(&str, &str, f64)] = &[
    ("text", "background", 4.5),
    ("text", "card", 4.5),
    ("textDim", "card", 3.0),
    ("accentText", "accent", 4.5),
    ("dangerText", "danger", 4.5),
];

#[derive(Debug, Serialize)]
pub struct ContrastRow {
    pub pair: String,
    pub ratio: f64,
    pub min: f64,
    pub ok: bool,
}

impl Theme {
    pub fn contrast(&self) -> Vec<ContrastRow> {
        CONTRAST_PAIRS
            .iter()
            .filter_map(|(f, b, min)| {
                let r = contrast_ratio(self.colors.get(*f)?, self.colors.get(*b)?)?;
                let ratio = (r * 100.0).round() / 100.0;
                Some(ContrastRow {
                    pair: format!("{f}/{b}"),
                    ratio,
                    min: *min,
                    ok: r >= *min,
                })
            })
            .collect()
    }
}

pub fn builtins() -> Vec<Theme> {
    BUILTIN
        .iter()
        .map(|(_, s)| Theme::parse(s).expect("built-in theme is valid"))
        .collect()
}

pub fn is_builtin(id: &str) -> bool {
    BUILTIN.iter().any(|(b, _)| *b == id)
}

/// Ids the panel uses for itself (`shell` = the embedding shell's colours).
pub fn is_reserved(id: &str) -> bool {
    matches!(id, "shell" | "auto" | "default")
}

pub struct Themes {
    pub dir: PathBuf,
}

impl Themes {
    pub fn open() -> Result<Self> {
        Ok(Self {
            dir: store::config_dir()?.join("themes"),
        })
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// User themes, sorted by name. Broken files are skipped with a note on
    /// stderr rather than hiding every theme.
    pub fn user(&self) -> Vec<Theme> {
        let mut out = Vec::new();
        let Ok(rd) = fs::read_dir(&self.dir) else {
            return out;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            match store::read_checked(&p).and_then(|s| Theme::parse(&s)) {
                Ok(t) if !is_builtin(&t.id) && p == self.path(&t.id) => out.push(t),
                Ok(_) => eprintln!(
                    "stance: {} ignored (id does not match the file name)",
                    p.display()
                ),
                Err(e) => eprintln!("stance: {} ignored: {e}", p.display()),
            }
        }
        out.sort_by_key(|t| t.name.to_lowercase());
        out
    }

    pub fn list(&self) -> Vec<Listed> {
        builtins()
            .into_iter()
            .map(|theme| Listed {
                builtin: true,
                theme,
            })
            .chain(self.user().into_iter().map(|theme| Listed {
                builtin: false,
                theme,
            }))
            .collect()
    }

    pub fn get(&self, id: &str) -> Result<Theme> {
        if !valid_id(id) {
            return Err(Error::new("usage", format!("`{id}` is not a theme id")));
        }
        if let Some((_, s)) = BUILTIN.iter().find(|(b, _)| *b == id) {
            return Theme::parse(s);
        }
        let p = self.path(id);
        if !p.exists() {
            return Err(Error::new("not_found", format!("no theme `{id}`")));
        }
        Theme::parse(&store::read_checked(&p)?)
    }

    /// Saves a user theme. Built-in ids are refused; an existing user theme
    /// needs `force`.
    pub fn put(&self, t: &Theme, force: bool) -> Result<()> {
        t.validate()?;
        if is_reserved(&t.id) {
            return Err(invalid(format!(
                "`{}` is reserved by the panel; pick another id",
                t.id
            )));
        }
        if is_builtin(&t.id) {
            return Err(Error::new(
                "exists",
                format!("`{}` is a built-in theme; save it under a new id", t.id),
            ));
        }
        let p = self.path(&t.id);
        if !p.exists() && self.user().len() >= MAX_USER_THEMES {
            return Err(Error::new(
                "too_many",
                format!("at most {MAX_USER_THEMES} saved themes; remove one first"),
            ));
        }
        if p.exists() && !force {
            return Err(Error::new(
                "exists",
                format!("theme `{}` exists; pass --force to replace it", t.id),
            ));
        }
        store::ensure_private_dir(&self.dir)?;
        let body = serde_json::to_string_pretty(t).unwrap_or_default() + "\n";
        store::atomic_write(&p, body.as_bytes(), false)
    }

    pub fn import(&self, file: &Path, force: bool) -> Result<Theme> {
        let s = read_import(file)?;
        let t = Theme::parse(&s)?;
        self.put(&t, force)?;
        Ok(t)
    }

    pub fn rm(&self, id: &str) -> Result<()> {
        if is_builtin(id) {
            return Err(Error::new(
                "usage",
                format!("`{id}` is built in and cannot be removed"),
            ));
        }
        self.get(id)?;
        fs::remove_file(self.path(id)).map_err(|e| Error::new("io", format!("remove theme: {e}")))
    }
}

/// Reads an import file: a regular file (symlinks followed are fine, the
/// target must be a regular file), at most 64 KiB. Ownership is not
/// required — themes are shared between people.
fn read_import(file: &Path) -> Result<String> {
    let io = |e: std::io::Error| Error::new("io", format!("{}: {e}", file.display()));
    let meta = fs::metadata(file).map_err(io)?;
    if !meta.is_file() {
        return Err(Error::new(
            "unsafe_path",
            format!("{} is not a regular file", file.display()),
        ));
    }
    if meta.len() > store::MAX_INPUT {
        return Err(Error::new(
            "too_large",
            format!("{} is larger than 64 KiB", file.display()),
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(file)
        .and_then(|f| {
            std::io::Read::read_to_end(
                &mut std::io::Read::take(f, store::MAX_INPUT + 1),
                &mut bytes,
            )
        })
        .map_err(io)?;
    if bytes.len() as u64 > store::MAX_INPUT {
        return Err(Error::new(
            "too_large",
            format!("{} is larger than 64 KiB", file.display()),
        ));
    }
    String::from_utf8(bytes).map_err(|_| invalid("theme file is not UTF-8"))
}

/// Writes `t` to `out` (0644, no symlink follow); refuses to replace an
/// existing file unless `force`.
pub fn export_to(t: &Theme, out: &Path, force: bool) -> Result<()> {
    let body = serde_json::to_string_pretty(t).unwrap_or_default() + "\n";
    if let Ok(m) = fs::symlink_metadata(out) {
        if m.file_type().is_symlink() || !m.is_file() {
            return Err(Error::new(
                "unsafe_path",
                format!("{} is not a regular file", out.display()),
            ));
        }
        if !force {
            return Err(Error::new(
                "exists",
                format!("{} exists; pass --force to replace it", out.display()),
            ));
        }
    }
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .custom_flags(libc::O_NOFOLLOW)
        .open(out)
        .map_err(|e| Error::new("io", format!("{}: {e}", out.display())))?;
    f.write_all(body.as_bytes())
        .map_err(|e| Error::new("io", format!("{}: {e}", out.display())))
}

/// The desktop's light/dark preference: "dark", "light" or "unknown".
/// Asks the XDG settings portal first (`org.freedesktop.appearance
/// color-scheme`: 1 dark, 2 light, 0 no preference), then GNOME's
/// gsettings. Each call has a short timeout; a missing tool is "unknown".
pub fn system_scheme() -> &'static str {
    use std::time::Duration;
    let t = Duration::from_secs(2);
    if let Some(busctl) = crate::tool::find("busctl")
        && let Ok(out) = crate::tool::run_tool(
            &busctl,
            &[
                "--user",
                "call",
                "org.freedesktop.portal.Desktop",
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Settings",
                "ReadOne",
                "ss",
                "org.freedesktop.appearance",
                "color-scheme",
            ],
            t,
        )
        && let Some(s) = parse_portal(&out)
    {
        return s;
    }
    if let Some(gs) = crate::tool::find("gsettings")
        && let Ok(out) = crate::tool::run_tool(
            &gs,
            &["get", "org.gnome.desktop.interface", "color-scheme"],
            t,
        )
        && let Some(s) = parse_gsettings(&out)
    {
        return s;
    }
    "unknown"
}

/// `busctl` prints `v u 1` for ReadOne (older portals: `v v u 1`); the last
/// word is the value. 0 (no preference) and anything odd give None.
fn parse_portal(out: &str) -> Option<&'static str> {
    match out.split_whitespace().last()?.parse::<u32>().ok()? {
        1 => Some("dark"),
        2 => Some("light"),
        _ => None,
    }
}

/// `gsettings get` prints `'prefer-dark'`, `'prefer-light'` or `'default'`.
fn parse_gsettings(out: &str) -> Option<&'static str> {
    match out.trim().trim_matches(['\'', '"']) {
        "prefer-dark" => Some("dark"),
        "prefer-light" | "default" => Some("light"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> Themes {
        let d = std::env::temp_dir().join(format!("stance-theme-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        Themes {
            dir: d.join("themes"),
        }
    }

    fn sample(id: &str) -> Theme {
        let mut t = builtins().into_iter().nth(2).unwrap();
        t.id = id.into();
        t.name = "Mine".into();
        t
    }

    #[test]
    fn contrast_math() {
        assert!((contrast_ratio("#000000", "#ffffff").unwrap() - 21.0).abs() < 1e-9);
        assert!((contrast_ratio("#ffffff", "#ffffff").unwrap() - 1.0).abs() < 1e-9);
        // #777777 on white is the classic 4.48:1
        let r = contrast_ratio("#777777", "#ffffff").unwrap();
        assert!((r - 4.48).abs() < 0.01, "{r}");
        assert!((contrast_ratio("#ff000000", "#ffffff").unwrap() - 21.0).abs() < 1e-9);
        assert!(contrast_ratio("red", "#fff").is_none());
    }

    #[test]
    fn contrast_rows() {
        let t = builtins().remove(0);
        assert_eq!(t.contrast().len(), 5);
    }

    #[test]
    fn builtins_are_valid_and_complete() {
        let b = builtins();
        assert_eq!(b.len(), 17);
        assert_eq!(b[0].id, "stance-dark");
        for t in &b {
            assert_eq!(t.colors.len(), COLOR_KEYS.len(), "{}", t.id);
            assert!(
                BUILTIN.iter().any(|(id, _)| *id == t.id),
                "file id {}",
                t.id
            );
        }
    }

    #[test]
    fn validation() {
        let ok = sample("mine");
        assert!(ok.validate().is_ok());
        let mut t = ok.clone();
        t.colors.insert("accent".into(), "red".into());
        assert!(t.validate().is_err());
        let mut t = ok.clone();
        t.colors.insert("accent".into(), "#ff00ff;}".into());
        assert!(t.validate().is_err());
        let mut t = ok.clone();
        t.colors.remove("scrim");
        assert!(t.validate().is_err());
        let mut t = ok.clone();
        t.colors.insert("evil".into(), "#000000".into());
        assert!(t.validate().is_err());
        let mut t = ok.clone();
        t.id = "../x".into();
        assert!(t.validate().is_err());
        let mut t = ok.clone();
        t.name = "evil\u{202E}txt".into();
        assert!(t.validate().is_err(), "bidi override");
        let mut t = ok.clone();
        t.metrics.opacity = 0.1;
        assert!(t.validate().is_err());
        let mut t = ok.clone();
        t.metrics.font_size = f64::NAN;
        assert!(t.validate().is_err());
        assert!(
            Theme::parse(r#"{"stance_theme":1,"id":"a","name":"A","colors":{},"exec":"x"}"#)
                .is_err()
        );
        assert!(valid_color("#11223344") && !valid_color("#123") && !valid_color("112233"));
    }

    #[test]
    fn metrics_default_when_missing() {
        let mut v = serde_json::to_value(sample("m")).unwrap();
        v.as_object_mut().unwrap().remove("metrics");
        let t = Theme::parse(&v.to_string()).unwrap();
        assert_eq!(t.metrics, Metrics::default());
        assert_eq!(t.metrics.radius, 12.0);
    }

    #[test]
    fn put_import_export_rm() {
        let th = tmp("io");
        assert!(
            th.put(&sample("catppuccin-mocha"), true).is_err(),
            "built-in id"
        );
        th.put(&sample("mine"), false).unwrap();
        assert!(th.put(&sample("mine"), false).is_err());
        th.put(&sample("mine"), true).unwrap();
        assert_eq!(th.get("mine").unwrap().name, "Mine");
        let l = th.list();
        assert_eq!(l.len(), 18);
        assert!(!l[17].builtin);

        let out = th.dir.parent().unwrap().join("exported.json");
        export_to(&th.get("nord").unwrap(), &out, false).unwrap();
        assert!(export_to(&th.get("nord").unwrap(), &out, false).is_err());
        let mut v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
        v["id"] = "nord-mine".into();
        fs::write(&out, v.to_string()).unwrap();
        assert_eq!(th.import(&out, false).unwrap().id, "nord-mine");

        fs::write(th.path("broken"), "{").unwrap();
        assert_eq!(th.user().len(), 2, "broken file skipped");
        th.rm("mine").unwrap();
        assert!(th.rm("nord").is_err());
        assert!(th.get("mine").is_err());
        fs::remove_dir_all(th.dir.parent().unwrap()).ok();
    }

    #[test]
    fn system_scheme_parsers() {
        assert_eq!(parse_portal("v u 1\n"), Some("dark"));
        assert_eq!(parse_portal("v u 2"), Some("light"));
        assert_eq!(parse_portal("v v u 1\n"), Some("dark"));
        assert_eq!(parse_portal("v u 0\n"), None);
        assert_eq!(parse_portal("v u 7\n"), None);
        assert_eq!(parse_portal(""), None);
        assert_eq!(parse_portal("Call failed: no such method"), None);
        assert_eq!(parse_gsettings("'prefer-dark'\n"), Some("dark"));
        assert_eq!(parse_gsettings("'prefer-light'\n"), Some("light"));
        assert_eq!(parse_gsettings("'default'\n"), Some("light"));
        assert_eq!(parse_gsettings("uint32 1"), None);
        assert_eq!(parse_gsettings(""), None);
        assert!(["dark", "light", "unknown"].contains(&system_scheme()));
    }
}
