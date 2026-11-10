//! `stance deck test`: a reference client that speaks the deck protocol
//! like the phone app will, so the PC side can be exercised without
//! Android. Keys and the paired PC live in
//! `$XDG_CONFIG_HOME/stance/deck-test-client.json` (0600).

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::noise::{self, Keypair, Session, fingerprint};
use super::pair;
use super::proto::{ClientMsg, SCHEMA, ServerMsg, State};
use crate::error::{Error, Result};
use crate::store;

const HELLO_WAIT: Duration = Duration::from_secs(8);
const RESULT_WAIT: Duration = Duration::from_secs(40);

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Saved {
    pub keypair: Keypair,
    pub pc_public: String,
    pub pc_name: String,
    pub hosts: Vec<String>,
    pub port: u16,
}

fn path() -> Result<std::path::PathBuf> {
    Ok(store::config_dir()?.join("deck-test-client.json"))
}

pub fn load() -> Result<Saved> {
    let p = path()?;
    let t = store::read_checked(&p).map_err(|_| {
        Error::new(
            "deck_client",
            "not paired yet: run `stance deck test pair <payload>`",
        )
    })?;
    serde_json::from_str(&t).map_err(|e| Error::new("deck_client", format!("{}: {e}", p.display())))
}

fn save(s: &Saved) -> Result<()> {
    let dir = store::config_dir()?;
    store::ensure_private_dir(&dir)?;
    let data =
        serde_json::to_vec_pretty(s).map_err(|e| Error::new("deck_client", e.to_string()))?;
    store::atomic_write(&path()?, &data, false)
}

fn addrs(hosts: &[String], port: u16) -> Vec<SocketAddr> {
    hosts
        .iter()
        .filter_map(|h| h.parse::<IpAddr>().ok())
        .map(|ip| SocketAddr::new(ip, port))
        .collect()
}

/// First reachable address wins; the last error is reported.
fn first_ok<T>(addrs: &[SocketAddr], mut f: impl FnMut(SocketAddr) -> Result<T>) -> Result<T> {
    let mut last = Error::new("deck_client", "no usable host address");
    for a in addrs {
        match f(*a) {
            Ok(v) => return Ok(v),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn expect_hello(s: &mut Session) -> Result<(String, Vec<super::proto::Button>, State)> {
    match s.recv::<ServerMsg>(HELLO_WAIT)? {
        Some(ServerMsg::Hello {
            schema,
            name,
            buttons,
            state,
            ..
        }) => {
            if schema != SCHEMA {
                return Err(Error::new(
                    "deck_client",
                    format!("server speaks schema {schema}, this client {SCHEMA}"),
                ));
            }
            Ok((name, buttons, state))
        }
        Some(ServerMsg::Bye { reason }) => Err(Error::new(
            "deck_closed",
            format!("server closed: {reason}"),
        )),
        Some(_) => Err(Error::new("deck_client", "expected hello")),
        None => Err(Error::new("deck_client", "no hello from the PC")),
    }
}

/// Pairs from a QR payload string and stores the result. Returns a report.
pub fn pair_qr(payload: &str, device_name: &str) -> Result<String> {
    let q = pair::parse(payload)?;
    let me = Keypair::generate()?;
    let mut s = first_ok(&addrs(&q.hosts, q.port), |a| {
        noise::pair_xx(a, &q.pc_public, &q.psk, &me)
    })?;
    s.send(&ClientMsg::Pair {
        name: device_name.to_string(),
    })?;
    let (pc_name, buttons, _) = expect_hello(&mut s)?;
    save(&Saved {
        keypair: me,
        pc_public: q.pc_public.clone(),
        pc_name: pc_name.clone(),
        hosts: q.hosts.clone(),
        port: q.port,
    })?;
    let _ = s.send(&ClientMsg::Bye);
    Ok(format!(
        "paired with {pc_name} (key {}), {} buttons",
        fingerprint(&q.pc_public),
        buttons.len()
    ))
}

fn connect(c: &Saved) -> Result<(Session, String, Vec<super::proto::Button>, State)> {
    let mut s = first_ok(&addrs(&c.hosts, c.port), |a| {
        noise::connect_ik(a, &c.pc_public, &c.keypair)
    })?;
    let (name, buttons, state) = expect_hello(&mut s)?;
    Ok((s, name, buttons, state))
}

fn describe(state: &State) -> String {
    let mut parts = vec![
        format!("mode: {}", state.active_mode.as_deref().unwrap_or("none")),
        match state.night {
            Some(k) => format!("night: {k} K"),
            None => "night: off".into(),
        },
    ];
    if let Some(p) = &state.pending {
        parts.push(format!(
            "pending: {} ({:.0} s left)",
            p.label,
            p.remaining_ms as f64 / 1000.0
        ));
    }
    format!("{} (v{})", parts.join(", "), state.version)
}

/// Lists the buttons and the state.
pub fn list() -> Result<String> {
    let (name, buttons, state) = fetch()?;
    let mut out = format!("{name}: {}\n", describe(&state));
    for b in buttons {
        let lit = match b.on {
            Some(true) => " [on]",
            _ => "",
        };
        out += &format!("  {:<20} {}{lit}\n", b.id, b.label);
    }
    Ok(out.trim_end().to_string())
}

fn fetch() -> Result<(String, Vec<super::proto::Button>, State)> {
    let (mut s, name, buttons, state) = connect(&load()?)?;
    let _ = s.send(&ClientMsg::Bye);
    Ok((name, buttons, state))
}

fn out(json: bool, text: String, data: serde_json::Value) {
    if json {
        println!(
            "{}",
            serde_json::json!({ "ok": true, "schema": 1, "data": data })
        );
    } else {
        println!("{text}");
    }
}

/// Entry for `stance deck test ARGS`: pair PAYLOAD | buttons | press ID |
/// confirm | revert | ping | watch [SECONDS].
pub fn run(args: &[String], name: Option<&str>, json: bool) -> Result<()> {
    let usage = || {
        Error::new(
            "usage",
            "stance deck test pair <payload> | buttons | press <id> | confirm | revert | ping | watch [seconds]",
        )
    };
    let msg = |m: String| {
        let d = serde_json::json!({ "message": m });
        out(json, m, d);
    };
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["pair", payload] => msg(pair_qr(payload, name.unwrap_or("stance-test"))?),
        ["buttons"] => {
            if json {
                let (n, buttons, state) = fetch()?;
                out(
                    true,
                    String::new(),
                    serde_json::json!({ "name": n, "buttons": buttons, "state": state }),
                );
            } else {
                println!("{}", list()?);
            }
        }
        ["press", id] => msg(press(id)?),
        ["confirm"] => msg(confirm()?),
        ["revert"] => msg(revert()?),
        ["ping"] => msg(ping()?),
        ["watch"] => watch(30)?,
        ["watch", secs] => watch(secs.parse().map_err(|_| usage())?)?,
        _ => return Err(usage()),
    }
    Ok(())
}

/// Sends one request and waits for its `result`; returns the message.
fn request(make: impl FnOnce(u32) -> ClientMsg) -> Result<String> {
    let (mut s, _, _, _) = connect(&load()?)?;
    let req = 1;
    s.send(&make(req))?;
    let end = Instant::now() + RESULT_WAIT;
    let mut msg = None;
    while Instant::now() < end {
        match s.recv::<ServerMsg>(Duration::from_millis(500))? {
            Some(ServerMsg::Ack {
                req: r,
                ok: false,
                error,
            }) if r == req => {
                return Err(Error::new(
                    "deck_refused",
                    error.unwrap_or_else(|| "refused".into()),
                ));
            }
            Some(ServerMsg::Result {
                req: r,
                ok,
                message,
                error,
            }) if r == req => {
                if !ok {
                    return Err(Error::new(
                        "deck_failed",
                        error.unwrap_or_else(|| "failed".into()),
                    ));
                }
                msg = Some(message.unwrap_or_else(|| "ok".into()));
                break;
            }
            Some(ServerMsg::Pong { req: r }) if r == req => {
                msg = Some("pong".into());
                break;
            }
            Some(ServerMsg::Bye { reason }) => {
                return Err(Error::new(
                    "deck_closed",
                    format!("server closed: {reason}"),
                ));
            }
            _ => {}
        }
    }
    let mut msg =
        msg.ok_or_else(|| Error::new("deck_client", "timed out waiting for the result"))?;
    // A display change pushes a pending state right after.
    if let Some(ServerMsg::State { state }) = s.recv::<ServerMsg>(Duration::from_millis(1200))?
        && let Some(p) = state.pending
    {
        msg += &format!(
            " (pending: {}, {:.0} s to confirm)",
            p.label,
            p.remaining_ms as f64 / 1000.0
        );
    }
    let _ = s.send(&ClientMsg::Bye);
    Ok(msg)
}

pub fn press(id: &str) -> Result<String> {
    request(|req| ClientMsg::Press {
        id: id.to_string(),
        req,
    })
}

pub fn confirm() -> Result<String> {
    request(|req| ClientMsg::Confirm { req })
}

pub fn revert() -> Result<String> {
    request(|req| ClientMsg::Revert { req })
}

pub fn ping() -> Result<String> {
    request(|req| ClientMsg::Ping { req })
}

/// Prints state pushes for `secs` seconds (like the phone's live view).
pub fn watch(secs: u64) -> Result<()> {
    let (mut s, name, _, state) = connect(&load()?)?;
    println!("{name}: {}", describe(&state));
    let end = Instant::now() + Duration::from_secs(secs.min(3600));
    while Instant::now() < end {
        match s.recv::<ServerMsg>(Duration::from_millis(500))? {
            Some(ServerMsg::State { state }) => println!("{}", describe(&state)),
            Some(ServerMsg::Hello { name, buttons, .. }) => {
                println!("{name}: buttons changed ({})", buttons.len())
            }
            Some(ServerMsg::Bye { reason }) => {
                return Err(Error::new(
                    "deck_closed",
                    format!("server closed: {reason}"),
                ));
            }
            _ => {}
        }
    }
    let _ = s.send(&ClientMsg::Bye);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_state() {
        let st = State {
            version: 3,
            active_mode: Some("work".into()),
            pending: Some(super::super::proto::Pending {
                label: "layout".into(),
                remaining_ms: 9000,
            }),
            night: None,
        };
        let d = describe(&st);
        assert!(
            d.contains("mode: work")
                && d.contains("night: off")
                && d.contains("pending: layout (9 s left)")
                && d.contains("v3")
        );
    }

    #[test]
    fn only_ip_hosts_are_dialled() {
        let a = addrs(
            &["127.0.0.1".into(), "evil.example".into(), "::1".into()],
            47800,
        );
        assert_eq!(a.len(), 2);
        assert!(first_ok::<()>(&[], |_| Ok(())).is_err());
    }
}
