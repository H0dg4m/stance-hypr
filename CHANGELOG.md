# Changelog

## 0.2.0 – unreleased

Quick controls.

- `night`: night light through hyprsunset's socket; uses a running hyprsunset
  (keeps its gamma, "off" = neutral 6000 K) or starts one; remembers the last
  warm temperature for `on` / `toggle`
- `brightness`: laptop backlight (sysfs + brightnessctl) and external screens
  over DDC/CI (ddcutil, connector → bus from ddcutil's DRM line, cached);
  relative `+N` / `-N`, floors of 5 % / 1 % so a screen never goes dark
- `audio`: list outputs from `pw-dump`, switch with `wpctl set-default`;
  ambiguous matches are errors
- `quick`: everything above in one JSON call for bars
- Modes: `night_light`, `audio_sink`, `[brightness]` (extras, outside the
  revert snapshot, failures are notes); `mode save --with-quick` also records
  night light and audio output
- Daemon: hotplug rules with `allow_extras` apply them on a separate thread,
  only when the rule changed the screens
- Panel redesign: section cards, segmented refresh / scale / night light,
  optional Material Symbols icons (`StanceTheme.iconFont`), screen map with
  resolution and focus dot, audio outputs as a list with short names
  (`short`, `bus`, `profile` in `audio list --json`), open animation
- Confirm card: countdown readout and bar, Keep · Enter / Revert · Esc
  buttons (Space no longer confirms)
- Screens remember where they were: turning one off and on (or re-plugging
  it, with the daemon running and no rule matching) puts it back in its last
  position, mode and scale (`memory.json` in the state dir; identical
  screens without serial are skipped)
- `arrange NAME=XxY …`: move several screens in one confirmable change;
  overlaps are refused, screens that touch nothing are reported
- Panel: drag screens on the map (edges snap), "identify" shows each
  screen's name on it, settings page (sections to show, confirm window,
  compact mode, hotplug rules editor, health from `doctor`), add-mode form
  and a mode editor (`mode put NAME --data JSON`, `rules [show|put]`)
- `status` reports whether the daemon is running
- `doctor` checks hyprsunset, ddcutil / brightnessctl, pw-dump / wpctl
- External tools run with a hard timeout (kill + reap)

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
