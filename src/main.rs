mod backend;
mod doctor;
mod error;
mod hypr;
mod model;

use std::process::ExitCode;

use serde::Serialize;
use serde_json::json;

use error::{Error, Result};
use model::Monitor;

const USAGE: &str = "\
stance — bar-first display settings and modes for Hyprland

Usage:
  stance monitors [--json]   List monitors
  stance doctor [--json]     Check the environment (read-only)
  stance --version
  stance --help

Planned: set, layout, confirm, revert, persist, mode, daemon.";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let json = take_flag(&mut args, "--json");

    match run(&args, json) {
        Ok(code) => code,
        Err(e) => {
            if json {
                print_json(
                    &json!({ "ok": false, "error": { "code": e.code, "message": e.message } }),
                );
            } else {
                eprintln!("stance: {}", e.message);
            }
            ExitCode::from(2)
        }
    }
}

fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let before = args.len();
    args.retain(|a| a != flag);
    args.len() != before
}

fn run(args: &[String], json: bool) -> Result<ExitCode> {
    match args.first().map(String::as_str) {
        None | Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Some("-V" | "--version") => {
            println!("stance {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        Some("monitors") => {
            no_extra(args)?;
            let mons = backend::detect()?.monitors()?;
            if json {
                ok_json(&mons)
            } else {
                print_monitors(&mons)
            }
            Ok(ExitCode::SUCCESS)
        }
        Some("doctor") => {
            no_extra(args)?;
            let checks = doctor::run();
            let failed = checks.iter().any(|c| c.status == doctor::Status::Fail);
            if json {
                print_json(&json!({ "ok": !failed, "data": checks }));
            } else {
                for c in &checks {
                    let tag = match c.status {
                        doctor::Status::Ok => "  ok ",
                        doctor::Status::Info => "info ",
                        doctor::Status::Warn => "warn ",
                        doctor::Status::Fail => "FAIL ",
                    };
                    println!("{tag} {:<15} {}", c.id, c.message);
                }
            }
            Ok(if failed {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            })
        }
        Some(
            cmd @ ("set" | "layout" | "confirm" | "revert" | "persist" | "mode" | "daemon"
            | "guardian"),
        ) => Err(Error::new(
            "not_implemented",
            format!("`{cmd}` is not implemented yet"),
        )),
        Some(other) => Err(Error::new(
            "usage",
            format!("unknown command `{other}`; see `stance --help`"),
        )),
    }
}

fn no_extra(args: &[String]) -> Result<()> {
    match args.get(1) {
        Some(a) => Err(Error::new("usage", format!("unexpected argument `{a}`"))),
        None => Ok(()),
    }
}

fn print_json(v: &impl Serialize) {
    // Serialization of our own types cannot fail.
    println!("{}", serde_json::to_string(v).unwrap_or_default());
}

fn ok_json(data: &impl Serialize) {
    print_json(&json!({ "ok": true, "data": data }));
}

fn print_monitors(mons: &[Monitor]) {
    for m in mons {
        let state = if !m.enabled {
            "off".to_string()
        } else if let Some(src) = &m.mirror_of {
            format!("mirror of {src}")
        } else {
            format!(
                "{}x{}@{} at {},{} scale {}{}{}",
                m.width,
                m.height,
                m.refresh,
                m.x,
                m.y,
                m.scale,
                if m.transform != 0 {
                    format!(" rot {}", m.transform)
                } else {
                    String::new()
                },
                if m.vrr { " vrr" } else { "" },
            )
        };
        let focus = if m.focused { "*" } else { " " };
        println!("{focus} {:<8} {:<32} {state}", m.name, m.description);
    }
}
