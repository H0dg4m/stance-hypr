# Installing stance

stance is one binary (`stance`) plus QML files for the panel. It needs
Hyprland **0.55+** with a Lua config (0.57 tested). Everything else is
optional and only switches features on (see "Optional tools").

## Arch Linux (AUR)

```sh
yay -S stance-hypr        # tagged releases
yay -S stance-hypr-git    # main branch
```

Files: `/usr/bin/stance`, `/usr/share/stance/` (panel, integrations,
examples, Waybar module), user units in `/usr/lib/systemd/user/`.

## Nix / NixOS

```sh
nix run github:H0dg4m/stance-hypr -- doctor
nix profile install github:H0dg4m/stance-hypr
```

The package carries the user units with store paths in
`lib/systemd/user/` and the panel in `share/stance/quickshell`. On NixOS
add `inputs.stance.packages.${system}.default` to
`environment.systemPackages` (or Home Manager `home.packages`) and enable
the units you want with `systemd.user.services` or by linking them.

## Any distribution: release tarball

Each release has a static (musl) binary:

```sh
curl -LO https://github.com/H0dg4m/stance-hypr/releases/download/vX.Y.Z/stance-X.Y.Z-x86_64-linux.tar.gz
curl -LO https://github.com/H0dg4m/stance-hypr/releases/download/vX.Y.Z/SHA256SUMS
sha256sum -c SHA256SUMS
tar xzf stance-X.Y.Z-x86_64-linux.tar.gz
install -Dm755 stance-X.Y.Z/stance ~/.local/bin/stance
mkdir -p ~/.local/share/stance && cp -r stance-X.Y.Z/quickshell/stance ~/.local/share/stance/quickshell
```

The units in `contrib/` call `/usr/bin/stance`; for a user install copy
them to `~/.config/systemd/user/` and change `ExecStart` to
`%h/.local/bin/stance` (and `/usr/share/stance` to `%h/.local/share/stance`).

## From source

```sh
cargo build --release
install -Dm755 target/release/stance ~/.local/bin/stance
```

## Then

```sh
stance doctor            # what works on this machine, what is missing
stance setup             # prints the steps for your bar (waybar, quickshell, end4), changes nothing
```

`stance setup --dry-run` shows the same without enabling units. Bar
integration details: [quickshell.md](quickshell.md); the phone app:
[deck.md](deck.md).

## Optional tools

| Feature | Needs |
| --- | --- |
| Panel and confirm card | quickshell |
| Night light | hyprsunset |
| Brightness | brightnessctl (laptop panel), ddcutil (external screens, DDC/CI) |
| Audio output | pipewire (pw-dump), wireplumber (wpctl) |
| Power profile in modes | power-profiles-daemon |
| Clipboard shelf, phone clipboard | wl-clipboard |
| Notifications, transfer prompts | libnotify (notify-send) with a notification daemon that supports actions |
| Phone over USB | android-tools (adb reverse) |

## Uninstall

Stop the units (`systemctl --user disable --now stance stance-deck
stance-panel stance-clip-text stance-clip-image`), remove the binary, and
delete `~/.config/stance`, `~/.local/state/stance` and
`~/.local/share/stance` if you want your settings and clipboard history gone.
