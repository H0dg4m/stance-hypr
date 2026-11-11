# Themes, language and features — contract (0.3.0)

## Theme file (`stance-theme` v1)
A theme is one JSON file; the same format is used for built-ins, user themes,
export and import.
```json
{
  "stance_theme": 1,
  "id": "catppuccin-mocha",
  "name": "Catppuccin Mocha",
  "dark": true,
  "colors": {
    "background": "#1e1e2e", "card": "#181825", "surface": "#313244",
    "surfaceHover": "#45475a", "border": "#45475a", "text": "#cdd6f4",
    "textDim": "#a6adc8", "accent": "#89b4fa", "accentText": "#11111b",
    "danger": "#f38ba8", "dangerText": "#11111b", "scrim": "#99000000"
  },
  "metrics": {
    "radius": 12, "chipRadius": 8, "fontFamily": "", "fontSize": 13,
    "iconFont": "", "animMs": 120, "padding": 12, "spacing": 8, "opacity": 1.0
  }
}
```
Rules (enforced by `stance theme import|put`):
- `id`: `[a-z0-9-]{1,40}`; built-in ids cannot be overwritten (import under a new id);
  `shell`, `auto` and `default` are reserved for the panel.
- `name`: 1–60 chars, no control characters.
- Colours: `#rrggbb` or `#aarrggbb` only (Qt style: alpha first). All 12 colour keys required (`scrim` = dimmed
  backdrop behind overlays).
- Metrics (all optional, defaults above): radius 0–40, chipRadius 0–30, fontSize 8–32,
  animMs 0–1000, padding 0–40, spacing 0–32, opacity 0.5–1.0 (applies to background
  and card only; text stays opaque), fontFamily/iconFont ≤ 64 chars, empty = the
  shell's font when embedded, else the default.
- Unknown keys are refused. File ≤ 64 KiB.
- `opacity` < 1 must never make overlays (clip preview, confirm card, settings window)
  show what is underneath through an opaque-looking panel: overlays always draw on an
  opaque `background` and hide what they cover.

Built-in themes are compiled into the binary (`themes/*.json` in the repo):
stance-dark, stance-light, catppuccin-mocha, catppuccin-macchiato, catppuccin-frappe,
catppuccin-latte, nord, gruvbox-dark, gruvbox-light, dracula, tokyo-night, rose-pine,
rose-pine-dawn, everforest-dark, kanagawa, one-dark, solarized-dark.
Palette credits and licences: [NOTICE](../NOTICE).
User themes live in `~/.config/stance/themes/<id>.json` (0600).

## CLI
Paths are taken literally (`~` is not expanded; the panel expands it).
Errors use the usual envelope; `exists` (with "--force" in the message) means an
overwrite needs `--force`. `put`/`import` return the saved theme in `data`,
`export --out` returns `data.path`, `rm` returns `data.removed`.
```
stance theme list [--json]          # built-in + user, with full data
stance theme show ID [--json]
stance theme put --data JSON [--force]    # save a user theme (editor "Save as")
stance theme check [ID|FILE]       # WCAG contrast of text/background, text/card, textDim/card,
                                    # accentText/accent, dangerText/danger; exit 1 if any is under
                                    # 4.5:1 (3:1 for textDim). `show --json` has the same as `contrast`.
stance theme import FILE [--force]  # validate + copy into the user dir
stance theme export ID [--out FILE] # print (or write, 0644, refuses to overwrite without --force)
stance theme rm ID                  # user themes only
```
`theme list --json` → `{"ok":true,"schema":1,"data":{"themes":[{"builtin":true,
"theme":{…full theme…}}, …]}}`. Sorted: built-ins in the order above, then user themes by name.

## Converting pywal / matugen palettes
`contrib/stance-theme-from-pywal` (python3, no dependencies) turns a pywal
`colors.json` (default `~/.cache/wal/colors.json`) or a matugen JSON
(`--matugen FILE [--scheme dark|light]`) into a theme and prints it; `--import` saves
it through `stance theme import`. `--id`/`--name` set the identity. Palettes failing
the contrast check are refused unless `--force`.
```
contrib/stance-theme-from-pywal --id wal --import
contrib/stance-theme-from-pywal --matugen ~/.cache/matugen/colors.json --scheme light
```
Test: `tests/theme-convert.sh [stance]`.

## Panel preferences (`~/.config/stance/panel.json`, written by the panel)
New keys:
- `"theme": "<id>" | "shell"` — `shell` = keep the colours the embedding shell passes
  (end4 integration default). Standalone default: `stance-dark`.
  `"auto"` follows the system's light/dark preference (see below).
- `"themeLight"` / `"themeDark"` — the pair `"auto"` uses; defaults `stance-light` and
  `stance-dark`. The settings page lists light themes under the first and dark ones
  under the second.
- `"schema": 1` — file version; a `panel.json` with a newer schema is read but the
  panel does not write it.
- `"themeOverrides": {colour or metric key: value}` — per-key tweaks on top of the
  selected theme (the "detailed customisation"); same validation as the file format.
- `"lang": "auto" | "en" | "tr"` — `auto` follows `LC_ALL`/`LC_MESSAGES`/`LANG`.

### Automatic light/dark
```
stance theme system [--json]    # dark | light | unknown
```
Asks the XDG settings portal (`org.freedesktop.appearance color-scheme`: 1 dark,
2 light, 0 no preference), then GNOME's `gsettings` (`prefer-dark` dark,
`prefer-light`/`default` light). `--json` → `{"ok":true,"schema":1,"data":{"scheme":"dark"}}`.
In `"auto"` the panel asks on start, whenever it opens and when Qt's colour scheme
changes; there is no background watcher. Qt's own value is only the fallback when
stance cannot answer, because without a Qt platform theme it reports Light on a dark
desktop. Until the first answer the default theme stays.

## Features
```
stance feature list [--json]
stance feature on|off NAME
```
Names: `clip` (clipboard shelf), `deck` (phone deck), `rules` (hotplug daemon),
`quick` (night light / brightness / audio in the panel).
`~/.config/stance/features.json` holds `{"clip":true,…}`; missing = on.
Side effects (user units only, never sudo):
- `clip off` → `clip.enabled=false`, `systemctl --user disable --now stance-clip-text stance-clip-image`
  (if installed); `clip on` → enabled=true and `enable --now`. Stored items are kept.
- `deck off|on` → `stance-deck.service`; `rules off|on` → `stance.service`.
- `quick` has no side effect (panel only).
The choice is saved before the units change; if a unit cannot be changed the
reply's `notes` name it and the choice stays saved (not atomic across both).
`feature list --json` → `{"data":{"features":[{"name":"clip","on":true,
"unit":"stance-clip-text.service"|null,"unit_active":true|null}]}}`.
The panel hides the sections of features that are off; the clip shelf IPC shows a
short "Clipboard shelf is turned off" note instead of the shelf.
