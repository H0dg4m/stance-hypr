# Quickshell integration

`quickshell/stance/` holds plain QML components with no dependency on any
particular shell:

| Component | What it is |
|---|---|
| `StanceService` | Runs `stance … --json`, watches the pending change (inotify, no polling) |
| `StancePanel` | Layout map + settings for the selected screen + quick controls + modes |
| `StanceQuick` | Night light, brightness sliders, audio output (part of the panel; hides what is unavailable) |
| `StanceSlider` | 0–100 slider without QtQuick.Controls; commits on release (DDC/CI writes are slow) |
| `StancePopup` | The panel in a layer-shell popup, under a bar button or top-right; owns the settings window (`settingsOpen`) |
| `StanceSettingsWindow` | Panel settings in their own centred, scrolling window (opened from the panel's settings chip) |
| `ConfirmOverlay` | "Keep these display settings?" card; create one per screen |
| `StanceTheme` | Colours / metrics (`docs/themes.md`). Resolved as overrides > named theme (`apply()`) > `shell` > default. An embedding shell passes its own look through `shell: ({ accent: ..., radius: ... })` and sets `StanceService { themeTarget: theTheme; defaultTheme: "shell" }`; the panel's `theme` / `themeOverrides` / `lang` prefs then pick named themes. `backgroundSolid` / `cardSolid` are for overlays (never translucent). Set `iconFont` to a Material Symbols family ("Material Symbols Rounded") for icons; empty = text only |
| `StanceI18n` | Singleton: `StanceI18n.t("English text")` / `f("... %1", x)`; English and Turkish, `lang` auto/en/tr from the panel prefs |
| `StanceAppearance` | Settings cards for language, theme picker + live editor (save / import / export) and the feature switches (`stance feature`) |
| `StanceSection`, `StanceSegmented`, `StanceIcon`, `StanceSwitch`, `StanceField` | Building blocks: titled card, segmented buttons, icon glyph, switch, text field |
| `StanceModes`, `StanceModeEditor`, `StanceSettings` | Modes card (add / edit), mode editor page, panel settings (prefs in `~/.config/stance/panel.json`) |
| `StanceDeckButtons` | Phone-buttons editor card in the settings window (`deck buttons / put / reset`) |
| `StanceIdentify` | Shows each screen's name on that screen for a moment |
| `StanceClip`, `StanceClipService` | Clipboard shelf overlay (`stance clip`, see `docs/clip.md`) and its data side (list on open, `clip.rev` watch while open) |
| `shell.qml` | Standalone shell with IPC (`toggle`, `open`, `close`, `settings`, `confirm`, `revert`, `clip`, `clipOpen`, `clipClose`) |

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

## Clipboard shelf

Needs `stance clip` (capture units running, `clip.enabled` set in the panel
settings under "Clipboard shelf"). The standalone shell owns the overlay:

```sh
qs -p /usr/share/stance/quickshell ipc call stance clip      # toggle
qs -p /usr/share/stance/quickshell ipc call stance clipOpen
qs -p /usr/share/stance/quickshell ipc call stance clipClose
```

Hyprland (Lua config):

```lua
hl.bind("SUPER + SHIFT + V", hl.dsp.exec_cmd("qs -p /usr/share/stance/quickshell ipc call stance clip"))
```

Keys: arrows move, Up/Down/Tab switch shelf, Space previews, Enter copies and
closes, P pins, S/Delete removes, Backspace undoes (or edits the search), `/`
or any other key searches, Esc clears the search, then closes. In search mode
letters type; use Ctrl+P / Ctrl+S to pin / remove then. The layer takes
exclusive keyboard focus only while it is open.

Embedding: create a `StanceClipService { bin: ... }` and a
`StanceClip { clip: thatService; screen: ...; onSettingsRequested: ... }`, then
set `open`. To try it without the real core:

```sh
STANCE_BIN=$PWD/contrib/fake-clip.sh STANCE_CLIP_DIR=/tmp/stance-clip-fixture qs -p quickshell/stance
```

(`contrib/fake-stance.sh` forwards to `target/release/stance` but handles `feature on|off` without touching systemd units; use it with an isolated `XDG_CONFIG_HOME` when testing the settings.)

(`STANCE_CLIP_DIR` is the directory image paths must live under; the default is
`$XDG_DATA_HOME/stance/clip`. `STANCE_FAKE_OFF=1` shows the disabled state,
`STANCE_FAKE_LOG=file` records the calls.)

The confirm card appears on every screen for any change, including ones made
from the CLI or a keybind.
