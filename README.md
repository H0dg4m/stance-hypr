# stance

Bar-first display settings and **modes** for Hyprland: change resolution, refresh rate,
scale, position, VRR or layout in two clicks, and switch between your own modes
(gaming, work, single screen…) with an automatic *confirm or revert* timer.

[Türkçe](README.tr.md)

> **Status: early development (M0).** Only read-only commands work today.

## Usage

```
stance monitors [--json]   # list monitors
stance doctor [--json]     # check the environment (read-only)
```

Every command supports `--json` (`{"ok": true, "data": …}` / `{"ok": false, "error": {"code", "message"}}`),
which is what bar interfaces consume.

## Design goals

- **Zero idle cost:** the CLI runs per command and exits; the optional daemon (hotplug rules) blocks on events.
- **Safe by default:** talks to Hyprland's socket directly (no `hyprctl`, no shell), generates config only
  from typed values, never runs commands from mode files, never needs root.
- **Lua and legacy configs:** Hyprland 0.55+ Lua configs via `eval`, hyprlang `.conf` as fallback.

## Build

```
cargo build --release
./target/release/stance doctor
```

## License

MIT
