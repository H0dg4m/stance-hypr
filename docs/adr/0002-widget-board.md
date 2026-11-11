# ADR 0002 — Widget board, first-run setup, layout (0.5.0)

Status: accepted, revised after review · 2026-11-11 (see "Revision" at the end)

## Context
The panel is one fixed column: map, selected screen, quick card (night,
brightness, audio), modes, messages. `panel.json` `show.*` can hide six of
those, nothing else. The maintainer wants the panel to open as a **widget
board** on first run, where the user keeps each feature at the size they
want, and wants order/visibility/button placement in the settings.
Two QML files are past 1000/600 lines and there is no QML test outside the
nested-Hyprland run. The panel still refreshes by spawning the CLI and
watching `pending.json` itself, although `stance events` exists (0.4).

Not in scope (still): a plugin system, third-party widgets, a general
widget set (clock, weather, workspaces...). Every widget wraps a stance
feature that already exists.

## Decisions

### 1. Widget contract (in-tree only)
`quickshell/stance/StanceWidgets.js` (`.pragma library`) exports
`schema: 1` and a list of manifests:

| field | meaning |
| --- | --- |
| `id` | `[a-z][a-z0-9_]*`, stable, stored in panel.json |
| `title`, `icon` | i18n key, Material Symbols name |
| `sizes` | subset of `S` (half width tile), `M` (full width, one row), `L` (full card) |
| `defaultSize` | one of `sizes` |
| `source` | data it reads: `monitors`, `quick`, `modes`, `clip`, `deck` |
| `feature` | gate from `stance feature` (`quick`, `clip`, `deck`) or null |
| `caps` | backend capabilities it needs (`stance caps`), e.g. `displays` |
| `actions` | the only `stance` sub-commands it may run (`["night"]`) |
| `component` | QML file name |

A widget is a `StanceWidget` (base item) with `size`, `theme`, `host`.
`host` is a `StanceWidgetHost` that runs commands for it: `host.run(args,
done)` refuses (and logs) any `args[0]` not in the manifest's `actions`.
That is a guard against mistakes in our own code, not a sandbox; there is
no way to load a widget from outside the install directory.

First set (sizes):

| id | S | M | L |
| --- | --- | --- | --- |
| `map` | – | short map | map + identify (today) |
| `screen` | – | on/off + refresh chips | full screen card (today) |
| `modes` | active mode, tap = next | mode chips | card with add/edit (today) |
| `night` | toggle tile | temperature chips | – |
| `brightness` | focused screen %, wheel ±5 | sliders | – |
| `audio` | current sink, tap = next | sink chips | – |
| `clip` | open shelf + count, paused state | last 3 items | – |
| `deck` | server state + paired count, tap = phone settings | – | – |

A widget whose feature is off or whose caps are missing renders nothing
(the board skips it, it keeps its place in the layout).

### 2. Board layout and panel.json schema 2
`panel.json` gets `board: [{id, size}]` (order = list order, a missing id
= hidden) and `onboarded: bool`; `show` is removed. The board flows S
tiles two per row and M/L full width; width stays 380 (`wide: true` → 560,
three S per row).

Migration 1 → 2 (in `normPrefs`, written back with `schema: 2` on the next
save): `map L, screen L, night M, brightness M, audio M, modes L` with every
`show.X === false` dropped; `onboarded: true` (existing users are not sent
through setup). Unknown ids and sizes not in the manifest are dropped,
duplicates keep the first. An empty board is allowed (the panel shows an
"Add widgets" button). A `schema > 2` file is read but never rewritten
(as today).

### 3. First run
If `panel.json` does **not exist** (not: unreadable or invalid), the panel
opens on a setup page instead of the board: three presets (Minimal: map M,
modes S, night S, audio S · Standard: the migrated default · Everything:
all at default size), a list to toggle each widget and pick its size, a
live preview of the board, and Finish (writes `onboarded: true`). Closing
the panel without Finish writes nothing; it comes back next time.
Settings → General → "Run setup again".

### 4. Layout settings
New settings category `layout` (between General and Appearance): the board
as rows with up/down, size segmented control, remove; "Add widget" lists
hidden ones; "Reset layout". All keyboard reachable (Tab, Space, arrows).
The six `show.*` switches in General go away (they live here now).
Button placement: `popupAnchor` = `button` (default: under the bar button,
or top-right without one) | `left` | `center` | `right` of the focused
screen. Where the bar button itself sits is the bar's business:
`stance setup end4|waybar` prints the lines for the chosen side
(`--position left|right`), still without editing files.

### 5. Panel on `stance events`
`StanceService` runs one `stance events` child while the panel or the
board is visible (and always for the confirm card's `pending`), and
re-reads by event: `monitors` → refresh, `pending` → pending file, `active`
→ status, `panel` → prefs file, `features`, `theme`, `modes`. It restarts
it with backoff when it exits; if `hello` never comes (old binary) it falls
back to today's FileView watch.

### 6. QML split and smoke test
Flat files (no subdirectory: the end4 integration imports the folder as
one module): `StanceWidget*.qml` per widget, `StanceSettings<Page>.qml` per
category, `StanceBoard.qml`, `StanceSetup.qml`. StancePanel keeps the
header, board/page switch and messages. No file over ~400 lines.

`tests/qml-smoke.sh`: `qs -p tests/qml/smoke.qml` with
`QT_QPA_PLATFORM=offscreen`, `STANCE_BACKEND=fake`, an isolated
`XDG_CONFIG_HOME/XDG_STATE_HOME` in a temp dir (never the real
`~/.config/stance`). It builds the board with every widget at every size,
each settings page, the setup page, schema-1 and schema-2 panel.json,
and fails on any QML warning/error line (`TypeError`, `ReferenceError`,
`Unable to assign`, `is not defined`, `Cannot read property`); it saves PNGs
for review. CI runs it in an `archlinux` container with `quickshell`.

## Revision (after review, before code)
- The manifest is a QML singleton (`StanceWidgets.qml`), not a `.pragma
  library` file: titles are English keys the caller passes to
  `StanceI18n.t`, and `StanceBoard` maps `id.size` to a `Component` by type
  name (no `Loader { source: "file" }`, which breaks under the end4 module
  import). Manifest fields for 0.5: `id, title, icon, sizes, defaultSize,
  feature`. `source`, `caps`, `actions` and `host.run` are dropped: widgets
  get the `StanceService` like the cards did, and an allow-list over our own
  code would only be decoration.
- First run is decided in `FileView.onLoadFailed` (`FileNotFound`), with
  `prefsState` = loading / missing / present so the board does not flash.
  An unreadable file gives defaults and is never written (fail closed).
- The setup card sits on top of the board inside the panel; the board under
  it is the live preview. Closing the panel loses nothing (setup stays until
  Done). "Customize" opens Settings → Layout.
- `known: [ids]` in panel.json: a widget added in a later release joins the
  end of the board once; removing it later keeps it out.
- Board availability (no night-light tool, one audio output...) is decided
  by the board, not by the widget's `visible` (an invisible slot would make
  its item invisible too and never come back).
- Deferred: `popupAnchor` and `stance setup --position` (inside end4/ii the
  popup is anchored to the bar button, so only the standalone shell could
  honour it), `wide` boards, and the panel's move to `stance events`
  (several `StanceService` instances exist per bar; needs a self-write guard
  for panel.json first). Splitting files is limited to what 0.5 touches.
- Clipboard tile: the standalone shell opens its shelf; in end4/ii the popup
  calls `qs -p <shellDir> ipc call stanceclip open`.
