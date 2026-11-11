//! Messages between the PC (`stance deck serve`) and a deck client (the
//! phone app, `stance deck test`). JSON inside Noise transport messages.
//!
//! The server is strict (unknown fields / types are errors and end the
//! session); clients should ignore what they don't know, so the server can
//! grow without breaking old apps.

use serde::{Deserialize, Serialize};

/// Bumped on incompatible changes; also part of the Noise prologue, so a
/// mismatch fails the handshake instead of being misparsed.
pub const SCHEMA: u32 = 1;
pub const PROLOGUE: &[u8] = b"stance-deck/1";

/// Session protocol negotiated inside the session (docs/adr/0003): `hello`
/// carries it, a v2 client answers with `caps`. The handshake stays /1.
pub const PROTO: u32 = 2;
/// Capability names (a closed set; unknown names are ignored).
pub const CAP_NAMES: &[&str] = &["clip.text", "clip.image", "file.push", "file.pull"];
/// First plaintext byte of a binary data frame (JSON starts with `{`).
pub const DATA_TAG: u8 = 0x00;
/// Data frame header: tag + u32 BE stream id.
pub const DATA_HEADER: usize = 5;
/// Largest payload a receiver accepts: 16384 frame − 16 tag − 5 header.
pub const PAYLOAD_MAX: usize = super::noise::POST_AUTH_MAX - 16 - DATA_HEADER;
/// Chunk size senders use.
#[cfg_attr(not(test), allow(dead_code))]
pub const CHUNK: usize = 16_000;

pub const MAX_BUTTONS: usize = 64;
pub const MAX_LABEL: usize = 40;
pub const MAX_ID: usize = 32;
pub const MAX_DEVICE_NAME: usize = 40;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMsg {
    /// First message after a pairing handshake: the name shown on the PC.
    Pair {
        name: String,
    },
    Press {
        id: String,
        req: u32,
    },
    Confirm {
        req: u32,
    },
    Revert {
        req: u32,
    },
    Ping {
        req: u32,
    },
    /// v2: once, after a `hello` with `proto >= 2`.
    Caps {
        proto: u32,
        caps: Vec<String>,
    },
    Bye,
}

/// Stream messages, the same in both directions (docs/adr/0003). The one
/// who sends `offer` is the sender of that stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case", deny_unknown_fields)]
pub enum StreamMsg {
    Offer {
        sid: u32,
        kind: String,
        size: u64,
        sha256: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime: Option<String>,
    },
    Accept {
        sid: u32,
        window: u64,
    },
    Decline {
        sid: u32,
        reason: String,
    },
    Window {
        sid: u32,
        bytes: u64,
    },
    End {
        sid: u32,
    },
    Done {
        sid: u32,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Cancel {
        sid: u32,
        reason: String,
    },
}

pub const STREAM_TYPES: &[&str] = &[
    "offer", "accept", "decline", "window", "end", "done", "cancel",
];

/// One decrypted plaintext.
#[derive(Debug, Clone, PartialEq)]
pub enum Plain<T> {
    /// A JSON message of the side's own type.
    Msg(T),
    /// A JSON stream message.
    Stream(StreamMsg),
    /// A binary data frame.
    Data { sid: u32, payload: Vec<u8> },
}

/// Splits a plaintext into a message, a stream message or a data frame.
/// Data frames must carry 1..=PAYLOAD_MAX bytes.
pub fn parse_plain<T: serde::de::DeserializeOwned>(b: &[u8]) -> Result<Plain<T>, String> {
    if b.first() == Some(&DATA_TAG) {
        if b.len() <= DATA_HEADER || b.len() - DATA_HEADER > PAYLOAD_MAX {
            return Err("bad data frame length".into());
        }
        let sid = u32::from_be_bytes([b[1], b[2], b[3], b[4]]);
        return Ok(Plain::Data {
            sid,
            payload: b[DATA_HEADER..].to_vec(),
        });
    }
    #[derive(Deserialize)]
    struct Tag<'a> {
        #[serde(borrow)]
        t: std::borrow::Cow<'a, str>,
    }
    let tag: Tag = serde_json::from_slice(b).map_err(|e| format!("bad message: {e}"))?;
    if STREAM_TYPES.contains(&tag.t.as_ref()) {
        serde_json::from_slice(b)
            .map(Plain::Stream)
            .map_err(|e| format!("bad message: {e}"))
    } else {
        serde_json::from_slice(b)
            .map(Plain::Msg)
            .map_err(|e| format!("bad message: {e}"))
    }
}

/// The plaintext of a data frame.
pub fn data_frame(sid: u32, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(DATA_HEADER + payload.len());
    v.push(DATA_TAG);
    v.extend_from_slice(&sid.to_be_bytes());
    v.extend_from_slice(payload);
    v
}

/// Our offered caps that the client also named, in our order.
pub fn agree_caps(offered: &[String], client: &[String]) -> Vec<String> {
    offered
        .iter()
        .filter(|c| client.contains(c) && CAP_NAMES.contains(&c.as_str()))
        .cloned()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ServerMsg {
    /// Sent once after every handshake (and again when deck.toml changes).
    Hello {
        schema: u32,
        /// Session protocol (2 since 0.6; absent from older PCs).
        #[serde(default = "proto_one")]
        proto: u32,
        /// Capabilities this PC offers (v2).
        #[serde(default)]
        caps: Vec<String>,
        /// PC name for the app's header.
        name: String,
        /// Addresses the app may try next time (the IP can change).
        hosts: Vec<String>,
        port: u16,
        buttons: Vec<Button>,
        state: State,
    },
    /// Full snapshot; `state.version` grows with every change.
    State {
        state: State,
    },
    /// The request was accepted (or refused before doing anything).
    Ack {
        req: u32,
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// The action finished.
    Result {
        req: u32,
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Pong {
        req: u32,
    },
    Bye {
        reason: String,
    },
    /// v2: the agreed capabilities, once, in reply to the client's `caps`.
    Caps {
        proto: u32,
        caps: Vec<String>,
    },
    /// A message a newer PC sends that this build does not know (clients
    /// ignore those). Never sent.
    #[serde(other, skip_serializing)]
    Unknown,
}

fn proto_one() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Button {
    pub id: String,
    pub label: String,
    /// Material Symbols name; the app may ignore it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// For toggles (night light, the active mode): lit or not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub version: u64,
    pub active_mode: Option<String>,
    /// A display change waiting for Keep / Revert.
    pub pending: Option<Pending>,
    /// Night light on, and its temperature.
    pub night: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    pub label: String,
    pub remaining_ms: u64,
}

/// Device names come from the phone: shown in the panel, so bounded and
/// stripped of control characters.
pub fn clean_name(s: &str) -> Option<String> {
    let t: String = s
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_DEVICE_NAME)
        .collect();
    let t = t.trim().to_string();
    (!t.is_empty()).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_messages_are_strict() {
        let m: ClientMsg = serde_json::from_str(r#"{"t":"press","id":"night","req":3}"#).unwrap();
        assert_eq!(
            m,
            ClientMsg::Press {
                id: "night".into(),
                req: 3
            }
        );
        assert!(serde_json::from_str::<ClientMsg>(r#"{"t":"exec","cmd":"x"}"#).is_err());
        assert!(
            serde_json::from_str::<ClientMsg>(r#"{"t":"press","id":"a","req":1,"x":1}"#).is_err()
        );
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name(" Pixel\n7 ").as_deref(), Some("Pixel7"));
        assert_eq!(clean_name("\u{7}\n"), None);
        assert_eq!(clean_name(&"x".repeat(100)).unwrap().len(), MAX_DEVICE_NAME);
    }
}
