//! `deck.toml`: the buttons the phone shows. Strict like mode files:
//! unknown fields are errors, sizes and counts are capped, and an action is
//! one of a fixed set of kinds mapped to `stance` arguments (see
//! `actions.rs`); there is no way to put a command or a shell line here.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::proto::{MAX_BUTTONS, MAX_ID, MAX_LABEL};
use crate::error::{Error, Result};
use crate::modes::valid_name;

/// Longest free-text argument (monitor, sink, reference).
const MAX_ARG: usize = 64;

#[cfg(test)]
pub const EXAMPLE: &str = include_str!("../../examples/deck.toml");

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeckFile {
    /// File format version (src/migrate.rs); missing = 1.
    #[serde(default = "schema_one")]
    pub schema: u32,
    #[serde(default, rename = "button")]
    pub buttons: Vec<ButtonCfg>,
}

fn schema_one() -> u32 {
    1
}

/// One button. The fields that apply depend on `action`; the others must
/// be absent (checked by `validate`).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonCfg {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub action: String,
    /// mode: mode name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// night: toggle | on | off | set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
    /// night on/set: temperature in K.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kelvin: Option<u32>,
    /// brightness / layout: monitor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor: Option<String>,
    /// brightness: `N`, `+N` or `-N` (percent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// audio: sink id, node name or unique text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sink: Option<String>,
    /// layout: left-of | right-of | above | below | mirror | single.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// layout: reference monitor for kinds that need one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

/// A validated action, ready to become argv.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Mode(String),
    Night(NightOp),
    Brightness {
        monitor: String,
        value: String,
    },
    Audio(String),
    Layout {
        kind: String,
        monitor: String,
        reference: Option<String>,
    },
    Confirm,
    Revert,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NightOp {
    Toggle,
    On(Option<u32>),
    Off,
    Set(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Button {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    pub action: Action,
}

fn bad(id: &str, msg: &str) -> Error {
    Error::new("deck_config", format!("button `{id}`: {msg}"))
}

/// Free-text argument: bounded, no control characters, and never something
/// the CLI could take for an option.
fn text_arg(id: &str, field: &str, v: &Option<String>) -> Result<String> {
    let v = v
        .as_deref()
        .ok_or_else(|| bad(id, &format!("`{field}` is required")))?;
    if v.is_empty() || v.chars().count() > MAX_ARG || v.chars().any(char::is_control) {
        return Err(bad(
            id,
            &format!("`{field}` must be 1-{MAX_ARG} printable characters"),
        ));
    }
    if v.starts_with('-') {
        return Err(bad(id, &format!("`{field}` must not start with `-`")));
    }
    Ok(v.to_string())
}

fn only(b: &ButtonCfg, allowed: &[&str]) -> Result<()> {
    let present = [
        ("name", b.name.is_some()),
        ("op", b.op.is_some()),
        ("kelvin", b.kelvin.is_some()),
        ("monitor", b.monitor.is_some()),
        ("value", b.value.is_some()),
        ("sink", b.sink.is_some()),
        ("kind", b.kind.is_some()),
        ("reference", b.reference.is_some()),
    ];
    for (f, p) in present {
        if p && !allowed.contains(&f) {
            return Err(bad(
                &b.id,
                &format!("`{f}` does not apply to action `{}`", b.action),
            ));
        }
    }
    Ok(())
}

fn check_brightness(id: &str, v: &str) -> Result<()> {
    let digits = v.strip_prefix(['+', '-']).unwrap_or(v);
    match digits.parse::<u8>() {
        Ok(n) if n <= 100 && !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) => {
            Ok(())
        }
        _ => Err(bad(id, "`value` must be N, +N or -N (percent, 0-100)")),
    }
}

impl ButtonCfg {
    pub fn validate(&self) -> Result<Button> {
        let id = &self.id;
        if id.is_empty()
            || id.len() > MAX_ID
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(bad(
                id,
                &format!("id may use letters, digits, - and _ (1-{MAX_ID})"),
            ));
        }
        if self.label.trim().is_empty()
            || self.label.chars().count() > MAX_LABEL
            || self.label.chars().any(char::is_control)
        {
            return Err(bad(
                id,
                &format!("label must be 1-{MAX_LABEL} printable characters"),
            ));
        }
        if let Some(i) = &self.icon
            && (i.is_empty()
                || i.len() > 40
                || !i
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'))
        {
            return Err(bad(
                id,
                "icon is a Material Symbols name (lowercase, digits, _)",
            ));
        }
        let action = match self.action.as_str() {
            "mode" => {
                only(self, &["name"])?;
                let n = text_arg(id, "name", &self.name)?;
                if !valid_name(&n) {
                    return Err(bad(id, "mode names may only use letters, digits, - and _"));
                }
                Action::Mode(n)
            }
            "night" => {
                only(self, &["op", "kelvin"])?;
                let op = self.op.as_deref().unwrap_or("toggle");
                let k = match self.kelvin {
                    Some(k) => {
                        crate::night::check_temperature(k).map_err(|e| bad(id, &e.message))?;
                        Some(k)
                    }
                    None => None,
                };
                match (op, k) {
                    ("toggle", None) => Action::Night(NightOp::Toggle),
                    ("off", None) => Action::Night(NightOp::Off),
                    ("on", k) => Action::Night(NightOp::On(k)),
                    ("set", Some(k)) => Action::Night(NightOp::Set(k)),
                    ("set", None) => return Err(bad(id, "`op = \"set\"` needs `kelvin`")),
                    ("toggle" | "off", Some(_)) => {
                        return Err(bad(id, "`kelvin` only applies to `on` and `set`"));
                    }
                    _ => return Err(bad(id, "`op` is toggle, on, off or set")),
                }
            }
            "brightness" => {
                only(self, &["monitor", "value"])?;
                let monitor = text_arg(id, "monitor", &self.monitor)?;
                let value = self
                    .value
                    .clone()
                    .ok_or_else(|| bad(id, "`value` is required"))?;
                check_brightness(id, &value)?;
                Action::Brightness { monitor, value }
            }
            "audio" => {
                only(self, &["sink"])?;
                Action::Audio(text_arg(id, "sink", &self.sink)?)
            }
            "layout" => {
                only(self, &["kind", "monitor", "reference"])?;
                let kind = text_arg(id, "kind", &self.kind)?;
                let l = crate::plan::Layout::parse(&kind)
                    .ok_or_else(|| bad(id, &format!("unknown layout kind `{kind}`")))?;
                let monitor = text_arg(id, "monitor", &self.monitor)?;
                let reference = if l.needs_reference() {
                    Some(text_arg(id, "reference", &self.reference)?)
                } else if self.reference.is_some() {
                    return Err(bad(id, &format!("`{kind}` takes no `reference`")));
                } else {
                    None
                };
                Action::Layout {
                    kind,
                    monitor,
                    reference,
                }
            }
            "confirm" => {
                only(self, &[])?;
                Action::Confirm
            }
            "revert" => {
                only(self, &[])?;
                Action::Revert
            }
            other => {
                return Err(bad(
                    id,
                    &format!(
                        "unknown action `{other}` (mode, night, brightness, audio, layout, confirm, revert)"
                    ),
                ));
            }
        };
        Ok(Button {
            id: self.id.clone(),
            label: self.label.trim().to_string(),
            icon: self.icon.clone(),
            action,
        })
    }
}

impl Button {
    /// Back to the file form (for editors: `stance deck buttons --json`).
    pub fn to_cfg(&self) -> ButtonCfg {
        let mut c = ButtonCfg {
            id: self.id.clone(),
            label: self.label.clone(),
            icon: self.icon.clone(),
            ..Default::default()
        };
        let s = |v: &str| Some(v.to_string());
        match &self.action {
            Action::Mode(n) => (c.action, c.name) = ("mode".into(), Some(n.clone())),
            Action::Night(op) => {
                c.action = "night".into();
                (c.op, c.kelvin) = match op {
                    NightOp::Toggle => (s("toggle"), None),
                    NightOp::Off => (s("off"), None),
                    NightOp::On(k) => (s("on"), *k),
                    NightOp::Set(k) => (s("set"), Some(*k)),
                };
            }
            Action::Brightness { monitor, value } => {
                (c.action, c.monitor, c.value) = (
                    "brightness".into(),
                    Some(monitor.clone()),
                    Some(value.clone()),
                )
            }
            Action::Audio(sink) => (c.action, c.sink) = ("audio".into(), Some(sink.clone())),
            Action::Layout {
                kind,
                monitor,
                reference,
            } => {
                c.action = "layout".into();
                (c.kind, c.monitor, c.reference) =
                    (Some(kind.clone()), Some(monitor.clone()), reference.clone());
            }
            Action::Confirm => c.action = "confirm".into(),
            Action::Revert => c.action = "revert".into(),
        }
        c
    }
}

/// An editor's JSON (same keys as the file) → validated TOML text.
pub fn from_json(data: &str) -> Result<String> {
    if data.len() > 64 * 1024 {
        return Err(Error::new("deck_config", "deck data is too large"));
    }
    let mut f: DeckFile = serde_json::from_str(data)
        .map_err(|e| Error::new("deck_config", format!("invalid deck data: {e}")))?;
    if f.schema > crate::migrate::CURRENT {
        return Err(crate::migrate::newer("deck.toml", f.schema));
    }
    f.schema = crate::migrate::CURRENT;
    let text = toml::to_string(&f).map_err(|e| Error::new("deck_config", e.to_string()))?;
    parse(&text)?;
    Ok(format!(
        "# Phone deck buttons (see examples/deck.toml; edited from the panel or `stance deck put`)\n{text}"
    ))
}

/// Writes `deck.toml` (text from [`from_json`]) unless the file on disk was
/// written by a newer stance (`newer_schema`). Keeps the old one as `.bak`.
#[allow(dead_code)] // TODO(0.4): drop once main.rs calls it (see CHANGELOG/ADR step 6-7)
pub fn write_file(path: &std::path::Path, text: &str) -> Result<()> {
    crate::migrate::guard_toml(path, "deck.toml")?;
    crate::store::atomic_write(path, text.as_bytes(), true)
}

/// Parses and validates a whole file. A newer file is read when it still
/// parses, else the error is `newer_schema`.
pub fn parse(text: &str) -> Result<Vec<Button>> {
    let newer = crate::migrate::toml_schema(text);
    let f: DeckFile = toml::from_str(text).map_err(|e| {
        if newer > crate::migrate::CURRENT {
            crate::migrate::newer("deck.toml", newer)
        } else {
            Error::new("deck_config", format!("deck.toml: {}", e.message()))
        }
    })?;
    if f.buttons.is_empty() {
        return Err(Error::new("deck_config", "deck.toml has no [[button]]"));
    }
    if f.buttons.len() > MAX_BUTTONS {
        return Err(Error::new(
            "deck_config",
            format!("at most {MAX_BUTTONS} buttons"),
        ));
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for b in &f.buttons {
        let v = b.validate()?;
        if !seen.insert(v.id.clone()) {
            return Err(bad(&v.id, "duplicate id"));
        }
        out.push(v);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(body: &str) -> Result<Vec<Button>> {
        parse(&format!("[[button]]\nid = \"a\"\nlabel = \"A\"\n{body}"))
    }

    #[test]
    fn example_is_valid() {
        let b = parse(EXAMPLE).unwrap();
        assert!(b.len() >= 5);
    }

    #[test]
    fn accepts_each_kind() {
        assert_eq!(
            one("action = \"mode\"\nname = \"work\"").unwrap()[0].action,
            Action::Mode("work".into())
        );
        assert_eq!(
            one("action = \"night\"").unwrap()[0].action,
            Action::Night(NightOp::Toggle)
        );
        assert_eq!(
            one("action = \"night\"\nop = \"set\"\nkelvin = 3500").unwrap()[0].action,
            Action::Night(NightOp::Set(3500))
        );
        assert!(one("action = \"brightness\"\nmonitor = \"DP-1\"\nvalue = \"-10\"").is_ok());
        assert!(one("action = \"audio\"\nsink = \"speakers\"").is_ok());
        assert!(
            one(
                "action = \"layout\"\nkind = \"left-of\"\nmonitor = \"DP-1\"\nreference = \"eDP-1\""
            )
            .is_ok()
        );
        assert!(one("action = \"layout\"\nkind = \"single\"\nmonitor = \"DP-1\"").is_ok());
        assert!(one("action = \"confirm\"").is_ok());
        assert!(one("action = \"revert\"").is_ok());
    }

    #[test]
    fn rejects_bad_input() {
        // unknown field / action / stray field for the kind
        assert!(one("action = \"exec\"\ncmd = \"id\"").is_err());
        assert!(one("action = \"run\"").is_err());
        assert!(one("action = \"confirm\"\nname = \"x\"").is_err());
        // option-looking and control characters
        assert!(one("action = \"audio\"\nsink = \"--json\"").is_err());
        assert!(one("action = \"audio\"\nsink = \"a\\nb\"").is_err());
        assert!(one("action = \"mode\"\nname = \"a b\"").is_err());
        assert!(one("action = \"mode\"\nname = \"../x\"").is_err());
        // brightness / night / layout details
        assert!(one("action = \"brightness\"\nmonitor = \"DP-1\"\nvalue = \"150\"").is_err());
        assert!(one("action = \"brightness\"\nmonitor = \"DP-1\"\nvalue = \"+\"").is_err());
        assert!(one("action = \"night\"\nop = \"set\"").is_err());
        assert!(one("action = \"night\"\nop = \"on\"\nkelvin = 99999").is_err());
        assert!(one("action = \"layout\"\nkind = \"left-of\"\nmonitor = \"DP-1\"").is_err());
        assert!(one("action = \"layout\"\nkind = \"spin\"\nmonitor = \"DP-1\"").is_err());
    }

    #[test]
    fn caps_and_duplicates() {
        assert!(parse("").is_err());
        assert!(parse("[[button]]\nid=\"a\"\nlabel=\"A\"\naction=\"confirm\"\n[[button]]\nid=\"a\"\nlabel=\"B\"\naction=\"revert\"").is_err());
        let many: String = (0..MAX_BUTTONS + 1)
            .map(|i| format!("[[button]]\nid=\"b{i}\"\nlabel=\"B\"\naction=\"confirm\"\n"))
            .collect();
        assert!(parse(&many).is_err());
        assert!(
            parse(&format!(
                "[[button]]\nid=\"{}\"\nlabel=\"A\"\naction=\"confirm\"",
                "x".repeat(MAX_ID + 1)
            ))
            .is_err()
        );
        assert!(
            parse(&format!(
                "[[button]]\nid=\"a\"\nlabel=\"{}\"\naction=\"confirm\"",
                "x".repeat(MAX_LABEL + 1)
            ))
            .is_err()
        );
        assert!(
            parse("surprise = 1\n[[button]]\nid=\"a\"\nlabel=\"A\"\naction=\"confirm\"").is_err()
        );
    }

    #[test]
    fn json_round_trip() {
        let buttons = parse(EXAMPLE).unwrap();
        let f = DeckFile {
            schema: 1,
            buttons: buttons.iter().map(Button::to_cfg).collect(),
        };
        let text = from_json(&serde_json::to_string(&f).unwrap()).unwrap();
        assert_eq!(parse(&text).unwrap(), buttons);
        assert!(
            from_json(r#"{"button":[{"id":"x","label":"X","action":"exec","name":"rm"}]}"#)
                .is_err()
        );
        assert!(
            from_json(
                r#"{"button":[{"id":"x","label":"X","action":"mode","name":"a","cmd":"x"}]}"#
            )
            .is_err()
        );
        assert!(from_json(r#"{"button":[]}"#).is_err());
    }

    #[test]
    fn schema_rules() {
        let b = "[[button]]\nid=\"a\"\nlabel=\"A\"\naction=\"confirm\"\n";
        // Missing and 1 are the same; written files say 1.
        assert!(parse(b).is_ok());
        assert!(parse(&format!("schema = 1\n{b}")).is_ok());
        let json = r#"{"button":[{"id":"a","label":"A","action":"confirm"}]}"#;
        assert!(from_json(json).unwrap().contains("schema = 1"));
        // Newer: read when it still parses, `newer_schema` when it does not.
        assert!(parse(&format!("schema = 2\n{b}")).is_ok());
        let e = parse(&format!("schema = 2\nfuture = 1\n{b}")).unwrap_err();
        assert_eq!(e.code, "newer_schema");
        let e = from_json(r#"{"schema":2,"button":[]}"#).unwrap_err();
        assert_eq!(e.code, "newer_schema");
    }

    #[test]
    fn write_refuses_newer_file() {
        let d = std::env::temp_dir().join(format!("stance-deckcfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        crate::store::ensure_private_dir(&d).unwrap();
        let p = d.join("deck.toml");
        std::fs::write(&p, "schema = 2\n").unwrap();
        assert_eq!(
            write_file(&p, "schema = 1\n").unwrap_err().code,
            "newer_schema"
        );
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "schema = 2\n");
        std::fs::write(&p, "schema = 1\n").unwrap();
        write_file(&p, "schema = 1\n# new\n").unwrap();
        std::fs::remove_dir_all(&d).ok();
    }
}
