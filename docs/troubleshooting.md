# Troubleshooting

Run `stance doctor` first. It only reads; each line is `ok`, `info`, `warn` or
`fail`. For a bug report use `stance doctor --report` (versions, features and
checks in one block, home path hidden). Below, one entry per message: the
symptom, why it happens and the fix. Messages are matched by their start.

## fail

### `HYPRLAND_INSTANCE_SIGNATURE is not set; is Hyprland running?`
- **Symptom:** every command fails; `doctor` stops after the first line.
- **Cause:** the shell you run it in does not belong to a Hyprland session
  (a TTY, SSH, a systemd unit or cron started without Hyprland's environment).
- **Fix:** run it from a terminal inside Hyprland. For a systemd user service
  import the variable at startup (Lua config: `hl.exec_cmd("dbus-update-activation-environment --systemd HYPRLAND_INSTANCE_SIGNATURE WAYLAND_DISPLAY")`;
  uwsm does this itself). Over SSH:
  `export HYPRLAND_INSTANCE_SIGNATURE=$(ls -t $XDG_RUNTIME_DIR/hypr | head -1)`.

### `socket1` / `socket2` failed (not owned by you, wrong permissions, missing)
- **Symptom:** `fail socket1 …` or `socket2 …`.
- **Cause:** the instance directory under `$XDG_RUNTIME_DIR/hypr/` is stale
  (Hyprland crashed and the signature is old), or not private to your user.
  stance refuses sockets it cannot trust.
- **Fix:** open a new terminal in the running session so the variable is fresh;
  if a directory under `$XDG_RUNTIME_DIR/hypr/` belongs to an instance that is
  no longer running (compare with `hyprctl instances`), you may remove that one
  only — never the one of your running session; check that
  `$XDG_RUNTIME_DIR` is `0700` and yours.

### `cannot talk to Hyprland: …`
- **Symptom:** `fail version`.
- **Cause:** the socket exists but Hyprland does not answer (hung, or the
  signature points to an old instance).
- **Fix:** same as above; if Hyprland really hangs, switch to a TTY and restart it.

### `parser` / `monitors` fail
- **Symptom:** `fail parser …` or `fail monitors …` with a Hyprland error text.
- **Cause:** Hyprland replied with an error, usually a version whose IPC changed.
- **Fix:** check the version line; on an untested Hyprland update stance. Reading
  works with `--force-version` while changing screens may not.

### `runtime_dir` fail
- **Symptom:** `fail runtime_dir /run/user/1000`.
- **Cause:** `$XDG_RUNTIME_DIR` is missing, not yours or group/world accessible.
- **Fix:** log in through a normal session (logind creates it with the right
  mode). Do not point it at `/tmp`.

### `stance_runtime` fail
- **Symptom:** `fail stance_runtime …/stance`.
- **Cause:** `$XDG_RUNTIME_DIR/stance` is not a private directory owned by you.
- **Fix:** delete it (`rm -r -- "${XDG_RUNTIME_DIR:?}/stance"`); stance recreates it with `0700`.

## warn

### `Hyprland X (untested version, please report issues)`
- **Symptom:** `warn version`; screen-changing commands refuse to run.
- **Cause:** Hyprland is newer than 0.55 – 0.57, the tested versions.
- **Fix:** update stance. To try anyway: `--force-version` (`STANCE_FORCE_VERSION=1`
  for the daemon and services). Reading, `confirm` and `revert` always work.

### `cannot read ~/.config/hypr/hyprland.lua` (or `.conf`)
- **Symptom:** `warn config`.
- **Cause:** your main config is somewhere else (`hyprland -c`), or unreadable.
- **Fix:** harmless unless you use `stance persist`; then keep the config in
  `~/.config/hypr` or load `stance-monitors` from wherever it is.

### `stance-monitors.lua exists but is not loaded; add require("stance-monitors") …`
- **Symptom:** `warn persist`; layouts do not survive a Hyprland restart.
- **Cause:** `stance persist` wrote the file but your config never loads it.
- **Fix:** add the printed line once to your main config
  (`require("stance-monitors")`, or `source = ~/.config/hypr/stance-monitors.conf`
  in legacy configs).

### `nwg-displays config loads after stance and will override persisted settings`
- **Symptom:** `warn nwg_displays`; after a restart your old layout is back.
- **Cause:** `monitors.lua` / `monitors.conf` from nwg-displays is loaded after
  stance's file and wins.
- **Fix:** move the stance line below nwg-displays' `require("monitors")`.

### `runtime_dir` warn
- **Symptom:** `warn runtime_dir`.
- **Cause:** `$XDG_RUNTIME_DIR` is yours but group or world accessible.
- **Fix:** `chmod 700 $XDG_RUNTIME_DIR`; find out what loosened it.

### `a damaged clipboard index was set aside with its content in …`
- **Symptom:** `warn clip`; the shelf starts empty.
- **Cause:** `index.json` could not be read (disk full, edited by hand, crash
  at the wrong moment). stance moved it, with its blobs, to `corrupt-<time>/`
  instead of deleting anything.
- **Fix:** look inside if you need something, then delete that folder; the
  warning goes away.

### `clip config: …`
- **Symptom:** `warn clip` with a parse or permission error.
- **Cause:** `clip.json` is unreadable or has been edited incorrectly.
- **Fix:** `stance clip config put --data '{"enabled":true}'` rewrites it.

### `clipboard shelf on but text/image capture is not running: …`
- **Symptom:** `warn clip`; copies never appear on the shelf.
- **Cause:** the capture units (`stance-clip-text`, `stance-clip-image`) are not
  active, or they start without `WAYLAND_DISPLAY` and `wl-paste` exits.
- **Fix:** `systemctl --user enable --now stance-clip-text stance-clip-image`;
  `systemctl --user status stance-clip-text` shows the error. If the cause is the
  environment, add `dbus-update-activation-environment --systemd WAYLAND_DISPLAY`
  at Hyprland start. Missing `wl-clipboard` gives the same symptom.

### `deck server on port N, M paired phone(s); ufw is enabled, allow your LAN: …`
- **Symptom:** `warn deck` (`info` if ufw's rules are unreadable); the phone
  cannot connect although the server runs.
- **Cause:** ufw drops incoming TCP on the deck port.
- **Fix:** `sudo ufw allow from 192.168.1.0/24 to any port 47800 proto tcp` — use
  your own LAN range (`ip -4 addr`), as narrow as possible
  (use your LAN range and the port shown). Or use USB:
  `stance deck serve --loopback` + `adb reverse tcp:47800 tcp:47800`.

## info

### `X reports no serial; matching uses description/make+model`
- **Cause:** the monitor's EDID has no serial.
- **Fix:** nothing. If two identical screens are swapped, give modes and rules
  the connector name (`DP-1`) instead.

### `not set up yet; stance persist will write … and you add require(...) once`
- **Fix:** only needed if you want layouts to survive a Hyprland restart: run
  `stance persist`, add the printed line once.

### `nwg-displays config … found; load stance after it so persisted settings win`
- **Fix:** put the stance line after nwg-displays' in your main config.

### `powerprofilesctl not found; modes cannot switch power profile`
- **Fix:** install `power-profiles-daemon` (or ignore; `power_profile` in modes
  is then skipped with a note).

### `hyprsunset not found; night light unavailable`
- **Fix:** install `hyprsunset`. The night-light controls stay hidden meanwhile.

### `pw-dump / wpctl not found; audio output switching unavailable`
- **Fix:** install PipeWire and WirePlumber (`pipewire`, `wireplumber`).

### `brightnessctl / ddcutil not found; brightness unavailable`
- **Fix:** laptop panel: `brightnessctl`. External screens: `ddcutil`. Only
  `brightnessctl found; install ddcutil …` means just the other kind is missing.

### `no /dev/i2c-* devices: load the i2c-dev module for DDC/CI brightness`
- **Symptom:** external screen brightness does nothing.
- **Cause:** the `i2c-dev` kernel module is not loaded, or you cannot access
  `/dev/i2c-*`.
- **Fix:** `sudo modprobe i2c-dev`; to keep it, put `i2c-dev` in
  `/etc/modules-load.d/i2c.conf`; add yourself to the `i2c` group and log in
  again (that group can talk to every I²C device, so only on a single-user
  machine you trust). Some monitors need DDC/CI switched on in their own menu.

### `phone deck server not running (optional: stance deck serve, …)`
- **Fix:** `systemctl --user enable --now stance-deck.service`, or ignore if you
  do not use a phone. `stance feature off deck` hides it from the panel.

### `clipboard shelf off (optional: …)` / `clipboard shelf turned off`
- **Fix:** `stance feature on clip` (or the Clipboard page in panel settings) and
  `systemctl --user enable --now stance-clip-text stance-clip-image`.

### `clipboard shelf paused (stance clip resume)`
- **Cause:** you paused capture (`stance clip pause 1h`); the deadline is kept on disk.
- **Fix:** `stance clip resume`.

### `<shell> (end4-pC|ii) has no stance button yet: …`
- **Fix:** run the printed commands (two symlinks and one line in
  `UtilButtons.qml`); see [quickshell.md](quickshell.md). Or ignore and use the
  standalone panel: `stance setup`.

### `… also runs and stores what stance skips (password-manager copies)`
- **Cause:** cliphist runs next to stance. stance never stores copies that a
  password manager marks sensitive, cliphist does.
- **Fix:** if that matters, stop cliphist (`wl-paste --watch cliphist store`
  in your autostart) and rely on stance's shelf.

## Panel problems that doctor does not show

- **The panel does not open / nothing happens on click.** Is the shell running?
  `qs -p /usr/share/stance/quickshell` (standalone) or your bar's own shell
  (`qs -c <name> log`). A QML error leaves the old version running
  (`Failed to load configuration`).
- **A feature section is missing in the panel or settings.** You turned the
  feature off: `stance feature list`, `stance feature on NAME`.
- **The shelf says "turned off".** Same: `stance feature on clip`.
- **Settings did not change after editing `panel.json` by hand.** The panel
  writes that file itself; edit it while the shell is stopped, or use the
  settings window.
