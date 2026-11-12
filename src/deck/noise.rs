//! Noise handshakes and encrypted sessions over TCP.
//!
//! Every connection starts with one cleartext mode byte (also mixed into the
//! prologue, so it cannot be swapped): `SESSION` = Noise_IK with a paired
//! device's static key, `PAIR` = Noise_XXpsk3 with the one-time PSK from the
//! QR. Frames are a u16 big-endian length + payload; before the handshake
//! completes frames are capped at `PRE_AUTH_MAX`.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use snow::{Builder, HandshakeState, TransportState};

use super::proto::{PROLOGUE, Plain, parse_plain};
use crate::error::{Error, Result};

pub const SESSION_PATTERN: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";
pub const PAIR_PATTERN: &str = "Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s";
pub const MODE_SESSION: u8 = 1;
pub const MODE_PAIR: u8 = 2;
pub const PRE_AUTH_MAX: usize = 1024;
pub const POST_AUTH_MAX: usize = 16 * 1024;
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// The whole server-side handshake, however slowly its bytes trickle in.
pub const HANDSHAKE_TOTAL: Duration = Duration::from_secs(10);
/// Noise adds a 16-byte tag; one Noise message is at most 65535 bytes.
const NOISE_MAX: usize = 65535;

fn err(e: impl std::fmt::Display) -> Error {
    Error::new("deck", format!("{e}"))
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Keypair {
    /// base64url, 32 bytes each.
    pub public: String,
    pub private: String,
}

impl std::fmt::Debug for Keypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keypair")
            .field("public", &self.public)
            .finish_non_exhaustive()
    }
}

impl Keypair {
    pub fn generate() -> Result<Self> {
        let kp = Builder::new(SESSION_PATTERN.parse().map_err(err)?)
            .generate_keypair()
            .map_err(err)?;
        Ok(Self {
            public: B64.encode(&kp.public),
            private: B64.encode(&kp.private),
        })
    }

    fn private_bytes(&self) -> Result<Vec<u8>> {
        decode_key(&self.private)
    }
}

pub fn decode_key(s: &str) -> Result<Vec<u8>> {
    let k = B64
        .decode(s.trim())
        .map_err(|_| Error::new("deck", "malformed key"))?;
    if k.len() != 32 {
        return Err(Error::new("deck", "keys are 32 bytes"));
    }
    Ok(k)
}

pub fn encode_key(k: &[u8]) -> String {
    B64.encode(k)
}

/// Short human-checkable fingerprint of a public key, shown on both sides
/// before pairing is trusted: the first 8 base64url characters in two
/// groups. Trivial to compute on Android, identical everywhere.
pub fn fingerprint(public_b64: &str) -> String {
    let a: String = public_b64.chars().take(4).collect();
    let b: String = public_b64.chars().skip(4).take(4).collect();
    format!("{a}-{b}")
}

fn prologue(mode: u8) -> Vec<u8> {
    let mut p = PROLOGUE.to_vec();
    p.push(mode);
    p
}

// ---- framing ----------------------------------------------------------------

pub fn write_frame(s: &mut TcpStream, data: &[u8]) -> Result<()> {
    let len = u16::try_from(data.len()).map_err(|_| Error::new("deck", "frame too large"))?;
    let mut buf = Vec::with_capacity(2 + data.len());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(data);
    s.write_all(&buf).map_err(err)
}

/// One frame, finished within `HANDSHAKE_TIMEOUT` in total.
pub fn read_frame(s: &mut TcpStream, max: usize) -> Result<Vec<u8>> {
    read_frame_by(s, max, Instant::now() + HANDSHAKE_TIMEOUT)
}

/// One frame, finished by `deadline` (a byte every few seconds does not
/// keep it alive).
pub fn read_frame_by(s: &mut TcpStream, max: usize, deadline: Instant) -> Result<Vec<u8>> {
    let mut len = [0u8; 2];
    read_exact_by(s, &mut len, deadline)?;
    let len = u16::from_be_bytes(len) as usize;
    if len == 0 || len > max {
        return Err(Error::new("deck", "frame size out of bounds"));
    }
    let mut buf = vec![0u8; len];
    read_exact_by(s, &mut buf, deadline)?;
    Ok(buf)
}

fn read_exact_by(s: &mut TcpStream, buf: &mut [u8], deadline: Instant) -> Result<()> {
    let mut off = 0;
    while off < buf.len() {
        let left = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| Error::new("deck", "timed out"))?;
        s.set_read_timeout(Some(left)).map_err(err)?;
        let rest = buf.get_mut(off..).unwrap_or_default();
        match s.read(rest) {
            Ok(0) => return Err(Error::new("deck_closed", "connection closed")),
            Ok(n) => off += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(err(e)),
        }
    }
    Ok(())
}

// ---- session ----------------------------------------------------------------

/// An established encrypted channel. Any decrypt error ends it (no resync).
pub struct Session {
    stream: TcpStream,
    transport: TransportState,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("peer", &self.peer())
            .finish_non_exhaustive()
    }
}

impl Session {
    fn new(stream: TcpStream, hs: HandshakeState) -> Result<Self> {
        Ok(Self {
            stream,
            transport: hs.into_transport_mode().map_err(err)?,
        })
    }

    pub fn peer(&self) -> Option<SocketAddr> {
        self.stream.peer_addr().ok()
    }

    pub fn send<T: Serialize>(&mut self, msg: &T) -> Result<()> {
        let plain = serde_json::to_vec(msg).map_err(err)?;
        self.send_plain(&plain)
    }

    /// One plaintext (JSON, or a data frame from `proto::data_frame`).
    pub fn send_plain(&mut self, plain: &[u8]) -> Result<()> {
        if plain.len() + 16 > POST_AUTH_MAX {
            return Err(Error::new("deck", "message too large"));
        }
        let mut out = vec![0u8; plain.len() + 16];
        let n = self.transport.write_message(plain, &mut out).map_err(err)?;
        write_frame(&mut self.stream, &out[..n])
    }

    /// Next JSON message, or `Ok(None)` when `timeout` passes quietly.
    /// A data frame here is a protocol error (schema 1 sessions).
    pub fn recv<T: DeserializeOwned>(&mut self, timeout: Duration) -> Result<Option<T>> {
        match self.recv_any::<T>(timeout)? {
            None => Ok(None),
            Some(Plain::Msg(m)) => Ok(Some(m)),
            Some(_) => Err(Error::new(
                "deck_proto",
                "stream traffic before negotiation",
            )),
        }
    }

    /// Next plaintext as a message, stream message or data frame.
    pub fn recv_any<T: DeserializeOwned>(&mut self, timeout: Duration) -> Result<Option<Plain<T>>> {
        let Some(plain) = self.recv_plain(timeout)? else {
            return Ok(None);
        };
        parse_plain(&plain)
            .map(Some)
            .map_err(|e| Error::new("deck_proto", e))
    }

    /// Next decrypted plaintext, or `Ok(None)` when `timeout` passes quietly.
    pub fn recv_plain(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        self.stream.set_read_timeout(Some(timeout)).map_err(err)?;
        // Peek for the length so a timeout before any byte is not an error.
        let mut probe = [0u8; 1];
        match self.stream.peek(&mut probe) {
            Ok(0) => return Err(Error::new("deck_closed", "connection closed")),
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(None);
            }
            Err(e) => return Err(err(e)),
        }
        // A frame has started: it has `HANDSHAKE_TIMEOUT` in total to finish.
        let frame = read_frame(&mut self.stream, POST_AUTH_MAX)?;
        let mut plain = vec![0u8; frame.len()];
        let n = self
            .transport
            .read_message(&frame, &mut plain)
            .map_err(|_| Error::new("deck", "decryption failed"))?;
        plain.truncate(n);
        Ok(Some(plain))
    }

    pub fn shutdown(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

fn hs_write(hs: &mut HandshakeState, s: &mut TcpStream, payload: &[u8]) -> Result<()> {
    let mut buf = vec![0u8; NOISE_MAX];
    let n = hs.write_message(payload, &mut buf).map_err(err)?;
    write_frame(s, &buf[..n])
}

fn hs_read(hs: &mut HandshakeState, s: &mut TcpStream, deadline: Instant) -> Result<Vec<u8>> {
    // The server closes without a word when it refuses (unknown device,
    // used or expired pairing code): say that instead of an I/O error.
    let frame = read_frame_by(s, PRE_AUTH_MAX, deadline).map_err(|_| {
        Error::new(
            "deck_auth",
            "the PC refused the connection (not paired, or the pairing code was used or expired)",
        )
    })?;
    let mut buf = vec![0u8; NOISE_MAX];
    let n = hs
        .read_message(&frame, &mut buf)
        .map_err(|_| Error::new("deck_auth", "handshake failed"))?;
    buf.truncate(n);
    Ok(buf)
}

fn read_mode(s: &mut TcpStream, deadline: Instant) -> Result<u8> {
    let mut b = [0u8; 1];
    read_exact_by(s, &mut b, deadline)?;
    Ok(u8::from_be_bytes(b))
}

// ---- server side ------------------------------------------------------------

pub enum Incoming {
    /// A paired device: its static key (base64url).
    Device(Session, String),
    /// A pairing that succeeded (PSK accepted); the caller still needs the
    /// `pair` message before storing the device.
    Paired(Session, String),
}

/// Runs the server side of a handshake. `is_paired(key)` decides whether an
/// IK initiator's static key belongs to a paired device; `take_psk()` hands
/// out the current pairing PSK for one attempt (the caller limits attempts).
pub fn accept(
    mut stream: TcpStream,
    server: &Keypair,
    is_paired: &dyn Fn(&str) -> bool,
    take_psk: &dyn Fn() -> Option<[u8; 32]>,
) -> Result<Incoming> {
    stream
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(err)?;
    stream
        .set_write_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(err)?;
    let deadline = Instant::now() + HANDSHAKE_TOTAL;
    let private = server.private_bytes()?;
    match read_mode(&mut stream, deadline)? {
        MODE_SESSION => {
            let pro = prologue(MODE_SESSION);
            let mut hs = Builder::new(SESSION_PATTERN.parse().map_err(err)?)
                .local_private_key(&private)
                .map_err(err)?
                .prologue(&pro)
                .map_err(err)?
                .build_responder()
                .map_err(err)?;
            hs_read(&mut hs, &mut stream, deadline)?;
            let key = hs
                .get_remote_static()
                .map(encode_key)
                .ok_or_else(|| Error::new("deck_auth", "no client key"))?;
            if !is_paired(&key) {
                return Err(Error::new("deck_auth", "unknown device"));
            }
            hs_write(&mut hs, &mut stream, &[])?;
            Ok(Incoming::Device(Session::new(stream, hs)?, key))
        }
        MODE_PAIR => {
            let psk =
                take_psk().ok_or_else(|| Error::new("deck_auth", "no pairing window open"))?;
            let pro = prologue(MODE_PAIR);
            let mut hs = Builder::new(PAIR_PATTERN.parse().map_err(err)?)
                .local_private_key(&private)
                .map_err(err)?
                .psk(3, &psk)
                .map_err(err)?
                .prologue(&pro)
                .map_err(err)?
                .build_responder()
                .map_err(err)?;
            hs_read(&mut hs, &mut stream, deadline)?;
            hs_write(&mut hs, &mut stream, &[])?;
            hs_read(&mut hs, &mut stream, deadline)?;
            let key = hs
                .get_remote_static()
                .map(encode_key)
                .ok_or_else(|| Error::new("deck_auth", "no client key"))?;
            Ok(Incoming::Paired(Session::new(stream, hs)?, key))
        }
        _ => Err(Error::new("deck_auth", "unknown connection mode")),
    }
}

// ---- client side ------------------------------------------------------------

fn dial(addr: SocketAddr, mode: u8) -> Result<TcpStream> {
    let mut s = TcpStream::connect_timeout(&addr, HANDSHAKE_TIMEOUT).map_err(err)?;
    s.set_read_timeout(Some(HANDSHAKE_TIMEOUT)).map_err(err)?;
    s.set_write_timeout(Some(HANDSHAKE_TIMEOUT)).map_err(err)?;
    s.set_nodelay(true).ok();
    s.write_all(&[mode]).map_err(err)?;
    Ok(s)
}

/// Normal session with a PC we paired with (its static key known).
pub fn connect_ik(addr: SocketAddr, pc_public: &str, me: &Keypair) -> Result<Session> {
    let mut s = dial(addr, MODE_SESSION)?;
    let pro = prologue(MODE_SESSION);
    let mut hs = Builder::new(SESSION_PATTERN.parse().map_err(err)?)
        .local_private_key(&me.private_bytes()?)
        .map_err(err)?
        .remote_public_key(&decode_key(pc_public)?)
        .map_err(err)?
        .prologue(&pro)
        .map_err(err)?
        .build_initiator()
        .map_err(err)?;
    hs_write(&mut hs, &mut s, &[])?;
    hs_read(&mut hs, &mut s, Instant::now() + HANDSHAKE_TIMEOUT)?;
    Session::new(s, hs)
}

/// Pairing with the PSK from the QR. Checks that the PC's static key is
/// the one the QR named (a different responder could only be an impostor).
pub fn pair_xx(addr: SocketAddr, pc_public: &str, psk: &[u8; 32], me: &Keypair) -> Result<Session> {
    let mut s = dial(addr, MODE_PAIR)?;
    let pro = prologue(MODE_PAIR);
    let mut hs = Builder::new(PAIR_PATTERN.parse().map_err(err)?)
        .local_private_key(&me.private_bytes()?)
        .map_err(err)?
        .psk(3, psk)
        .map_err(err)?
        .prologue(&pro)
        .map_err(err)?
        .build_initiator()
        .map_err(err)?;
    hs_write(&mut hs, &mut s, &[])?;
    hs_read(&mut hs, &mut s, Instant::now() + HANDSHAKE_TIMEOUT)?;
    let got = hs.get_remote_static().map(encode_key);
    if got.as_deref() != Some(pc_public) {
        return Err(Error::new(
            "deck_auth",
            "the PC's key does not match the QR",
        ));
    }
    hs_write(&mut hs, &mut s, &[])?;
    Session::new(s, hs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deck::proto::{ClientMsg, ServerMsg};
    use std::net::TcpListener;
    use std::sync::Mutex;

    fn server_once(
        server: Keypair,
        paired: Vec<String>,
        psk: Option<[u8; 32]>,
    ) -> (SocketAddr, std::thread::JoinHandle<Result<String>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let h = std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let psk = Mutex::new(psk);
            let inc = accept(s, &server, &|k| paired.iter().any(|p| p == k), &|| {
                psk.lock().unwrap().take()
            })?;
            let (mut sess, key, kind) = match inc {
                Incoming::Device(s, k) => (s, k, "device"),
                Incoming::Paired(s, k) => (s, k, "paired"),
            };
            let m: Option<ClientMsg> = sess.recv(Duration::from_secs(2))?;
            sess.send(&ServerMsg::Pong { req: 7 })?;
            Ok(format!("{kind}:{key}:{m:?}"))
        });
        (addr, h)
    }

    #[test]
    fn pairing_then_session() {
        let pc = Keypair::generate().unwrap();
        let phone = Keypair::generate().unwrap();
        let psk = [7u8; 32];
        let (addr, h) = server_once(pc.clone(), vec![], Some(psk));
        let mut s = pair_xx(addr, &pc.public, &psk, &phone).unwrap();
        s.send(&ClientMsg::Pair {
            name: "Pixel".into(),
        })
        .unwrap();
        let r: Option<ServerMsg> = s.recv(Duration::from_secs(2)).unwrap();
        assert_eq!(r, Some(ServerMsg::Pong { req: 7 }));
        let out = h.join().unwrap().unwrap();
        assert!(
            out.starts_with(&format!("paired:{}", phone.public)),
            "{out}"
        );

        let (addr, h) = server_once(pc.clone(), vec![phone.public.clone()], None);
        let mut s = connect_ik(addr, &pc.public, &phone).unwrap();
        s.send(&ClientMsg::Ping { req: 1 }).unwrap();
        let _: Option<ServerMsg> = s.recv(Duration::from_secs(2)).unwrap();
        assert!(h.join().unwrap().unwrap().starts_with("device:"));
    }

    #[test]
    fn wrong_psk_unknown_device_and_impostor_fail() {
        let pc = Keypair::generate().unwrap();
        let phone = Keypair::generate().unwrap();
        let (addr, h) = server_once(pc.clone(), vec![], Some([1u8; 32]));
        assert!(
            pair_xx(addr, &pc.public, &[2u8; 32], &phone).is_err() || h.join().unwrap().is_err()
        );

        let (addr, h) = server_once(pc.clone(), vec![], None);
        let _ = connect_ik(addr, &pc.public, &phone);
        assert_eq!(h.join().unwrap().unwrap_err().code, "deck_auth");

        // QR names a different PC key than the one answering.
        let other = Keypair::generate().unwrap();
        let (addr, _h) = server_once(pc.clone(), vec![], Some([3u8; 32]));
        assert_eq!(
            pair_xx(addr, &other.public, &[3u8; 32], &phone)
                .unwrap_err()
                .code,
            "deck_auth"
        );
    }

    #[test]
    fn frames_are_bounded() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            read_frame(&mut s, PRE_AUTH_MAX)
        });
        let mut c = TcpStream::connect(addr).unwrap();
        c.write_all(&2000u16.to_be_bytes()).unwrap();
        assert!(h.join().unwrap().is_err());
        assert!(decode_key("short").is_err());
    }
}
