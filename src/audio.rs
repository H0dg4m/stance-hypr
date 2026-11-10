//! Audio output sinks: listing via `pw-dump` JSON, switching via
//! `wpctl set-default <id>` (argv only, id taken from the resolved list).

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::tool;

const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Sink {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub default: bool,
    /// Short display name: text in `[...]` of the description, else the
    /// node nick, else the description (at most 40 characters).
    pub short: String,
    /// "pci", "usb", "bluetooth", ... (empty when unknown).
    pub bus: String,
    /// Device profile, e.g. "Analog Stereo" (empty when unknown).
    pub profile: String,
    #[serde(rename = "virtual")]
    pub virtual_: bool,
}

pub fn available() -> bool {
    tool::find("pw-dump").is_some() && tool::find("wpctl").is_some()
}

fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .take(160)
        .collect()
}

/// Pure: picks the short label for a sink.
fn short_name(description: &str, nick: Option<&str>) -> String {
    let bracket = description
        .rfind('[')
        .and_then(|i| description[i + 1..].strip_suffix(']').map(str::trim))
        .filter(|b| !b.is_empty());
    let pick = bracket
        .or(nick.map(str::trim).filter(|n| !n.is_empty()))
        .unwrap_or(description)
        .trim();
    let s: String = pick
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    if s.chars().count() > 40 {
        let mut t: String = s.chars().take(39).collect();
        t.push('…');
        t
    } else {
        s
    }
}

fn name_of(v: &Value) -> Option<String> {
    match v {
        Value::Object(o) => o.get("name")?.as_str().map(str::to_owned),
        Value::String(s) => serde_json::from_str::<Value>(s)
            .ok()
            .and_then(|j| j.get("name")?.as_str().map(str::to_owned)),
        _ => None,
    }
}

/// Pure: parses `pw-dump` output into sinks (sorted by id).
pub fn parse_pw_dump(json: &str) -> Result<Vec<Sink>> {
    let v: Value = serde_json::from_str(json)
        .map_err(|e| Error::new("audio", format!("pw-dump: bad JSON: {e}")))?;
    let arr = v
        .as_array()
        .ok_or_else(|| Error::new("audio", "pw-dump: expected an array"))?;
    let mut default_name: Option<String> = None;
    for o in arr {
        let is_meta = o["type"].as_str().is_some_and(|t| t.ends_with("Metadata"));
        if is_meta
            && o["props"]["metadata.name"].as_str() == Some("default")
            && let Some(items) = o["metadata"].as_array()
        {
            for m in items {
                if m["key"].as_str() == Some("default.audio.sink") {
                    default_name = name_of(&m["value"]);
                }
            }
        }
    }
    let mut sinks = Vec::new();
    for o in arr {
        if !o["type"].as_str().is_some_and(|t| t.ends_with("Node")) {
            continue;
        }
        let p = &o["info"]["props"];
        if p["media.class"].as_str() != Some("Audio/Sink") {
            continue;
        }
        let (Some(id), Some(name)) = (o["id"].as_u64(), p["node.name"].as_str()) else {
            continue;
        };
        let Ok(id) = u32::try_from(id) else { continue };
        let description = p["node.description"]
            .as_str()
            .or_else(|| p["node.nick"].as_str())
            .unwrap_or(name);
        let virtual_ = p["node.virtual"].as_bool().unwrap_or(false) || p.get("device.id").is_none();
        let short = short_name(description, p["node.nick"].as_str());
        sinks.push(Sink {
            id,
            short,
            bus: clean(p["device.bus"].as_str().unwrap_or("")),
            profile: clean(p["device.profile.description"].as_str().unwrap_or("")),
            name: clean(name),
            description: clean(description),
            default: default_name.as_deref() == Some(name),
            virtual_,
        });
    }
    sinks.sort_by_key(|s| s.id);
    Ok(sinks)
}

pub fn list() -> Result<Vec<Sink>> {
    let path = tool::find("pw-dump")
        .ok_or_else(|| Error::new("audio", "pw-dump not found (install pipewire)"))?;
    parse_pw_dump(&tool::run_tool(&path, &[], TIMEOUT)?)
}

pub fn get_default() -> Result<Option<Sink>> {
    Ok(list()?.into_iter().find(|s| s.default))
}

/// Query is shown in mode files and CLI: bounded, no control characters.
pub fn validate_match(q: &str) -> Result<()> {
    let n = q.trim().chars().count();
    if n == 0 || n > 128 || q.chars().any(char::is_control) {
        return Err(Error::new(
            "audio",
            "sink match must be 1-128 printable characters",
        ));
    }
    Ok(())
}

/// Resolution order: numeric id, exact node.name, then a case-insensitive
/// substring of description or name. Several substring hits is an error.
pub fn resolve<'a>(q: &str, sinks: &'a [Sink]) -> Result<&'a Sink> {
    validate_match(q)?;
    let q = q.trim();
    if q.bytes().all(|b| b.is_ascii_digit())
        && let Ok(id) = q.parse::<u32>()
    {
        return sinks
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::new("audio_not_found", format!("no sink with id {id}")));
    }
    if let Some(s) = sinks.iter().find(|s| s.name == q) {
        return Ok(s);
    }
    let needle = q.to_lowercase();
    let hits: Vec<&Sink> = sinks
        .iter()
        .filter(|s| {
            s.description.to_lowercase().contains(&needle)
                || s.name.to_lowercase().contains(&needle)
        })
        .collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => Err(Error::new(
            "audio_not_found",
            format!("no sink matches '{}'", clean(q)),
        )),
        many => Err(Error::new(
            "audio_ambiguous",
            format!(
                "'{}' matches {} sinks: {}",
                clean(q),
                many.len(),
                many.iter()
                    .map(|s| s.id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

/// Makes the resolved sink the default output. Returns it with default=true.
pub fn set_default(q: &str) -> Result<Sink> {
    let sinks = list()?;
    let mut sink = resolve(q, &sinks)?.clone();
    let wpctl = tool::find("wpctl")
        .ok_or_else(|| Error::new("audio", "wpctl not found (install wireplumber)"))?;
    tool::run_tool(&wpctl, &["set-default", &sink.id.to_string()], TIMEOUT)?;
    sink.default = true;
    Ok(sink)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIX: &str = include_str!("../tests/fixtures/pw-dump-sinks.json");

    #[test]
    fn parses_fixture() {
        let s = parse_pw_dump(FIX).unwrap();
        assert_eq!(s.len(), 4);
        assert!(s[0].virtual_ && s[0].id == 33);
        assert!(!s[1].virtual_);
        let d: Vec<_> = s.iter().filter(|x| x.default).collect();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].id, 86);
    }

    #[test]
    fn short_names() {
        let s = parse_pw_dump(FIX).unwrap();
        let by = |id| s.iter().find(|x| x.id == id).unwrap();
        assert_eq!(by(85).short, "EPIC Q40");
        assert_eq!(by(85).bus, "pci");
        assert!(!by(85).profile.is_empty());
        assert_eq!(by(86).short, "Razer Barracuda X 2.4");
        assert_eq!(by(86).bus, "usb");
        assert_eq!(by(33).short, "Easy Effects Sink");
        assert_eq!(by(33).bus, "");
        assert_eq!(short_name("Long ", None), "Long");
        assert_eq!(short_name(&"x".repeat(80), None).chars().count(), 40);
        assert_eq!(short_name("A [b]", Some("nick")), "b");
        assert_eq!(short_name("A []", Some("nick")), "nick");
    }

    #[test]
    fn resolves() {
        let s = parse_pw_dump(FIX).unwrap();
        assert_eq!(resolve("88", &s).unwrap().id, 88);
        assert_eq!(resolve("razer", &s).unwrap().id, 86);
        assert_eq!(resolve("easyeffects_sink", &s).unwrap().id, 33);
        assert_eq!(resolve("hdmi", &s).unwrap().id, 85);
        assert_eq!(resolve("analog", &s).unwrap_err().code, "audio_ambiguous");
        assert_eq!(resolve("99", &s).unwrap_err().code, "audio_not_found");
        assert_eq!(resolve("nope", &s).unwrap_err().code, "audio_not_found");
        assert!(resolve("a\nb", &s).is_err());
        assert!(resolve("  ", &s).is_err());
    }

    #[test]
    fn exact_name_beats_ambiguity() {
        let mk = |id, name: &str| Sink {
            id,
            name: name.into(),
            description: "x".into(),
            default: false,
            short: String::new(),
            bus: String::new(),
            profile: String::new(),
            virtual_: false,
        };
        let s = vec![mk(1, "a"), mk(2, "ab")];
        assert_eq!(resolve("a", &s).unwrap().id, 1);
    }

    #[test]
    fn bad_json() {
        assert!(parse_pw_dump("nope").is_err());
        assert!(parse_pw_dump("{}").is_err());
    }
}
