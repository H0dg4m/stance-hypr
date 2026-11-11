# Changelog

## Unreleased

### 0.7.1 work
- Clipboard images both ways (`clip_images` in transfer.toml; PNG, JPEG, WebP, GIF up to 32 MiB; the type is sniffed from the bytes).
- `trusted_devices`: files from those phones are saved without a prompt (still announced).
- Panel: Settings → Phone → Clipboard and files gets "Images too" and a per-phone "Save files without asking".
- `deck test send-image FILE`; tests for both image directions, non-image bytes, trusted phones.

### Phone transfers (0.7.0 work, docs/adr/0004-phone-transfers.md)
- Clipboard text both ways and files both ways with paired phones, all off by default: `stance deck transfer [show | put --data JSON]` (`transfer.toml`), `stance deck send FILE [--to PHONE]`, `stance deck transfers`, `stance deck transfer pending | accept ID | decline ID`.
- Files from a phone always ask (notification with Accept / Decline, one prompt at a time server-wide, 60 s), land in `~/Downloads/stance` through a temp file in the same folder, are checked (size, SHA-256) before they get their cleaned name, never overwrite, and are never opened. Cooldown after 3 refusals, free-space and 1 GiB in-flight limits.
- Clipboard text from a phone is applied with a notification showing it; text a phone sent is not sent back to it; only copies made after the phone connected go out (never pins, undo or old items).
- Button actions run on a worker thread: a slow action no longer stalls the session.
- `deck test send-clip | push | receive`; tests/fake.sh covers every path with fake `wl-copy` and `notify-send`.

### Phone deck protocol v2 (0.6.0 work, docs/adr/0003-deck-protocol-v2.md)
- `hello` carries `proto: 2` and `caps`; a v2 client answers with `caps` once and gets the agreed set. Old apps and old PCs keep working (the handshakes do not change).
- Streams: `offer`/`accept`/`decline`/`window`/`end`/`done`/`cancel` and binary data frames with credit-based flow control, size caps, SHA-256 check, sid rules, offer rate limit, idle timeout (`src/deck/stream.rs`). stance 0.6 offers no capability and declines every offer; clipboard and file transfers come in 0.7.
- Threat model for transfers in docs/deck.md; wire format in docs/protocol.md §3.7; `v2` test vectors.
- `stance deck test` negotiates v2; `deck test offer [kind]` and `deck test stray-data` check a running PC (tests/fake.sh runs them in CI).

### Widget board (0.5.0 work)
- The panel is a board of widgets (map, screen, night light, brightness,
  audio, modes, clipboard, phone), each in size S, M or L; see `docs/widgets.md`
- `panel.json` schema 2: ordered `board`, `known`, `onboarded`; schema 1
  `show.*` switches migrate to the board on read
- Settings -> Layout: presets (Minimal, Standard, Everything), reorder,
  resize, remove and add widgets, reset layout; General has "Run setup again"
- The old "Show in panel" switches are gone (Layout replaces them)
- Settings code split into one `StanceSettings<Name>.qml` per category;
  chips, switches and segmented controls take keyboard focus (Tab, Space or
  Enter, arrows)

### Automatic light/dark (0.4, ADR 0001 step 7)
- `stance theme system [--json]` prints `dark`, `light` or `unknown` (XDG
  settings portal, then GNOME gsettings)
- Panel: Appearance has an "Automatic (follow system)" tile and a light and a
  dark theme picker; `panel.json` gets `theme: "auto"`, `themeLight`,
  `themeDark`. The panel re-reads the system setting when it opens

### Config schema (0.4, ADR 0001 step 6)
- Every file stance writes carries a version (`"schema": 1` / `schema = 1`):
  `clip.json`, `features.json`, `deck.toml`, `deck-devices.toml`,
  `memory.json`, `panel.json`. Missing = 1, so 0.2.0 and 0.3.0 files are read
  unchanged and reads never write
- A file from a newer stance is read where safe and never overwritten: the
  write fails with error code `newer_schema`
- `src/migrate.rs` (upgrade-step registry) and 0.2.0 / 0.3.0 fixtures with
  load tests

### Core split (0.4.0 work, docs/adr/0001-core-split.md)
- `--backend NAME` / `STANCE_BACKEND` choose the compositor backend
  (`hyprland`, default; `fake`, an in-memory one for tests: state in
  `<state dir>/fake.json` or `STANCE_FAKE_STATE`)
- Backend capabilities: `stance caps [--json]`, error code `unsupported`
  (mode options on a backend without them, `persist`, mirroring, ...);
  `OptKey` is neutral (hyprlang paths, Lua/legacy rendering and `getoption`
  parsing now live in `src/backend/hyprland.rs`); `allow_tearing` stays the
  mode-file key
- Internal: `src/commands.rs` returns serializable results (monitors, status,
  caps, set, layout, arrange, lid, confirm, revert); one renderer prints them
  (CLI text and JSON byte-identical to 0.3)
- Internal: night light talks through `night::Link` (hyprsunset socket
  behind the Hyprland backend), `Backend::compat_check()` replaces the
  version gate in `main`; no module outside `backend/hyprland.rs` and
  `doctor.rs` uses `hypr::Instance`
- Internal: `Backend::events()` yields neutral events (monitor added /
  removed, config reloaded) from an `EventSource`; the daemon's loop takes
  an event source and a clock, so debounce, burst pause and reload are
  unit-tested with scripted events and virtual time
- `stance events`: JSON lines for monitor changes (backend events) and the
  files the panel watches (pending, active mode, clip.rev, features, panel,
  themes, modes, rules) through inotify; no idle cost, ends with its reader
  (docs/protocol.md). The panel can use it instead of folder watches
- `tests/fake.sh`: set / confirm / revert / timeout / mode apply without a
  compositor (runs in CI)

### Assurance (0.3.1 work)
- Tests: password-manager (`wl-copy --sensitive`) copies, 30 MB image kept and
  34 MB dropped, capture watcher exits with the compositor, 0.2.0 → 0.3.0
  upgrade (`tests/upgrade.sh`, also in CI); all 16 feature on/off combinations
  checked in the panel
- CI: QML must parse, every panel string has a Turkish translation
  (`scripts/i18n-check.py`), shell scripts parse
- docs/troubleshooting.md for every doctor message; new screenshots; the
  Turkish README matches the English one
- Panel: reset keeps the open settings page, narrow navigation shows labels on
  hover, a turned-off shelf shows only how to turn it on

### Getting started and reporting
- The phone app moved to its own repository,
  [stance-deck-android](https://github.com/H0dg4m/stance-deck-android); this
  repository keeps the server, the protocol and the test vectors
- `stance setup [waybar|quickshell|end4] [--dry-run]`: detects your bar and
  prints the exact steps; enables only stance's own units (new
  `stance-panel.service` runs the standalone panel for Waybar and other bars)
- `stance doctor --report`: versions, features and checks in one block for bug
  reports; issue template, CONTRIBUTING, support table in the README
- `stance clip pause [15m|1h|30s]` / `resume`: capture stops until the
  deadline, which is kept on disk; shown by doctor and the panel
- Fixed: a freshly written helper could fail to start (ETXTBSY) under load
- Scan fixes (docs/scans/0.3.0.md): clip config writes are locked; the store is
  capped (5000 items / 1 GiB); a damaged index no longer deletes content and a
  newer one is refused; `https://a\\@b` is not a link to b; tampered index
  entries never become paths; `clip purge`; terminal-safe `clip get`; PNG sizes
  checked; theme names refuse bidi characters, imports are size-capped and
  saved themes limited to 200; every external tool has a timeout; panel text
  that comes from outside is shown as plain text; `doctor --report` hides your
  home path and user name

### Themes, language, features
- 17 built-in panel themes (Stance dark/light, Catppuccin Mocha/Macchiato/
  Frappé/Latte, Nord, Gruvbox dark/light, Dracula, Tokyo Night, Rosé Pine and
  Dawn, Everforest, Kanagawa, One Dark, Solarized dark); `stance theme
  list|show|put|import|export|rm`, one strict JSON format for all of them
  (docs/themes.md)
- `stance feature list|on|off clip|deck|rules|quick`: people who do not want a
  part turn it off; its user units are stopped and the panel hides it
- Short commands end quietly on a closed pipe (`stance theme list | head`)

### Clipboard shelf (`stance clip`)
- Images, texts and links you copy, on three shelves; inspired by
  ClipBoardLine (MIT), not a fork. Off until enabled; see docs/clip.md
- Captured by `wl-paste --watch stance clip ingest` (two user units,
  `contrib/stance-clip-text.service` and `-image.service`); copies that a
  password manager marks sensitive, empty clipboards and unknown states are
  never stored
- Images typed by magic bytes (PNG, JPEG, WebP, GIF, BMP; SVG refused), size
  caps (text 256 KiB, image 32 MiB), file-manager copies ignored
- Same content is one entry (moved to the top), pins survive retention
  (1/3/7/15/30 days), `rm` is undoable, `clear [--all]`
- Store is private (`~/.local/share/stance/clip`, 0700/0600), index holds
  only 200-character previews; `clip.rev` tells the panel when to reload
- `doctor` reports the shelf, its watchers and that cliphist (if running)
  keeps what stance skips

## 0.2.0 – 2026-11-11

The display tool that can't strand you on a black screen, now with quick
controls, a full panel and a phone remote.

### Safety
- Screen-changing commands refuse to run on a Hyprland newer than the tested
  versions unless `--force-version` is given (`STANCE_FORCE_VERSION=1` for
  the daemon); reading, keep and revert always work
- Every `--json` reply carries `"schema": 1` (see docs/protocol.md)
- External tools run by absolute path with a hard timeout (kill + reap)

### Displays
- Screens remember their place: turning one off and on, opening the lid or
  re-plugging it (daemon, no matching rule) puts it back in its last
  position, mode and scale (`memory.json`; identical screens without a
  serial are skipped)
- `arrange NAME=XxY …`: several positions in one confirmable change;
  overlaps are refused, screens that touch nothing are reported
- `lid close|open` for Hyprland's lid switch: panel off only when another
  screen is on, no confirm timer
- `status` reports whether the daemon runs; `power` shows AC / battery

### Modes and rules
- Modes gain `night_light`, `audio_sink` and `[brightness]` (extras, outside
  the revert snapshot, failures are notes); `mode save --with-quick`
- `mode put NAME --data JSON`, `rules [show|put]` for editors
- Rules gain `on_power = "ac" | "battery"`; the daemon re-checks them when
  the charger is plugged in or out (kernel uevents, no D-Bus); quick
  settings from rules run on their own thread, only when screens changed

### Quick controls
- `night` (hyprsunset socket; keeps your gamma, "off" = 6000 K, starts one
  if needed), `brightness` (backlight + DDC/CI, floors so a screen never
  goes dark), `audio` (pw-dump + wpctl, ambiguous matches are errors),
  `quick` (all in one JSON call)

### Panel
- Redesign: section cards, segmented controls, optional Material Symbols
  icons (`StanceTheme.iconFont`), open animation
- Map: drag screens (edges snap), resolution and focus dot, "identify"
- Settings window (centred, scrolls): sections to show, confirm window,
  compact mode, hotplug rules editor (with power source), phone pairing,
  health from `doctor`; the panel itself scrolls when taller than the screen
- Modes: add from the current setup, full mode editor
- Confirm card: countdown, Keep · Enter / Revert · Esc (Space removed)

### Phone deck
- Android app "Stance Deck" (`android/`, Kotlin + Compose): QR or pasted-code
  pairing with fingerprint check, several PCs, button grid, Keep / Revert
  banner with countdown, reconnect with back-off, device key wrapped by the
  Android Keystore, screenshots blocked while pairing
- Its Noise core matches the Rust test vectors byte for byte and is tested
  live against `deck serve` (also in CI); `tests/e2e-android.sh` drives the
  app on an emulator against a nested Hyprland (keep, revert, timeout)
- Panel: Phone card (QR, devices) and a phone-buttons editor;
  `deck buttons | put | reset`
- `deck serve` (Wi-Fi, or `--loopback` for USB via `adb reverse`), `deck
  pair` (QR, one phone, 120 s, desktop notice on pairing), `deck status`,
  `deck devices` (last seen), `deck unpair`, `deck test` reference client
- Noise IK sessions, XXpsk3 pairing; limits count failures only, so a flood
  never locks out a paired phone; buttons from `deck.toml` run whitelisted
  stance actions only; protocol and test vectors in docs/protocol.md

### Known limits
- The Android app was verified on an emulator and with screenshot tests,
  not yet on a real phone
- `deck serve` listens on all interfaces unless `--loopback`; allow TCP
  47800 from your LAN only (doctor checks ufw)
- The hyprlang `.conf` path (Hyprland 0.55/0.56) is still untested

### Integrations and packaging
- `waybar` status line and contrib/waybar module
- `doctor`: hyprsunset, brightness tools, audio tools, deck server and ufw,
  and the exact install steps for ii / end4-pC shells
- PKGBUILDs for `stance-hypr` and `stance-hypr-git`, user units for the
  daemon and the deck server

## 0.1.0 – 2026-11-10

First release.

- `monitors`, `set`, `layout` (left-of, right-of, above, below, mirror, single)
- Confirm-or-revert with a detached guardian; stacked changes keep the oldest snapshot
- Verification against `monitors -j` after every change (Hyprland's `eval` says "ok" even when nothing happened)
- Modes: monitors, workspace binding, whitelisted Hyprland options, power profile
- `persist` to `stance-monitors.lua` / `.conf`
- Optional `daemon`: hotplug rules with debounce and burst limit, re-applies the active mode after a config reload
- `doctor`
- Quickshell components, standalone shell, ii and end4-pC integrations
- Tested on Hyprland 0.57.1 (Lua); hyprlang `.conf` path for 0.55/0.56 is experimental
