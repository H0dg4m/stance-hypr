# ADR 0003 — Deck protocol v2: negotiation, streams, transfer threat model (0.6.0)

Status: accepted, revised after review · 2026-11-11 (see "Revision")

## Context
0.7.0 adds clipboard sync and file send/receive between the phone and the
PC (KDE Connect-like). Schema 1 carries only small JSON messages (presses,
state), at most one Noise message (16 KiB) each. Bulk data needs streams,
flow control and limits, and the app and the PC are released separately
(two repositories), so each side must keep working with the other's older
version. 0.6.0 ships the protocol layer and its threat model; 0.7.0 turns
on the features that use it.

## Decisions

### 1. Version and capability negotiation (no handshake change)
- The Noise handshakes and the prologue `stance-deck/1` stay as they are:
  an old app still connects. Version 2 is negotiated inside the session.
- `hello` gains `proto: 2` and `caps: [..]` (what this PC offers). Old apps
  ignore both (clients ignore unknown fields).
- A v2 client answers with `caps {proto, caps}` once, as its first message
  after the first `hello`. The server replies `caps {proto: 2, caps}` with
  the **intersection**. Until then, and for clients that never send it, the
  session is schema 1: binary frames and stream messages are errors.
- Capability names: `clip.text`, `clip.image`, `file.push` (phone → PC),
  `file.pull` (PC → phone). 0.6.0 offers none (`caps: []`); the stream
  layer exists and is tested, the server declines every offer.
- An old server never sends `proto`: a v2 app stays on schema 1.

### 2. Streams
Messages (both directions; the sender of `offer` is the "sender"):

| t | fields | |
| --- | --- | --- |
| `offer` | `sid`, `kind`, `size`, `sha256`, `name`?, `mime`? | `kind` is a negotiated cap |
| `accept` | `sid`, `window` | receiver agrees; `window` bytes may be sent |
| `decline` | `sid`, `reason` | no data follows |
| `window` | `sid`, `bytes` | more credit |
| `end` | `sid` | all `size` bytes sent |
| `done` | `sid`, `ok`, `error`? | receiver checked size and SHA-256 |
| `cancel` | `sid`, `reason` | either side, any time |

- `sid` is a u32 chosen by the sender: odd from the phone, even from the
  PC, never reused in a session.
- Data travels in **binary frames**: the Noise plaintext starts with byte
  `0x00` (JSON always starts with `{`), then `sid` as u32 big-endian, then
  the payload (1..=16363 bytes; senders use 16000, see Revision). Payload is in order (TCP), no offsets.
- Protocol errors end the session (as in schema 1): data for an unknown or
  not-accepted sid, beyond the window, beyond `size`, an empty payload, a
  binary frame before negotiation, a reused sid, wrong parity.
- Limits: at most 2 open streams per direction; per-kind size caps
  (`clip.text` 1 MiB, `clip.image` 32 MiB, files configurable, default
  512 MiB); `name` ≤ 255 bytes, `mime` ≤ 127; `sha256` 64 lowercase hex.
- A receiver that refuses after `end` (hash or size mismatch) answers
  `done {ok: false}` and discards the data; the session continues.

### 3. Transfer threat model (enforced by 0.7.0, written now)
Added to docs/deck.md: per-transfer consent on the receiving side (or an
explicit per-device rule), size caps, streaming to a temp file in a
private directory with the hash checked before anything is visible, file
names cleaned (no path separators, no leading dot or dash, no control
characters, length cap, collision suffix), a quarantine folder
(`~/Downloads/stance` by default), never opened or executed automatically,
rate limits on offers, a MIME allow-list for clipboard images, and
clipboard entries marked sensitive (password managers, `clip pause`) never
leave the PC. A paired phone is trusted to press buttons, not to write
files: file push stays off until the user turns it on.

### 4. Test vectors
`tests/fixtures/deck-vectors.json` gains a `v2` section: the JSON of
`caps`/`offer`, the bytes of a binary frame, and that frame encrypted as
the second IK transport message (nonce 1), so the app can check its codec.
Existing values do not change.

## Revision (after review, before code)
- **Negotiation rules.** `caps` is accepted exactly once per session and
  only as a reply to a `hello` with `proto >= 2`; a second `caps`, or a
  stream message / binary frame before it, ends the session. The server
  answers once with the intersection; that answer is final. Later `hello`s
  (deck.toml changed) repeat the same `proto`/`caps`; if the offered caps
  ever change, the server ends the session instead of renegotiating.
  Unknown cap names are ignored on both sides. `hello` travels inside Noise
  transport, so `proto`/`caps` are authenticated: no downgrade by a
  network attacker.
- **The PC never sends** a stream message or a binary frame to a client
  that has not completed `caps` (the current Android app treats a non-JSON
  plaintext as fatal; Protocol.kt). New client fields go only into messages
  that exist after negotiation (the PC's strict parser drops unknown ones).
- **Frame math.** `Session::send` allows 16384 − 16 (tag) = 16368 bytes of
  plaintext; a binary frame has a 5-byte header, so the payload is at most
  16363. Senders use chunks of 16000 (`CHUNK`); receivers accept 1..=16363.
- **Credit.** `accept.window` ≤ 256 KiB; each payload byte uses one byte of
  credit; `window.bytes` adds credit. Total credit granted never exceeds
  `size` (so the receiver's buffering is bounded by its own grants); no
  overflow (u64, checked). Data beyond credit or beyond `size` ends the
  session.
- **Lifecycle.** `end` before `size` bytes is a protocol error; `done` is
  sent exactly once per accepted stream. On session end every open stream
  is cancelled and its temp data deleted. A stream with no data or credit
  for 30 s is cancelled. Frames for a sid closed in the last 8 closes are
  dropped silently (a `cancel` may cross data in flight); any other unknown
  sid ends the session. Running out of sids ends the session.
- **DoS.** At most 4 offers per 10 s per session (more: `decline` "too
  fast"), 2 open streams per direction;
  `size` checked against the per-kind cap before anything else.
- **Known limits in 0.6.** Actions still run inline in the session loop, so
  a slow action pauses reading (and stream credit); a peer stalled in the
  middle of a frame can hold the loop for up to 5 s. 0.7.0 moves actions to
  a worker thread before any real transfer is enabled.
- **Left for 0.7** (scan 0.6.0): at most 1 offer awaiting consent once
  consent is asynchronous; ending the session when the offered caps change
  (they are a constant in 0.6).
- **0.6.0 implements:** `proto`/`caps` in `hello`, the `caps` exchange,
  `Unknown` server messages in the Rust client (`#[serde(other)]`), the
  binary frame codec in `Session`, the stream state machine with credit
  accounting and its tests, the vectors. The server offers no caps and
  declines every offer. **Not in 0.6:** file writing, consent UI, per-device
  policy, threading.
