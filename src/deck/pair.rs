//! Pairing: QR payload format, paired devices store, PC key, local hosts.
//!
//! QR payload: `stance-deck:1?h=192.168.0.26,10.0.0.5&p=47800&k=<pc key>&s=<psk>&n=<pc name>`
//! (keys base64url without padding). Whoever sees the QR during the window
//! can pair, so the panel shows it only while asked and it expires in 120 s.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::noise::{Keypair, decode_key, encode_key};
use super::proto::clean_name;
use crate::error::{Error, Result};
use crate::store;

pub const SCHEME: &str = "stance-deck:1?";
pub const DEFAULT_PORT: u16 = 47800;
pub const PAIR_WINDOW_S: u64 = 120;
const MAX_HOSTS: usize = 6;
const MAX_DEVICES: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub struct QrPayload {
    pub hosts: Vec<String>,
    pub port: u16,
    pub pc_public: String,
    pub psk: [u8; 32],
    pub name: String,
}

fn bad() -> Error {
    Error::new("deck_qr", "not a stance pairing code")
}

/// Percent-encoding only for the PC name (hosts and keys are URL-safe).
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn dec(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

pub fn format(q: &QrPayload) -> String {
    format!(
        "{SCHEME}h={}&p={}&k={}&s={}&n={}",
        q.hosts.join(","),
        q.port,
        q.pc_public,
        encode_key(&q.psk),
        enc(&q.name)
    )
}

pub fn parse(s: &str) -> Result<QrPayload> {
    let rest = s.trim().strip_prefix(SCHEME).ok_or_else(bad)?;
    if rest.len() > 512 {
        return Err(bad());
    }
    let mut map = BTreeMap::new();
    for kv in rest.split('&') {
        let (k, v) = kv.split_once('=').ok_or_else(bad)?;
        if map.insert(k, v).is_some() {
            return Err(bad());
        }
    }
    let get = |k: &str| map.get(k).copied().ok_or_else(bad);
    let hosts: Vec<String> = get("h")?
        .split(',')
        .filter(|h| !h.is_empty())
        .map(|h| {
            h.parse::<Ipv4Addr>()
                .map(|_| h.to_string())
                .map_err(|_| bad())
        })
        .collect::<Result<_>>()?;
    if hosts.is_empty() || hosts.len() > MAX_HOSTS {
        return Err(bad());
    }
    let port: u16 = get("p")?.parse().map_err(|_| bad())?;
    let pc_public = get("k")?.to_string();
    decode_key(&pc_public)?;
    let psk: [u8; 32] = decode_key(get("s")?)?.try_into().map_err(|_| bad())?;
    let name = dec(get("n")?)
        .and_then(|n| clean_name(&n))
        .ok_or_else(bad)?;
    if port == 0 {
        return Err(bad());
    }
    Ok(QrPayload {
        hosts,
        port,
        pc_public,
        psk,
        name,
    })
}

/// QR as SVG (for the panel) and as terminal text.
pub fn render_svg(payload: &str) -> Result<String> {
    let code = qrcode::QrCode::new(payload.as_bytes())
        .map_err(|e| Error::new("deck_qr", e.to_string()))?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(256, 256)
        .quiet_zone(true)
        .build())
}

pub fn render_text(payload: &str) -> Result<String> {
    let code = qrcode::QrCode::new(payload.as_bytes())
        .map_err(|e| Error::new("deck_qr", e.to_string()))?;
    Ok(code
        .render::<qrcode::render::unicode::Dense1x2>()
        .quiet_zone(true)
        .build())
}

// ---- PC key and paired devices -----------------------------------------------

fn key_path() -> Result<PathBuf> {
    Ok(store::config_dir()?.join("deck-key.json"))
}

pub fn devices_path() -> Result<PathBuf> {
    Ok(store::config_dir()?.join("deck-devices.toml"))
}

/// This PC's static key, created on first use (0600, private dir).
pub fn pc_keypair() -> Result<Keypair> {
    let path = key_path()?;
    if path.exists() {
        let text = store::read_checked(&path)?;
        let kp: Keypair = serde_json::from_str(&text)
            .map_err(|_| Error::new("deck", "deck-key.json is damaged"))?;
        decode_key(&kp.public)?;
        decode_key(&kp.private)?;
        return Ok(kp);
    }
    store::ensure_private_dir(&store::config_dir()?)?;
    let kp = Keypair::generate()?;
    let data = serde_json::to_vec(&kp).map_err(|e| Error::new("io", e.to_string()))?;
    store::atomic_write(&path, &data, false)?;
    Ok(kp)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub name: String,
    pub key: String,
    pub paired_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Devices {
    /// File format version (src/migrate.rs); missing = 1.
    #[serde(default = "schema_one")]
    pub schema: u32,
    #[serde(default, rename = "device")]
    pub devices: Vec<Device>,
}

fn schema_one() -> u32 {
    1
}

pub fn load_devices() -> Result<Devices> {
    let path = devices_path()?;
    if !path.exists() {
        return Ok(Devices::default());
    }
    let text = store::read_checked(&path)?;
    let newer = crate::migrate::toml_schema(&text);
    let d: Devices = toml::from_str(&text).map_err(|e| {
        if newer > crate::migrate::CURRENT {
            crate::migrate::newer("deck-devices.toml", newer)
        } else {
            Error::new("deck", format!("deck-devices.toml: {}", e.message()))
        }
    })?;
    if d.devices.len() > MAX_DEVICES {
        return Err(Error::new("deck", "too many paired devices"));
    }
    for dev in &d.devices {
        decode_key(&dev.key)?;
    }
    Ok(d)
}

pub fn save_devices(d: &Devices) -> Result<()> {
    store::ensure_private_dir(&store::config_dir()?)?;
    crate::migrate::guard_toml(&devices_path()?, "deck-devices.toml")?;
    let mut d = d.clone();
    d.schema = crate::migrate::CURRENT;
    let body = toml::to_string(&d).map_err(|e| Error::new("io", e.to_string()))?;
    let text =
        format!("# paired deck devices (`stance deck devices`, `stance deck unpair NAME`)\n{body}");
    store::atomic_write(&devices_path()?, text.as_bytes(), false)
}

/// Adds (or re-keys, same name) a device; names are unique.
pub fn add_device(name: &str, key: &str) -> Result<Device> {
    let name = clean_name(name).ok_or_else(|| Error::new("deck", "device name is empty"))?;
    let mut d = load_devices()?;
    d.devices.retain(|x| x.name != name && x.key != key);
    if d.devices.len() >= MAX_DEVICES {
        return Err(Error::new(
            "deck",
            "too many paired devices; unpair one first",
        ));
    }
    let dev = Device {
        name,
        key: key.to_string(),
        paired_at: crate::apply::now_ms() / 1000,
    };
    d.devices.push(dev.clone());
    save_devices(&d)?;
    Ok(dev)
}

pub fn remove_device(name: &str) -> Result<bool> {
    let mut d = load_devices()?;
    let before = d.devices.len();
    d.devices.retain(|x| x.name != name);
    let removed = d.devices.len() != before;
    if removed {
        save_devices(&d)?;
    }
    Ok(removed)
}

/// LAN IPv4 addresses worth putting in the QR: up, not loopback, not
/// container / VM bridges.
pub fn local_hosts() -> Vec<String> {
    let mut out = Vec::new();
    // SAFETY: getifaddrs allocates a list we walk read-only and free once.
    unsafe {
        let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut head) != 0 {
            return out;
        }
        let mut cur = head;
        while !cur.is_null() {
            let ifa = &*cur;
            cur = ifa.ifa_next;
            if ifa.ifa_addr.is_null() || i32::from((*ifa.ifa_addr).sa_family) != libc::AF_INET {
                continue;
            }
            if ifa.ifa_flags & (libc::IFF_UP as u32) == 0
                || ifa.ifa_flags & (libc::IFF_LOOPBACK as u32) != 0
            {
                continue;
            }
            let name = std::ffi::CStr::from_ptr(ifa.ifa_name).to_string_lossy();
            if [
                "docker",
                "br-",
                "veth",
                "virbr",
                "vboxnet",
                "lxc",
                "tun",
                "wg",
                "tailscale",
            ]
            .iter()
            .any(|p| name.starts_with(p))
            {
                continue;
            }
            let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
            let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
            if !out.contains(&ip.to_string()) {
                out.push(ip.to_string());
            }
        }
        libc::freeifaddrs(head);
    }
    out.truncate(MAX_HOSTS);
    out
}

pub fn pc_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .and_then(|h| clean_name(&h))
        .unwrap_or_else(|| "stance".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> QrPayload {
        QrPayload {
            hosts: vec!["192.168.0.26".into(), "127.0.0.1".into()],
            port: 47800,
            pc_public: Keypair::generate().unwrap().public,
            psk: [9; 32],
            name: "H0dZ laptop & co".into(),
        }
    }

    #[test]
    fn payload_round_trip() {
        let q = sample();
        let s = format(&q);
        assert!(s.starts_with(SCHEME));
        assert_eq!(parse(&s).unwrap(), q);
        assert!(render_svg(&s).unwrap().contains("<svg"));
        assert!(!render_text(&s).unwrap().is_empty());
    }

    #[test]
    fn hostile_payloads_rejected() {
        let s = format(&sample());
        assert!(parse("https://evil.example/").is_err());
        assert!(parse(&s.replace("h=192.168.0.26", "h=evil.example")).is_err());
        assert!(parse(&s.replace("p=47800", "p=0")).is_err());
        assert!(parse(&format!("{s}&p=1")).is_err());
        assert!(parse(&s.replace("n=", "n=%0A")).is_ok_and(|q| !q.name.contains('\n')));
        assert!(parse(&format!("{SCHEME}{}", "a".repeat(600))).is_err());
    }

    #[test]
    fn hosts_skip_loopback() {
        assert!(!local_hosts().iter().any(|h| h.starts_with("127.")));
    }
}
