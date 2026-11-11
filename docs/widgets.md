# Widgets (panel board)

The panel is a board of widgets. Each widget wraps one stance feature; the
user picks which ones show, in what order and at which size (Settings ->
Layout, or the first-run setup). Widgets are in-tree only: there is no
third-party widget API.

## Manifest

Declared in `quickshell/stance/StanceWidgets.qml`:

| Field | Meaning |
| --- | --- |
| `id` | stable key stored in `panel.json` (`map`, `screen`, `night`, `brightness`, `audio`, `modes`, `clip`, `deck`) |
| `title` | English text, also the i18n key (`StanceI18n.t`) |
| `icon` | Material Symbols name |
| `sizes` | subset of `"S"`, `"M"`, `"L"`: S = half-width tile, M = full width, one row, L = full card |
| `defaultSize` | size used when the widget is added |
| `feature` | `null`, or `quick` / `clip` / `deck`: the widget is hidden while that `stance feature` is off (Layout shows "Off in Features") |

`StanceBoard.qml` maps each id to its component. Presets: `minimal`,
`standard` (the 0.4 panel) and `everything`.

## Board in panel.json (schema 2)

```json
{ "schema": 2, "onboarded": true,
  "board": [ { "id": "map", "size": "L" }, { "id": "modes", "size": "S" } ],
  "known": ["map", "screen", "night", "brightness", "audio", "modes", "clip", "deck"] }
```

- `board` is an ordered list; an id that is absent is hidden. Unknown ids,
  duplicates (first wins) and sizes a widget does not have (-> its default)
  are cleaned on read.
- `known` lists the widget ids this file has seen. A widget added in a later
  version that is not in `known` joins the end of the board once; if the user
  removes it afterwards it stays out because `known` now has it.

## First run and migration

- `panel.json` missing: first run, the setup screen shows; finishing it writes
  the file with `onboarded: true`.
- Present but unreadable (bad JSON, wrong type): the defaults are used for this
  session, it is not treated as a first run, and the file is never overwritten
  until the user changes a setting.
- Schema 1 (`show.map|screen|modes|night|brightness|audio: false` switches):
  the standard board minus the switched-off parts; setup is skipped. Reading
  never rewrites; the file becomes schema 2 on the next save.
- Reset: "Reset layout" restores the standard board; "Run setup again" in
  General sets `onboarded` to false.
