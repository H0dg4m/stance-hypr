//! `deck.toml`: the buttons the phone shows. Strict like mode files:
//! unknown fields are errors, sizes and counts are capped, and an action is
//! one of a fixed set of kinds mapped to `stance` arguments (see
//! `actions.rs`); there is no way to put a command or a shell line here.

use std::collections::HashSet;

use serde::Deserialize;

use super::proto::{MAX_BUTTONS, MAX_ID, MAX_LABEL};
use crate::error::{Error, Result};
use crate::modes::valid_name;

/// Longest free-text argument (monitor, sink, reference).
const MAX_ARG: usize = 64;

#[cfg(test)]
pub const EXAMPLE: &str = include_str!("../../examples/deck.toml");

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeckFile {
    #[serde(default, rename = "button")]
    pub buttons: Vec<ButtonCfg>,
}

/// One button. The fields that apply depend on `action`; the others must
/// be absent (checked by `validate`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonCfg {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    pub action: String,
    /// mode: mode name.
    pub name: Option<String>,
    /// night: toggle | on | off | set.
    pub op: Option<String>,
    /// night on/set: temperature in K.
    pub kelvin: Option<u32>,
    /// brightness / layout: monitor.
    pub monitor: Option<String>,
    /// brightness: `N`, `+N` or `-N` (percent).
    pub value: Option<String>,
    /// audio: sink id, node name or unique text.
    pub sink: Option<String>,
    /// layout: left-of | right-of | above | below | mirror | single.
    pub kind: Option<String>,
    /// layout: reference monitor for kinds that need one.
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

/// Parses and validates a whole file.
pub fn parse(text: &str) -> Result<Vec<Button>> {
    let f: DeckFile = toml::from_str(text)
        .map_err(|e| Error::new("deck_config", format!("deck.toml: {}", e.message())))?;
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
}
