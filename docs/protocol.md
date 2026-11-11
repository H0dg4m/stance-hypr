# Protocols and file formats

For people writing a bar module, another client, or the phone app. Three
things are specified here: the `--json` envelope of the CLI, the guardian
(confirm-or-revert) state files, and the phone deck wire format. The deck's
purpose, actions and threat model are in [deck.md](deck.md).

## 1. CLI JSON envelope

Every command accepts `--json` and prints one line:

```json
{"ok":true,"schema":1,"data":{...}}
{"ok":false,"schema":1,"error":{"code":"usage","message":"unknown option `--x`"}}
```

`schema` is bumped only on breaking changes to these shapes. `code` is
stable (`usage`, `io`, `env`, `untested_version`, `deck`, `deck_auth`,
`deck_proto`, `deck_closed`, ...), `message` is for humans. Exit status is
0 on `ok`. Unknown fields may appear in future versions: ignore them.

`untested_version`: Hyprland is newer than every version stance was tested
with; commands that change screens (`set`, `layout`, `arrange`, `lid`,
`mode apply`, `daemon`) refuse unless `--force-version` is passed or
`STANCE_FORCE_VERSION` is set. Reading, `confirm` and `revert` always work.

`unsupported`: the backend lacks the capability the command needs (see
`stance caps`); nothing was changed. `--backend NAME` (or `STANCE_BACKEND`)
picks the backend: `hyprland` (default) or `fake` (tests, no compositor).

`stance caps --json` data: `{backend, caps:{displays, mirror, vrr, transform,
workspaces, compositor_options, persist, events}}`, all booleans. A UI hides
what is `false`.

`stance status --json` data:

| field | type | meaning |
| --- | --- | --- |
| `active_mode` | string or null | last applied mode |
| `daemon` | bool | hotplug daemon holds its lock |
| `pending` | object or null | see `PendingInfo` below |
| `warnings` | string[] | leftovers dropped at startup, etc. |

`PendingInfo` (also inside the result of any display change):
`{label, generation, deadline_ms, remaining_ms}`. `deadline_ms` is Unix time in
milliseconds; compare with your own clock only for ordering, count down from
`remaining_ms`.

`stance monitors --json` data: array of `{name, description, make, model,
serial, enabled, focused, dpms, width, height, refresh, x, y, scale,
transform, vrr, vrr_capable, mirror_of, modes:[{width,height,refresh}]}`.
`stance quick --json` data: `{night:{available,running,on,temperature,warm,
gamma}, brightness:[{monitor,kind:"ddc"|"backlight",value,max,percent}],
sinks:[{id,name,description,short,bus,profile,default,virtual}], errors:{}}`.
A section that failed is missing and explained in `errors`.

## 2. Guardian state files

Directory: `$STANCE_STATE_DIR`, else `$XDG_STATE_HOME/stance`, else
`~/.local/state/stance` (mode 0700). Files are written atomically
(temp file + rename), mode 0600. All mutation happens under an exclusive
`flock` on `lock` in that directory.

### pending.json

Exists exactly while a display change waits for Keep / Revert.

```json
{
  "session": "<HYPRLAND_INSTANCE_SIGNATURE>",
  "generation": 7,
  "deadline_ms": 1731260000000,
  "timeout_ms": 12000,
  "label": "2560x1440@180 on DP-1",
  "snapshot": { "monitors": [MonitorSpec], "options": {}, "power": null,
                "workspaces": [[1, "DP-1"]] },
  "active_mode_before": ["work", true]
}
```

`MonitorSpec`: `{name, enabled, width, height, refresh, x, y, scale,
transform, vrr (0|1|2), mirror (string|null)}`. The file is strict: unknown
fields are rejected by stance itself, so third-party writers are not
supported. Readers (bars) should only read.

### Lifecycle

1. A user-started change takes the lock, captures the touched state, applies
   it, verifies against `monitors -j`, then writes `pending.json` with a new
   `generation` and spawns a detached `stance guardian <generation>`.
2. A second change while pending keeps the oldest `snapshot` (merge: first
   value wins per monitor/option, newly touched items are added) and bumps
   `generation`; the older guardian notices and exits.
3. The guardian polls every 150 ms. It exits when `pending.json` is gone, its
   `generation` or `session` differs, or Hyprland no longer answers. At
   `deadline_ms` it takes the lock, re-reads, and if the generation is still
   its own restores the snapshot (verified, one retry), removes
   `pending.json` and notifies.
4. `confirm` removes `pending.json`. `revert` restores now. A `pending.json`
   from another `session` (compositor restarted) is dropped at the next
   command start with a warning.
5. Automatic changes (daemon rules, mode re-apply after reload) never create
   a pending state.

Watching for changes: use inotify on the **directory** (the file appears and
disappears; the panel's `StanceService.qml` does this with a `FileView`
filtered to `pending.json`). Counting down locally from `deadline_ms` is
fine; do not poll the CLI per frame.

### `stance events`

Streams one JSON object per line on stdout until the reader goes away
(SIGPIPE or a closed pipe ends it, also while idle) or it is killed. A UI
runs it as a child process and re-reads what changed; the lines carry no
data beyond what happened. No inotify watches in your own code, no polling.

```json
{"event":"hello","schema":1,"backend":"hyprland","monitor_events":true}
{"event":"monitors","change":"added","name":"DP-2"}
{"event":"pending"}
```

| `event` | when | then read |
| --- | --- | --- |
| `hello` | first line; `backend` is null without a compositor, `monitor_events` says whether `monitors` lines can come | |
| `monitors` | `change` is `added` / `removed` (with `name`, empty if unknown) or `reload` (compositor config reloaded) | `stance monitors` |
| `pending` | `pending.json` changed (change made, confirmed, reverted, expired) | `stance status` |
| `active` | the active mode changed | `stance status` |
| `clip` | `clip.rev` changed (the shelf changed) | `stance clip list` |
| `clip_config` | `clip.json` changed | `stance clip config show` |
| `features` | `features.json` changed | `stance feature list` |
| `panel` | `panel.json` changed | the file itself (see section 4) |
| `theme` | a file in `themes/` changed, or the folder appeared | `stance theme list` |
| `modes` | a file in `modes/` changed | `stance mode list` |
| `rules` | `rules.toml` changed | `stance rules` |
| `backend_closed` | the compositor went away; file events continue | |
| `backend_error` | the backend's event stream failed (`code`, `message`) | |

A change usually produces one line, sometimes two (a file written in two
steps): treat every line as "re-read", never as a counter. Unknown `event`
names may appear in later versions: ignore them. `schema` is bumped only on
breaking changes to these lines.

### Other files (read-only for clients)

`active.json` `{session, mode, extras}`: active mode. `memory.json`: last
known place of each screen. `night.json`: remembered warm temperature.
Their layout may change between minor versions; use the CLI instead.

### Deck CLI data

| command | `data` |
| --- | --- |
| `deck status` | `{port, loopback, hosts[], fingerprint, pairing, devices:[{name, paired_at, connected, last_seen}]}` (`last_seen` is null until the phone has connected; error `deck_not_running` without a server) |
| `deck devices` | `{devices:[{name, key, paired_at}], server: <deck status data> or null}` |
| `deck pair` | `{payload, expires_ms, svg, fingerprint}` |
| `deck buttons` | `{custom, button:[{id, label, icon?, action, …action fields}]}` (same keys as `deck.toml`) |
| `deck put --data '{"button":[…]}'` / `deck reset` | `{message, done}` |

`STANCE_DECK_DIR` (absolute path) replaces `$XDG_RUNTIME_DIR/stance` for the
control socket and the pairing QR.

## 3. Phone deck wire format (schema 1)

### 3.1 Transport and framing

TCP (default port 47800). First byte on the connection, in the clear: the
mode byte, `0x01` session, `0x02` pairing. Then both directions exchange
frames: `u16` big-endian length (1..65535, `0` is invalid) followed by that
many bytes. Limits enforced by the server: 1024 bytes per frame before the
handshake finished, 16384 after; handshake complete within 5 s.

Each frame is exactly one Noise message (no fragmenting). Handshake
payloads are empty. After the handshake every frame is one Noise transport
message whose plaintext is one JSON object (UTF-8, at most 16384 - 16 bytes).

### 3.2 Noise parameters

| | session | pairing |
| --- | --- | --- |
| pattern | `Noise_IK_25519_ChaChaPoly_BLAKE2s` | `Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s` |
| prologue | ASCII `stance-deck/1` then the mode byte `0x01` | same text then `0x02` |
| phone role | initiator, knows the PC's static public key | initiator, has the PSK |
| PSK | none | 32 bytes from the QR, applied at position 3 |
| messages | 1: phone to PC, 2: PC to phone | 1, 2, 3 (alternating, phone first) |

Keys are raw X25519, 32 bytes, base64url without padding wherever they are
stored or shown. Static keys of the phone are long-lived (one per PC, or one
per app; the PC identifies a device by its key only).

Session: the PC looks the phone's key up after decrypting message 1 and drops
the connection without a word when it is unknown. Pairing: the phone must
check that the static key the PC sent in message 2 equals the `k` of the QR
and abort otherwise. After the handshake the phone's first transport message
is `{"t":"pair","name":"<device name>"}`; the PC stores the device only after
it arrives, and answers with `hello`. A closed connection before any frame
means "refused": not paired, PSK used or expired.

Nonces are Noise's counters; there is no resync: any decrypt failure ends
the session. Rekeying is not used (sessions are short).

### 3.3 QR payload

```
stance-deck:1?h=<host>[,<host>...]&p=<port>&k=<pc public key>&s=<psk>&n=<pc name>
```

`k`, `s` are base64url (no padding) of 32 bytes; `n` is percent-encoded.
`h` lists IPv4/IPv6 literals or names to try in order. Unknown parameters:
ignore. Version `1` after the colon must match or the app refuses.

Fingerprint of a PC or phone key: the first 8 characters of the base64url
public key, split `AbCd-EfGh`. Show it to the user before trusting and keep
the PC key (not the fingerprint) for IK.

### 3.4 Messages

All objects have `"t"`. Client to server (the server rejects unknown fields
and types and ends the session):

| t | fields | notes |
| --- | --- | --- |
| `pair` | `name` | only as the first message of a pairing session |
| `press` | `id`, `req` | button id from `hello`; `req` is a client counter |
| `confirm` | `req` | keep the pending display change |
| `revert` | `req` | revert it now |
| `ping` | `req` | liveness, answered by `pong` |
| `bye` | | end politely |

Server to client (clients must ignore unknown fields and unknown `t`):

| t | fields |
| --- | --- |
| `hello` | `schema` (1), `name`, `hosts` string[], `port`, `buttons` [{`id`, `label`, `icon`?, `on`?}], `state` |
| `state` | `state`: `{version, active_mode, pending: {label, remaining_ms} or null, night: kelvin or null}` |
| `ack` | `req`, `ok`, `error`? |
| `result` | `req`, `ok`, `message`?, `error`? |
| `pong` | `req` |
| `bye` | `reason` |

Order for a press: `ack` (accepted or refused), later `result` (finished),
and a `state` if something changed. `hello` arrives after every handshake and
again when `deck.toml` changes (replace the whole button list). `state`
messages are full snapshots; ignore one whose `version` is lower than the last
seen. The pending countdown does not bump `version`: count down locally from
`remaining_ms` when a state with `pending` arrives. `req` values only need to
be unique per connection. At most 10 presses per second per device; actions
are serialised, so a second `press` may wait for the first.

Strings from the PC (labels, names, errors) are untrusted text: do not
interpret them as markup. Icons are Material Symbols names and may be ignored.

Example session after pairing (plaintext inside Noise):

```
phone: {"t":"ping","req":1}
pc:    {"t":"pong","req":1}
phone: {"t":"press","id":"night","req":2}
pc:    {"t":"ack","req":2,"ok":true}
pc:    {"t":"result","req":2,"ok":true,"message":"night light 3500 K"}
pc:    {"t":"state","state":{"version":4,"active_mode":null,"pending":null,"night":3500}}
```

### 3.5 Test vectors

`tests/fixtures/deck-vectors.json` holds, for both handshakes, fixed test-only
keys (static and ephemeral for both sides, the PSK), the prologue, every
handshake message, the handshake hash, and one transport message in each
direction with plaintext and ciphertext. An implementation passes when it,
given the same inputs and fixed ephemerals, produces the same bytes. They are
generated by `src/deck/vectors.rs` and checked by `cargo test`; regenerate
only on an intended protocol change (`STANCE_WRITE_VECTORS=1 cargo test
vectors`).

### 3.6 Client checklist

1. Pair: parse QR, connect (try `h` in order), send `0x02`, run XXpsk3,
   verify the PC key, send `pair`, store `{pc key, hosts, port, name, own key
   pair}`.
2. Connect: send `0x01`, run IK with the stored key, read `hello`.
3. Render buttons, react to `state`, show Keep / Revert while `pending` is set.
4. On any error reconnect with back-off; "refused" means the PC unpaired this
   phone: drop the stored PC.

## 4. Config file versions (schema 1)
Every file stance writes carries its own version: JSON top-level `"schema": 1`,
TOML `schema = 1`. A missing key means 1, so everything 0.2.0 and 0.3.0 wrote
is valid as it is (fixtures in `tests/fixtures/config-0.2.0/` and
`config-0.3.0/`; `src/migrate.rs` holds the upgrade steps, none yet).

| File | Where | Written by |
| --- | --- | --- |
| `panel.json` | config dir | the panel |
| `clip.json`, `features.json` | config dir | `clip config`, `feature` |
| `deck.toml`, `deck-devices.toml` | config dir | `deck put`, `deck pair` |
| `memory.json` | state dir | the daemon / `apply` |
| `modes/*.toml`, `rules.toml` | config dir | `mode put`, `rules put` |
| `index.json` (clipboard), theme files | data / config | already versioned (`schema`, `stance_theme`) |

Reading never rewrites a file. A file with a **newer** schema than the running
stance is read where that is safe (known keys only), but is never written:
the command fails with error code `newer_schema` and the file stays as it is.
Update stance, or move the file aside.
