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
    Bye,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ServerMsg {
    /// Sent once after every handshake (and again when deck.toml changes).
    Hello {
        schema: u32,
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
