# Changelog

## 0.2.0 – unreleased

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
- Settings page: sections to show, confirm window, compact mode, hotplug
  rules editor (with power source), phone pairing, health from `doctor`
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
