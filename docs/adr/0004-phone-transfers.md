# ADR 0004 — Phone transfers: clipboard sync and files (0.7.0)

Status: accepted, revised after review · 2026-11-12 (see "Revision")

## Context
0.6 shipped the stream layer (ADR 0003) with no capability offered. 0.7
turns on clipboard sync and file transfer in both directions, under the
threat model in docs/deck.md. Two prerequisites from the 0.6 scan: actions
must not block the session loop, and consent must be asynchronous.

## Decisions

### 1. Session loop and actions
One thread per session still owns the socket (Noise transport state is
not split). Button actions go to a per-session worker thread over a
channel; results come back over a channel. The loop polls the socket with
a 100 ms timeout while a job, a consent or a stream is open (500 ms when
idle), sends queued data chunks as credit allows, and never waits on an
action or a person.

### 2. Settings: `~/.config/stance/transfer.toml` (schema 1)
| key | default | |
| --- | --- | --- |
| `clip_to_phone` | false | PC clipboard (the clip shelf's new items) goes to connected phones |
| `clip_from_phone` | false | the phone's "send clipboard" sets the PC clipboard |
| `files_from_phone` | `"off"` | `"off"` or `"ask"` (a notification with Accept / Decline, 60 s, then declined) |
| `download_dir` | `~/Downloads/stance` | received files only land here |
| `max_file_mb` | 512 | 1..=512 |

CLI `stance deck transfer [show | put --data JSON]`, panel Settings →
Phone. Offered caps follow the settings: `clip.text`/`clip.image` when
either clip switch is on, `file.push` when `files_from_phone = "ask"`,
`file.pull` always (the user starts those on the PC). When the settings
change, sessions whose offered caps change end with `bye "settings
changed"`; the app reconnects and negotiates again.

### 3. Clipboard
- Phone → PC (`clip_from_phone`): accepted without a prompt (the user
  pressed "send" on the phone). Text is UTF-8, ≤ 1 MiB; images must be
  PNG/JPEG/WebP/GIF by content sniffing (not by the offered mime). Applied
  with `wl-copy` (argv, no shell); the hash goes into an echo guard so the
  PC does not send it back.
- PC → phone (`clip_to_phone`): the session watches the clip shelf's rev;
  each new newest item created after the session started, not in the echo
  guard, of an agreed kind, is offered (one at a time; a newer one cancels
  an unsent older one). The shelf never stores password-manager copies or
  copies made while paused, so those never leave. Needs the clip feature.
- The phone app reads its clipboard only on an explicit button (Android
  limits background clipboard access anyway) and applies received text or
  images only while it is open.

### 4. Files
- Phone → PC (`file.push`): with `"ask"`, a notification (`notify-send`
  with actions, argv only) names the device, file name (cleaned) and size;
  Accept within 60 s or it is declined. At most 1 offer awaits consent;
  others are declined "busy". Data goes to
  `$XDG_STATE_HOME/stance/incoming/` (0700 dir, 0600 file, random name);
  after size + SHA-256 match it is moved to `download_dir` under the
  cleaned name (collision → ` (2)`, ` (3)`...), mode 0600, never opened.
  Failure or session end deletes the temp file.
- PC → phone (`file.pull`): `stance deck send FILE [--to DEVICE]` asks the
  server over the control socket; the server checks a regular file ≤ the
  cap, hashes it, and offers it to that connected device (or all
  connected ones). The phone asks its user and saves to its Downloads.
- Name cleaning: keep the last path component, drop control characters,
  `/`, `\`, leading dots, dashes and spaces; ≤ 120 bytes on a char
  boundary keeping the extension; empty → `file`.

### 5. Tests
`deck test` grows `send-clip TEXT`, `push FILE` and `receive` (accepts
offers, writes to a directory), so tests/fake.sh exercises every path
with a fake `wl-copy` and `notify-send` on PATH and isolated directories.

## Revision (after review, before code)
- **Scope 0.7.0:** text clipboard both ways, `file.pull` (PC → phone,
  started by the user on the PC) and `file.push` with `"ask"`. Images,
  the panel UI and per-device "always accept" rules move to 0.7.1.
  `clip_from_phone` is `"off"` (default) or `"notify"`: applied without a
  prompt (the phone user pressed send) but every apply shows a
  notification with a short cleaned preview, so a silently replaced
  clipboard (e.g. a command for a terminal) is visible. docs/deck.md
  states this exception to "every transfer asks".
- **Async consent:** `Decision::Defer` keeps the offer pending (counts
  toward the 2 open streams; not subject to the 30 s idle timer, its own
  60 s); `accept(sid)` / `decline(sid)` resolve it; data before accept
  stays a protocol error; a peer `cancel` clears it. **One pending consent
  for the whole server** (in `Shared`), keyed by a transfer id so a stale
  click cannot accept a later offer. The prompt is `notify-send --wait -A`
  on its own thread (killed at 60 s; no output = decline), with markup
  characters stripped. Fallback for daemons without actions: `stance deck
  transfer pending | accept ID | decline ID` over the control socket.
- **Incoming files:** the temp file is `download_dir/.stance-part-<random>`
  (same filesystem as the target; created `O_EXCL|O_NOFOLLOW`, 0600). On
  success it is linked to the cleaned name with a collision suffix
  (`link`, never overwrite) and unlinked; the final file is 0644 like any
  download. `download_dir` must be a real directory (not a symlink),
  created 0700 if missing. Stale `.stance-part-*` are swept at start.
  Before accepting, free space must exceed the size by 64 MiB; at most
  1 GiB of incoming temp data server-wide.
- **Names:** last path component; drop NUL/control, invisible and bidi
  characters (`clip::is_invisible`), `/` `\`; strip leading dots, dashes,
  spaces and trailing dots/spaces; ≤ 120 bytes keeping an extension of at
  most 16 bytes; empty → `file`.
- **Echo guard:** a server-wide set of text hashes from phones with a 30 s
  TTL, set before `wl-copy` runs; compared on the bytes the shelf stores.
- **PC → phone clip:** candidates are entries with `id` greater than the
  highest id at session start, picked by `created` (never `updated` or
  list order, so pins, `copy`, undo never send old items), not trashed,
  text/link kinds, ≤ 1 MiB; `enabled` and `paused` re-checked at offer
  time; not sent back to the phone it came from; one in flight per
  session, a newer item replaces an unsent older one.
- **`deck send FILE`:** the CLI opens the file (`O_NOFOLLOW` off: a
  symlink is resolved once by open), `fstat`s the fd (regular file only,
  ≤ cap), and passes the **path** to the server, which reopens with
  `O_NONBLOCK`, re-checks type/size, hashes on a worker thread, then
  streams exactly `size` bytes from that fd in credit-sized reads; size
  and mtime are re-checked at the end (changed → `cancel`). The command
  answers with a transfer id; `stance deck transfers` lists recent ones
  with their state (queued, offered, sending, done, declined, failed).
- **Settings:** `transfer.toml` is read into `Shared` (mtime-checked each
  tick, one snapshot for all sessions); offered caps are computed from it
  and passed to `agree_caps`. Sessions whose offered caps change cancel
  their streams and end with `bye "settings changed"`. Unknown keys, bad
  values or an unreadable file → everything off (logged).
- **Per-device cooldown:** after 3 declined/expired file offers from a
  device, its offers are declined "cooldown" for 5 minutes.
