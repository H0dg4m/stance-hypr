# Phone deck

A paired phone shows buttons; pressing one runs a stance action on this PC.
Wi-Fi with QR pairing first; USB through `adb reverse` for those who want it.
The buttons come from `~/.config/stance/deck.toml` (see `examples/deck.toml`).

## Transport

TCP, frames are a big-endian u16 length plus a Noise message. The first frame
before authentication is at most 1 KiB; after it, at most 16 KiB. A handshake
must finish within 5 s. Port 47800 by default; a loopback-only listen option
exists for USB users.

## Handshakes

Every connection starts with one cleartext **mode byte** before the first
frame: `1` = session (IK), `2` = pairing (XXpsk3). The prologue is the
bytes `stance-deck/1` followed by that mode byte, so neither the protocol
version nor the mode can be swapped without the handshake failing.

The PC key fingerprint shown on both sides is the first 8 characters of its
base64url public key, as `AbCd-EfGh`.

- **Pairing**: `Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s`. The 32-byte PSK is in
  the QR, single use, valid 120 s, burned on the first failed attempt. The
  phone checks that the PC's static key equals the one in the QR. The PC
  stores the device (name + static public key) only after the handshake
  succeeded and a valid `pair` message arrived.
- **Sessions**: `Noise_IK_25519_ChaChaPoly_BLAKE2s`. The phone knows the PC's
  key from pairing; the PC looks the phone's key up after decrypting message
  1. Unknown key: the handshake fails. `unpair` takes effect at once: the
  PC re-reads `deck-devices.toml` on change and drops the sessions of removed
  devices.

QR payload: `stance-deck:1?h=<hosts,comma>&p=<port>&k=<pc key b64url>&s=<psk b64url>&n=<pc name>`.

## Messages

JSON inside Noise transport messages, `"t"` selects the type (full types in
`src/deck/proto.rs`, schema 1). The server is strict: unknown fields or types
end the session. Clients should ignore what they don't know.

Client to server: `pair {name}` (first message after pairing), `press {id, req}`,
`confirm {req}`, `revert {req}`, `ping {req}`, `bye`.

Server to client: `hello {schema, name, hosts, port, buttons, state}` after
every handshake and again when `deck.toml` changes; `state {state}` full
snapshots (`version` grows when something changes; the pending countdown
alone does not bump it, so clients count down locally from `remaining_ms`);
`ack {req, ok, error?}` request accepted or refused; `result {req, ok,
message?, error?}` action finished; `pong {req}`; `bye {reason}`.

`state` holds the active mode, the pending display change (label plus
remaining time: the phone shows Keep / Revert, which send `confirm` /
`revert`) and the night light temperature when on. Buttons for toggles carry
`on`.

## Actions

Whitelist, each mapped to arguments for our own binary (`current_exe`);
never a shell, stdin null, 30 s timeout then kill:

| action | needs | runs |
| --- | --- | --- |
| `mode` | `name` | `stance mode apply NAME` |
| `night` | `op` (toggle, on, off, set), `kelvin` | `stance night ...` |
| `brightness` | `monitor`, `value` (N, +N, -N) | `stance brightness MON VALUE` |
| `audio` | `sink` | `stance audio set SINK` |
| `layout` | `kind`, `monitor`, `reference` | `stance layout KIND MON [REF]` |
| `confirm`, `revert` | | `stance confirm`, `stance revert` |

Display changes keep their confirm timer (12 s) and revert on their own if
nobody confirms. Launching arbitrary apps is not part of the deck.

`deck.toml` is strict: unknown fields and action kinds are errors, at most 64
buttons, ids 1-32 of `[A-Za-z0-9_-]` and unique, labels 1-40 printable
characters, free-text arguments at most 64 characters, no control characters
and never starting with `-` (so nothing can pass for an option). Fields that
don't belong to the action are rejected. No file: night toggle, Keep, Revert
and one button per mode.

## Threat model

| Threat | Handling |
| --- | --- |
| Attacker on the same LAN | Everything is Noise-encrypted and authenticated. Without a paired key or the QR PSK the handshake fails; pre-auth frames are capped at 1 KiB, handshake time 5 s, handshakes rate-limited per IP and globally, connections capped. |
| Replay | Per-session ephemeral keys plus Noise nonces; any decrypt failure ends the session (no resync). Old recordings fail in a new session. |
| Eavesdropping on pairing | The QR is the out-of-band channel; anyone who sees the QR during the window can pair, so it is shown only while asked, expires in 120 s, is single use and its file is removed at expiry. |
| Stolen phone | `stance deck unpair NAME` (or the panel) revokes at once and drops live sessions. The reach of a stolen phone is limited to the buttons in `deck.toml`. Keep dangerous buttons out of the file; display changes still ask for confirmation and revert on their own. |
| Malicious QR (phone side) | It can only point the app at another host; no PC access results. The app shows the PC name and key fingerprint before pairing and treats labels as untrusted text. |
| DoS | See LAN row; also 10 presses per second per device, actions serialised per device, subprocesses killed at the timeout. |
| Malicious device name | Cleaned (control characters stripped, 40 chars) before it is stored or shown. |
| Local process on the PC | Loopback is not exempt from authentication. The pairing control socket and key files are 0600 in private directories. |
| USB (`adb reverse`) | The phone reaches the PC at 127.0.0.1; same handshakes, no extra trust. The per-IP limit sees loopback only, so it must not be the only guard. |

Not covered: a compromised PC account (it owns the keys anyway), and denial
of service by someone who can saturate the Wi-Fi.

## Testing without a phone

`stance deck test pair '<payload>'`, then `list`, `press ID`, `confirm`,
`revert`, `ping`, `watch SECONDS`. State lives in
`~/.config/stance/deck-test-client.json`.
