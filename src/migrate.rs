//! Schema versions of the files stance writes (docs/adr/0001-core-split.md,
//! step 6). JSON files carry a top-level `"schema": N`, TOML files
//! `schema = N`; a missing key means 1 (everything 0.2.0 and 0.3.0 wrote).
//!
//! Rules: a reader accepts every version up to [`CURRENT`] and upgrades it in
//! memory (reads never write); a *newer* file may still be read where that is
//! safe, but nothing is ever written over it: writers call a `guard_*`
//! function first and get the error code `newer_schema`.

use std::path::Path;

use serde_json::Value;

use crate::error::{Error, Result};

/// The schema every config file is written with by this build.
pub const CURRENT: u32 = 1;

/// One upgrade step: `(file, from version)` → rewrites the value to
/// `from + 1`. None are needed yet; the first one lands with schema 2.
type Step = fn(&mut Value);
const STEPS: &[(&str, u32, Step)] = &[];

pub fn newer(file: &str, found: u32) -> Error {
    Error::new(
        "newer_schema",
        format!(
            "{file} was written by a newer stance (schema {found}, this one understands {CURRENT}); \
             not changing it. Update stance or move the file aside."
        ),
    )
}

/// The schema of a parsed JSON file. Missing = 1; anything that is not a
/// positive integer is invalid.
pub fn json_schema(file: &str, v: &Value) -> Result<u32> {
    match v.get("schema") {
        None => Ok(1),
        Some(s) => s
            .as_u64()
            .filter(|n| (1..=u64::from(u32::MAX)).contains(n))
            .map(|n| n as u32)
            .ok_or_else(|| {
                Error::new(
                    "invalid",
                    format!("{file}: schema must be a positive integer"),
                )
            }),
    }
}

/// Applies every upgrade step from the file's schema to [`CURRENT`] and
/// stamps the result. Returns the schema found. A newer file is left alone
/// (and `Ok(found)` tells the caller to read with care).
pub fn upgrade_json(file: &str, v: &mut Value) -> Result<u32> {
    let found = json_schema(file, v)?;
    for from in found..CURRENT {
        if let Some((_, _, step)) = STEPS.iter().find(|(f, n, _)| *f == file && *n == from) {
            step(v);
        }
    }
    if found < CURRENT
        && let Some(o) = v.as_object_mut()
    {
        o.insert("schema".into(), CURRENT.into());
    }
    Ok(found)
}

/// Best-effort schema of a TOML text: missing or unparsable = 1 (an
/// unparsable file is somebody else's error to report).
pub fn toml_schema(text: &str) -> u32 {
    text.parse::<toml::Table>()
        .ok()
        .and_then(|t| t.get("schema").and_then(|v| v.as_integer()))
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(1)
}

fn read_small(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 1 << 20 {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Call before writing `path` (JSON): refuses if the file on disk is newer.
/// A missing or unreadable file passes (nothing of value to protect).
pub fn guard_json(path: &Path, file: &str) -> Result<()> {
    let Some(text) = read_small(path) else {
        return Ok(());
    };
    let found = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| json_schema(file, &v).ok())
        .unwrap_or(1);
    if found > CURRENT {
        return Err(newer(file, found));
    }
    Ok(())
}

/// Same for a TOML file.
pub fn guard_toml(path: &Path, file: &str) -> Result<()> {
    let Some(text) = read_small(path) else {
        return Ok(());
    };
    let found = toml_schema(&text);
    if found > CURRENT {
        return Err(newer(file, found));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures(ver: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(format!("config-{ver}"))
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("stance-migrate-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        crate::store::ensure_private_dir(&d).unwrap();
        d
    }

    #[test]
    fn schema_missing_is_one() {
        let mut v: Value = serde_json::json!({"a": 1});
        assert_eq!(json_schema("x.json", &v).unwrap(), 1);
        assert_eq!(upgrade_json("x.json", &mut v).unwrap(), 1);
        assert!(json_schema("x.json", &serde_json::json!({"schema": 0})).is_err());
        assert!(json_schema("x.json", &serde_json::json!({"schema": "1"})).is_err());
        assert_eq!(toml_schema("a = 1\n"), 1);
        assert_eq!(toml_schema("schema = 3\n"), 3);
        assert_eq!(toml_schema("not toml ["), 1);
    }

    #[test]
    fn newer_files_are_never_overwritten() {
        let d = tmp("newer");
        let j = d.join("clip.json");
        std::fs::write(&j, r#"{"schema":2,"enabled":true}"#).unwrap();
        assert_eq!(
            guard_json(&j, "clip.json").unwrap_err().code,
            "newer_schema"
        );
        let t = d.join("deck.toml");
        std::fs::write(&t, "schema = 9\n").unwrap();
        assert_eq!(
            guard_toml(&t, "deck.toml").unwrap_err().code,
            "newer_schema"
        );
        // Current, old and missing files pass.
        std::fs::write(&j, r#"{"schema":1}"#).unwrap();
        assert!(guard_json(&j, "clip.json").is_ok());
        assert!(guard_json(&d.join("none.json"), "none.json").is_ok());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn fixtures_0_2_0_and_0_3_0_load() {
        for ver in ["0.2.0", "0.3.0"] {
            let dir = fixtures(ver);
            let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
            // features.json arrived in 0.3.0; 0.2.0 has none (= all on).
            if ver == "0.3.0" {
                let f = crate::feature::load_at(&dir.join("features.json")).unwrap();
                assert_eq!(f.get("clip"), Some(&false));
                assert_eq!(f.get("quick"), Some(&true));
            } else {
                assert!(!dir.join("features.json").exists());
            }
            let buttons = crate::deck::config::parse(&read("deck.toml")).unwrap();
            assert_eq!(buttons.len(), 2, "{ver}");
            let devs: crate::deck::pair::Devices =
                toml::from_str(&read("deck-devices.toml")).unwrap();
            assert_eq!(devs.devices.len(), 1, "{ver}");
            assert_eq!(devs.schema, 1);
            for f in ["memory.json", "panel.json"] {
                let mut v: Value = serde_json::from_str(&read(f)).unwrap();
                assert_eq!(upgrade_json(f, &mut v).unwrap(), 1, "{ver} {f}");
            }
            assert_eq!(toml_schema(&read("rules.toml")), 1);
            assert_eq!(toml_schema(&read("modes/desk.toml")), 1);
        }
    }
}
