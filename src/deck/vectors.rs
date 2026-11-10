//! Test vectors for other implementations (the Android app): fixed static
//! and ephemeral keys, fixed PSK, so every handshake message and the first
//! transport messages are byte-for-byte reproducible. The expected values
//! live in `tests/fixtures/deck-vectors.json`; set `STANCE_WRITE_VECTORS=1`
//! to regenerate that file after an intentional protocol change.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde_json::{Value, json};
use snow::Builder;

use super::noise::{MODE_PAIR, MODE_SESSION, PAIR_PATTERN, SESSION_PATTERN};
use super::proto::PROLOGUE;

fn bytes(seed: u8) -> [u8; 32] {
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = seed.wrapping_add(i as u8);
    }
    k
}

fn pubkey(private: &[u8; 32]) -> Vec<u8> {
    x25519_public(private)
}

/// X25519 base-point multiplication via snow's resolver.
fn x25519_public(private: &[u8; 32]) -> Vec<u8> {
    use snow::resolvers::{BoxedCryptoResolver, DefaultResolver};
    let r: BoxedCryptoResolver = Box::new(DefaultResolver);
    let mut dh = r.resolve_dh(&snow::params::DHChoice::Curve25519).unwrap();
    dh.set(private);
    dh.pubkey().to_vec()
}

fn h(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn generate() -> Value {
    let (pc_s, phone_s) = (bytes(0x10), bytes(0x40));
    let (pc_e, phone_e) = (bytes(0x70), bytes(0xA0));
    let psk = bytes(0xD0);
    let (pc_pub, phone_pub) = (pubkey(&pc_s), pubkey(&phone_s));
    let ping = br#"{"t":"ping","req":1}"#;
    let pong = br#"{"t":"pong","req":1}"#;

    // ---- IK ---------------------------------------------------------------
    let pro: Vec<u8> = [PROLOGUE, &[MODE_SESSION]].concat();
    let mut i = Builder::new(SESSION_PATTERN.parse().unwrap())
        .local_private_key(&phone_s)
        .unwrap()
        .remote_public_key(&pc_pub)
        .unwrap()
        .fixed_ephemeral_key_for_testing_only(&phone_e)
        .prologue(&pro)
        .unwrap()
        .build_initiator()
        .unwrap();
    let mut r = Builder::new(SESSION_PATTERN.parse().unwrap())
        .local_private_key(&pc_s)
        .unwrap()
        .fixed_ephemeral_key_for_testing_only(&pc_e)
        .prologue(&pro)
        .unwrap()
        .build_responder()
        .unwrap();
    let (mut b1, mut b2, mut tmp) = (vec![0u8; 512], vec![0u8; 512], vec![0u8; 512]);
    let n1 = i.write_message(&[], &mut b1).unwrap();
    r.read_message(&b1[..n1], &mut tmp).unwrap();
    let n2 = r.write_message(&[], &mut b2).unwrap();
    i.read_message(&b2[..n2], &mut tmp).unwrap();
    let ik_hash = h(i.get_handshake_hash());
    assert_eq!(ik_hash, h(r.get_handshake_hash()));
    let mut it = i.into_transport_mode().unwrap();
    let mut rt = r.into_transport_mode().unwrap();
    let mut t1 = vec![0u8; 128];
    let mut t2 = vec![0u8; 128];
    let tn1 = it.write_message(ping, &mut t1).unwrap();
    rt.read_message(&t1[..tn1], &mut tmp).unwrap();
    let tn2 = rt.write_message(pong, &mut t2).unwrap();
    it.read_message(&t2[..tn2], &mut tmp).unwrap();
    let ik = json!({
        "pattern": SESSION_PATTERN,
        "prologue_hex": h(&pro),
        "initiator_static_private_hex": h(&phone_s),
        "initiator_ephemeral_private_hex": h(&phone_e),
        "responder_static_private_hex": h(&pc_s),
        "responder_static_public_hex": h(&pc_pub),
        "responder_ephemeral_private_hex": h(&pc_e),
        "message1_hex": h(&b1[..n1]),
        "message2_hex": h(&b2[..n2]),
        "handshake_hash_hex": ik_hash,
        "initiator_to_responder": {"plaintext": String::from_utf8_lossy(ping), "ciphertext_hex": h(&t1[..tn1])},
        "responder_to_initiator": {"plaintext": String::from_utf8_lossy(pong), "ciphertext_hex": h(&t2[..tn2])},
    });

    // ---- XXpsk3 -----------------------------------------------------------
    let pro: Vec<u8> = [PROLOGUE, &[MODE_PAIR]].concat();
    let mut i = Builder::new(PAIR_PATTERN.parse().unwrap())
        .local_private_key(&phone_s)
        .unwrap()
        .psk(3, &psk)
        .unwrap()
        .fixed_ephemeral_key_for_testing_only(&phone_e)
        .prologue(&pro)
        .unwrap()
        .build_initiator()
        .unwrap();
    let mut r = Builder::new(PAIR_PATTERN.parse().unwrap())
        .local_private_key(&pc_s)
        .unwrap()
        .psk(3, &psk)
        .unwrap()
        .fixed_ephemeral_key_for_testing_only(&pc_e)
        .prologue(&pro)
        .unwrap()
        .build_responder()
        .unwrap();
    let n1 = i.write_message(&[], &mut b1).unwrap();
    r.read_message(&b1[..n1], &mut tmp).unwrap();
    let n2 = r.write_message(&[], &mut b2).unwrap();
    i.read_message(&b2[..n2], &mut tmp).unwrap();
    assert_eq!(i.get_remote_static().unwrap(), &pc_pub[..]);
    let mut b3 = vec![0u8; 512];
    let n3 = i.write_message(&[], &mut b3).unwrap();
    r.read_message(&b3[..n3], &mut tmp).unwrap();
    assert_eq!(r.get_remote_static().unwrap(), &phone_pub[..]);
    let xx_hash = h(i.get_handshake_hash());
    assert_eq!(xx_hash, h(r.get_handshake_hash()));
    let mut it = i.into_transport_mode().unwrap();
    let mut rt = r.into_transport_mode().unwrap();
    let pair = br#"{"t":"pair","name":"Test phone"}"#;
    let tn1 = it.write_message(pair, &mut t1).unwrap();
    rt.read_message(&t1[..tn1], &mut tmp).unwrap();
    let tn2 = rt.write_message(pong, &mut t2).unwrap();
    it.read_message(&t2[..tn2], &mut tmp).unwrap();
    let xx = json!({
        "pattern": PAIR_PATTERN,
        "prologue_hex": h(&pro),
        "psk_hex": h(&psk),
        "psk_position": 3,
        "initiator_static_private_hex": h(&phone_s),
        "initiator_static_public_hex": h(&phone_pub),
        "initiator_ephemeral_private_hex": h(&phone_e),
        "responder_static_private_hex": h(&pc_s),
        "responder_static_public_hex": h(&pc_pub),
        "responder_ephemeral_private_hex": h(&pc_e),
        "message1_hex": h(&b1[..n1]),
        "message2_hex": h(&b2[..n2]),
        "message3_hex": h(&b3[..n3]),
        "handshake_hash_hex": xx_hash,
        "initiator_to_responder": {"plaintext": String::from_utf8_lossy(pair), "ciphertext_hex": h(&t1[..tn1])},
        "responder_to_initiator": {"plaintext": String::from_utf8_lossy(pong), "ciphertext_hex": h(&t2[..tn2])},
    });

    json!({
        "comment": "Generated by src/deck/vectors.rs. Keys are test-only. Framing (u16 BE length) is not included: each *_hex is one Noise message.",
        "pc_public_b64url": B64.encode(&pc_pub),
        "phone_public_b64url": B64.encode(&phone_pub),
        "ik": ik,
        "xxpsk3": xx,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/deck-vectors.json"
    );

    #[test]
    fn vectors_match_the_fixture() {
        let v = generate();
        if std::env::var_os("STANCE_WRITE_VECTORS").is_some() {
            std::fs::write(PATH, serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap();
        }
        let have: Value = serde_json::from_str(&std::fs::read_to_string(PATH).unwrap()).unwrap();
        assert_eq!(
            v, have,
            "protocol bytes changed; rerun with STANCE_WRITE_VECTORS=1 if intended"
        );
    }
}
