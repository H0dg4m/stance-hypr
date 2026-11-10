# Quickshell integration

`quickshell/stance/` holds plain QML components with no dependency on any
particular shell:

| Component | What it is |
|---|---|
| `StanceService` | Runs `stance … --json`, watches the pending change (inotify, no polling) |
| `StancePanel` | Layout map + settings for the selected screen + quick controls + modes |
| `StanceQuick` | Night light, brightness sliders, audio output (part of the panel; hides what is unavailable) |
| `StanceSlider` | 0–100 slider without QtQuick.Controls; commits on release (DDC/CI writes are slow) |
| `StancePopup` | The panel in a layer-shell popup, under a bar button or top-right |
| `ConfirmOverlay` | "Keep these display settings?" card; create one per screen |
| `StanceTheme` | Colours / radii / font; override to match your shell |
| `shell.qml` | Standalone shell with IPC (`toggle`, `open`, `close`, `confirm`, `revert`) |

Packages install them to `/usr/share/stance/quickshell`; from source use
`quickshell/stance` in this repo.

## Your own shell

Make the folder importable as a module (symlink it into your config, e.g.
`~/.config/quickshell/<shell>/modules/stance`), then:

```qml
import qs.modules.stance

Item {
    id: root
    // ... your bar button, calling stancePopup.open = !stancePopup.open

    StanceService { id: stanceService }
    StanceTheme { id: stanceTheme; accent: "#8ab4f8" }

    StancePopup {
        id: stancePopup
        service: stanceService
        theme: stanceTheme
        anchorItem: myButton          // opens under the button
        barHeight: 40
    }

    // In a per-screen bar this gives exactly one card per screen.
    ConfirmOverlay {
        service: stanceService
        theme: stanceTheme
        screen: root.QsWindow.window ? root.QsWindow.window.screen : null
        enabled: root.visible
    }
}
```

## end-4 illogical-impulse (`ii`) and the end4-pC fork

Ready-made buttons that follow the shell's colours and button style live in
`quickshell/integrations/ii/` and `quickshell/integrations/end4-pC/`. With
`CONFIG=~/.config/quickshell/ii` (or `end4-pC`):

```sh
ln -s /usr/share/stance/quickshell "$CONFIG/modules/stance"
ln -s /usr/share/stance/integrations/ii/StanceUtilButton.qml "$CONFIG/modules/ii/bar/"   # or end4-pC/
```

Then add one line to `$CONFIG/modules/ii/bar/UtilButtons.qml`, as the first
child of its `RowLayout` (ii) or `Flow` (end4-pC):

```qml
StanceUtilButton {}                                   // ii
StanceUtilButton { contentColor: root.contentColor }  // end4-pC
```

The shell reloads by itself. To remove: delete the line and the two links.

## Any other bar

Start the standalone shell once (e.g. `exec-once` in Hyprland):

```sh
qs -p /usr/share/stance/quickshell
```

and bind your bar's click / a key to:

```sh
qs -p /usr/share/stance/quickshell ipc call stance toggle
```

Waybar example:

```jsonc
"custom/stance": {
  "format": "󰍹",
  "on-click": "qs -p /usr/share/stance/quickshell ipc call stance toggle",
  "tooltip": false
}
```

The confirm card appears on every screen for any change, including ones made
from the CLI or a keybind.
