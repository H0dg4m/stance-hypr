# Phone deck

A paired phone shows buttons; pressing one runs a stance action on this PC.
Wi-Fi with QR pairing first; USB through `adb reverse` for those who want it.
The buttons come from `~/.config/stance/deck.toml` (see `examples/deck.toml`).
The Android app is in [stance-deck-android](https://github.com/H0dg4m/stance-deck-android); the byte-level format is in
[protocol.md](protocol.md) (with test vectors for other implementations).

## Transport

TCP, frames are a big-endian u16 length plus a Noise message. The first frame
before authentication is at most 1 KiB; after it, at most 16 KiB. A handshake
must finish within 10 s in total. Port 47800 by default; a loopback-only listen option
exists for USB users.

## Handshakes

Every connection starts with one cleartext **mode byte** before the first
frame: `1` = session (IK), `2` = pairing (XXpsk3). The prologue is the
bytes `stance-deck/1` followed by that mode byte, so neither the protocol
version nor the mode can be swapped without the handshake failing.

The PC key fingerprint shown on both sides is the first 8 characters of its
base64url public key, as `AbCd-EfGh`.

- **Pairing**: `Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s`. The 32-byte PSK is in
  the QR, single use, valid 120 s, good for 3 handshake attempts and ended by the first successful pairing. The
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

Edit the file by hand, in the panel (Settings, Phone buttons: add, remove,
reorder, "Reset to defaults"), or from the CLI: `stance deck buttons [--json]`
prints the current set (`custom` tells whether a file exists), `stance deck put
--data '{"button":[...]}'` validates and replaces it, `stance deck reset` goes
back to the defaults and keeps the old file as `deck.toml.bak`. A running
server picks the change up and sends a new `hello` to connected phones.

## Threat model

| Threat | Handling |
| --- | --- |
| Attacker on the same LAN | Everything is Noise-encrypted and authenticated. Without a paired key or the QR PSK the handshake fails; pre-auth frames are capped at 1 KiB, the whole handshake must finish within 10 s (each later frame within 5 s), at most 4 handshakes in flight (2 per IP) and 8 sessions; a session that sends no authenticated frame within 20 s, or then stays silent for 60 s, is closed. Failed handshakes are limited (10 per IP and 40 overall per minute; the overall limit only blocks IPs that failed themselves, so a flood cannot lock out a paired phone). |
| Replay | Per-session ephemeral keys plus Noise nonces; any decrypt failure ends the session (no resync). A replayed first handshake message cannot complete a session (the attacker lacks the keys); it holds a session slot for at most 20 s. |
| Eavesdropping on pairing | The QR is the out-of-band channel; anyone who sees the QR during the window can pair, so it is shown only while asked, expires in 120 s, is single use and its file is removed at expiry. |
| Stolen phone | `stance deck unpair NAME` (or the panel) revokes at once and drops live sessions. The reach of a stolen phone is limited to the buttons in `deck.toml`. Keep dangerous buttons out of the file; display changes still ask for confirmation and revert on their own. |
| Malicious QR (phone side) | It can only point the app at another host; no PC access results. The app shows the PC name and key fingerprint before pairing and treats labels as untrusted text. |
| DoS | See LAN row; also 10 presses per second per device, actions run one at a time per session (at most 4 queued; nothing queued runs after the session ends or the phone is unpaired), subprocesses killed at the timeout. |
| Malicious device name | Cleaned (control characters stripped, 40 chars) before it is stored or shown. |
| Local process on the PC | Loopback is not exempt from authentication. The pairing control socket and key files are 0600 in private directories. |
| USB (`adb reverse`) | The phone reaches the PC at 127.0.0.1; same handshakes, no extra trust. The per-IP limit sees loopback only, so it must not be the only guard. |

### Clipboard and file transfers (0.7)

Design: [ADR 0004](adr/0004-phone-transfers.md); wire format: protocol.md
§3.7. Everything is off until set in `~/.config/stance/transfer.toml`
(`stance deck transfer put --data JSON`):

| key | default | |
| --- | --- | --- |
| `clip_to_phone` | false | text you copy on the PC (the clip shelf's new copies) goes to connected phones; needs `stance clip` on |
| `clip_from_phone` | `"off"` | `"notify"`: the phone's "send clipboard" sets the PC clipboard and shows a notification with a preview |
| `files_from_phone` | `"off"` | `"ask"`: each file asks (notification with Accept / Decline, or `stance deck transfer accept ID`), 60 s, then declined |
| `download_dir` | `~/Downloads/stance` | received files land only here |
| `max_file_mb` | 512 | 1..512 |
| `clip_images` | false | clipboard images too (PNG, JPEG, WebP, GIF, up to 32 MiB), both ways as the two clip switches allow; the type comes from the content, not from the phone |
| `trusted_devices` | `[]` | phones (as paired, at most 16) whose files are saved without asking; saving is still announced. Trust ends when the phone is unpaired or paired again with a new key. A trusted phone can fill the download folder up to the free-space margin: only for your own phones |

`stance deck send FILE [--to PHONE]` offers a file (up to `max_file_mb`) to
that phone, or to the first connected one; the phone asks its user.
`stance deck transfers` lists recent transfers; a file nobody takes fails
after a few minutes. Text a phone sent is not sent back to that phone (it
can reach another phone, like any copy on the PC). The panel has the same
switches under Settings → Phone → Clipboard and files.
Clipboard text from the phone is the one exception to "every transfer
asks": the person pressed send on the phone, and the PC shows what it
received.

| Threat | Handling |
| --- | --- |
| A paired phone writes files the user did not want | Pressing buttons and writing files are different trust: file push is off until the user turns it on, and every transfer asks on the receiving side unless the user set an explicit per-device rule. |
| Huge or endless transfers | Per-kind size caps (text 1 MiB, images 32 MiB, files 512 MiB), credit-based flow control (at most 256 KiB granted ahead), 2 open streams per direction, 4 offers per 10 s, idle streams cancelled after 30 s. |
| Corrupt or swapped content | Data streams into a temp file in a private directory; the SHA-256 and size are checked before the file becomes visible; a mismatch deletes it. |
| Path tricks in names | Names are cleaned: no path separators, no leading dot or dash, no control characters, length capped, a suffix on collision. Files land only in the quarantine folder (`~/Downloads/stance` by default). |
| Running received content | Nothing is ever opened or executed automatically. Clipboard images must be an allowed MIME type (PNG, JPEG, WebP, GIF). |
| Secrets leaving the PC | Clipboard entries marked sensitive (password managers) and everything copied while `stance clip pause` is on are never sent. |
| Old apps | A client that did not negotiate v2 never receives stream traffic (old apps treat binary frames as fatal). |

Not covered: a compromised PC account (it owns the keys anyway), and denial
of service by someone who can saturate the Wi-Fi.

## The Android app

Pairing shows the PC name and key fingerprint and waits for the user to
compare it with the PC's before the handshake starts. The device key is
generated per PC and wrapped by an Android Keystore key. Pairing codes are
never put in screenshots (`FLAG_SECURE`). The connection exists only while a
PC screen is open; there is no background service. Strings from the PC are
shown as plain text, shortened and stripped of control characters. See
[stance-deck-android](https://github.com/H0dg4m/stance-deck-android) for building, signing and the
tests (Noise vectors, live interop, screenshots, an optional emulator run).

## Running and checking the server

- `stance deck status` (or `--json`): port, hosts, PC key fingerprint,
  whether a pairing window is open, paired phones with connected / last seen.
- Every new pairing raises a desktop notification with the phone's name and
  key fingerprint, so an unexpected pairing does not go unnoticed.
- `STANCE_DECK_DIR=/abs/path` moves the control socket and pairing QR out
  of `$XDG_RUNTIME_DIR/stance`, so a second server (tests) can run next to
  the real one.

## Testing without a phone

`stance deck test pair '<payload>'`, then `list`, `press ID`, `confirm`,
`revert`, `ping`, `watch SECONDS`. State lives in
`~/.config/stance/deck-test-client.json`.

For the real app without a phone: `scripts/e2e.sh` in stance-deck-android runs the debug APK
on an emulator against a nested Hyprland and a loopback server (`adb
reverse`), pairs, presses a layout button and checks Keep, Revert and the
automatic revert. It skips itself when adb, an AVD or the APK are missing.
