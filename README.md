# stance

Bar-first display settings and **modes** for Hyprland. Change refresh rate,
resolution, scale, position or layout in two clicks, switch between your own
modes (gaming, work, single screen…), and never get stuck on a black screen:
every change reverts on its own unless you confirm it.

[Türkçe](README.tr.md)

<p align="center">
  <img src="docs/panel.png" alt="stance panel" width="330">
  &nbsp;
  <img src="docs/confirm.png" alt="confirm or revert" width="330">
</p>

## Why

Most display tools are "open an app, change, close" or background profile
switchers. stance is built for the bar:

- **Two clicks from the bar** – a small popup with a live layout map.
- **Confirm or revert** – a 12 s countdown on every screen; do nothing and the
  previous setup comes back. Works even if the bar itself dies (a detached
  guardian process owns the timer).
- **Modes beyond monitors** – a mode can also bind workspaces to screens,
  toggle animations / blur / tearing, set gaps and switch the power profile.
  Strictly whitelisted: mode files cannot run commands.
- **Hotplug rules** (optional daemon) – plug in the dock, get your desk mode.
- **Zero idle cost** – the CLI runs per click and exits (~1 ms, < 1 MB
  binary); the optional daemon blocks on Hyprland's event socket (~3 MB RSS).

## Install

Arch (AUR): `stance-hypr-git` (see `packaging/aur`). From source:

```sh
cargo build --release
install -Dm755 target/release/stance ~/.local/bin/stance
stance doctor
```

Requires Hyprland **0.55+** with a Lua config (0.57 is tested; 0.57 removed
hyprlang `.conf` support). Legacy `.conf` support for 0.55/0.56 exists but is
experimental.

## Bar integration

- **Quickshell** (end-4 / illogical-impulse and forks, or your own shell):
  drop-in components – see [docs/quickshell.md](docs/quickshell.md).
- **Any other bar** (Waybar, …): run the bundled standalone shell once,
  then bind a click to `qs -p /usr/share/stance/quickshell ipc call stance toggle`.
- **No bar**: everything works from the CLI.

## CLI

```
stance monitors                       list monitors
stance set DP-1 --hz 165              change one monitor
stance set eDP-1 --off                (refuses to turn off the last screen)
stance layout right-of eDP-1 DP-1     left-of | right-of | above | below | mirror | single
stance confirm | stance revert        keep / undo the pending change
stance persist                        write ~/.config/hypr/stance-monitors.lua
stance mode save work                 snapshot the current setup as a mode
stance mode apply gaming
stance daemon                         hotplug rules (systemd unit in contrib/)
stance doctor                         read-only environment check
```

`<monitor>` can be the description (`"JDR EPIC Q40"`), `"make model"` or the
connector. Changes revert after 12 s unless confirmed (`--timeout S`,
`--no-confirm` for scripts). Every command speaks JSON with `--json`.

### Modes

`~/.config/stance/modes/<name>.toml`:

```toml
description = "External only, effects off"
power_profile = "performance"           # power-saver | balanced | performance
workspaces = { "1-5" = "JDR EPIC Q40" }

[hyprland]                              # whitelist: animations, blur, allow_tearing, gaps_in, gaps_out
animations = false
blur = false
allow_tearing = true

[[monitor]]
match = "JDR EPIC Q40"
mode = "2560x1440@180"
position = "0x0"
vrr = 1

[[monitor]]
match = "AU Optronics 0x7EAD"
enabled = false
```

Monitors a mode names but that are not connected are skipped, so one mode
works docked and undocked. More in [`examples/`](examples).

### Hotplug rules

`~/.config/stance/rules.toml` – a rule fires when the connected set equals its
list exactly; nothing matches → nothing happens.

```toml
[[rule]]
monitors = ["AU Optronics 0x7EAD", "JDR EPIC Q40"]
mode = "desk"
allow_extras = false   # true: also apply the mode's Hyprland options / power profile
```

Enable with `systemctl --user enable --now stance.service`.

### Persisting

Runtime changes are temporary on purpose (a projector should not stick). Run
`stance persist` to write `stance-monitors.lua` next to your config and add
`require("stance-monitors")` once – after nwg-displays' `require("monitors")`
if you use it. stance never edits your main config.

## Security

- Talks to Hyprland's socket directly; no `hyprctl`, no shell, ever.
- Lua is generated only from typed, validated values. EDID strings never
  reach the compositor; they are only displayed (control characters removed).
- Mode and rule files: 64 KiB max, unknown fields rejected, must be owned by
  you and not group/world writable, no command field exists.
- State and config writes are atomic (`O_EXCL|O_NOFOLLOW` temp file, fsync,
  rename) and refuse symlink targets. State lives in a 0700 directory.
- Pending changes are bound to the Hyprland instance; a snapshot from an old
  session is never applied to a new one. No root.

## Development

```sh
cargo test                    # unit tests
tests/nested.sh lua           # end-to-end in a throwaway nested Hyprland
```

## License

MIT
