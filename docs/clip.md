# stance clip — design (0.3.0)

A keyboard-first clipboard shelf for Hyprland, inspired by ClipBoardLine
(MIT, github.com/orkunisitmak/clipboardline). Not a fork: same ideas, stance's
core + Quickshell panel. It coexists with cliphist (end4/ii runs
`wl-paste --watch cliphist store`); we add our own watchers and never touch theirs.

## Scope
In 0.3.0:
- Three shelves: **images, texts, links**. A copy is a link only if it is one
  line, has no whitespace and starts with `http://` or `https://` with a host.
- Capture: two systemd user units run `wl-paste --type text --watch stance clip ingest`
  and `wl-paste --type image --watch stance clip ingest --image`. systemd restarts them
  (wl-paste exits when the compositor restarts). Off by default: `clip.enabled`
  must be set (panel settings or `stance clip config put`).
- Only `CLIPBOARD_STATE=data` is stored; `sensitive` (wl-clipboard 2.3 sets it for
  `x-kde-passwordManagerHint`), `nil`, `clear`, missing or unknown are skipped.
- Bounded reads: text ≤ 256 KiB, image ≤ 32 MiB (read max+1, drop if larger).
- Images: type by magic bytes (png, jpeg, webp, gif, bmp); everything else, SVG
  included, is dropped. PNG width/height read from the header.
- File-manager copies (`file://` URIs only) are ignored in 0.3.0.
- Dedupe: same sha256 → one entry, `updated` bumped, pin kept. Copy-back from the
  panel moves the item to the top (no self-copy marker).
- Pin. Pinned items survive retention.
- Retention: one setting, 1/3/7/15/30 days, default 30. Prune on every ingest
  (cheap) and `stance clip prune`.
- Remove = soft delete (`trashed` timestamp); `undo` restores the last removal;
  trashed entries are purged after 1 day by prune. Blobs are mark-and-sweep
  collected under the lock (blob written before index → crash leaves an orphan,
  never a dangling reference).
- Search: preview (first 200 chars), first 4 KiB of text, host.
- Copy back with `wl-copy` (`--type` from the sniffed mime).
- Panel: overlay (layer-shell, keyboard exclusive only while open) with three
  rows. Arrows move, Tab or Up/Down switches shelf, Space previews, Enter copies
  and closes, P pins, S/Delete removes, Backspace undoes, `/` or typing searches,
  Esc closes. IPC `qs ... ipc call stance clip` (toggle).
- Settings card in StanceSettingsWindow: enable, text/image capture, retention.

Later: screenshot folder watch (duplicates the Print → wl-copy path), OCR (only eng
data here), paste into previous window (`sendshortcut`, terminals need Ctrl+Shift+V),
titles/tags/folders, link enrichment (network), visual search, markup/crop.

Caveat in docs: cliphist, if running, still stores what we skip; our store is
0700/0600 but not encrypted.

## Running the capture units
`systemctl --user enable --now stance-clip-text stance-clip-image`. wl-paste
needs `WAYLAND_DISPLAY` in the systemd user environment; uwsm and most
Hyprland setups import it, plain `exec-once` setups need
`dbus-update-activation-environment --systemd WAYLAND_DISPLAY` at startup.
`stance doctor` reports when the shelf is on but capture is not running.

## Panel embedding
The standalone shell (`quickshell/stance/shell.qml`) has `ipc call stance clip`.
For end4/ii, `quickshell/integrations/end4-pC/StanceClipShelf.qml` holds one
shelf for the whole shell with its own IPC target `stanceclip`.

## Storage
`$XDG_DATA_HOME/stance/clip/` (0700):
- `index.json` (atomic write, schema 1, `rev` counter) — metadata + preview only.
- `blobs/<sha256>.<ext>` (0600) — full text (`.txt`) or image.
- `lock` — flock; every writer does load → mutate → atomic write under it.
  `config.lock` guards `clip.json` (config put / pause) the same way.
  Readers do not lock (rename is atomic).
- `$XDG_STATE_HOME/stance/clip.rev` — rewritten with the rev number after every
  mutation; the panel watches it.

Blobs are written under the lock (temp file + rename) so `sweep` never races
a concurrent ingest; an existing blob of the wrong size is rewritten. A
damaged `index.json` is moved to `index.json.corrupt` by the next writer.
`list --limit N` is per kind. Previews drop bidi/zero-width characters;
non-ASCII hosts stay text (homographs).

Limits: at most 5000 unpinned items and 1 GiB in total; the oldest unpinned
items go first. Index entries with a malformed hash/extension are dropped on
load. A damaged index is moved aside with its blobs to `corrupt-<time>/` (doctor
warns until you delete it) and the shelf starts empty. An index from a newer stance is
refused untouched. `clip purge` deletes removed items at once (otherwise kept a
day for `undo`). `clip get` replaces control characters when printing to a
terminal.

Ids: monotonic integer as string (`"417"`), validated `[0-9]+` before any use.

## CLI and JSON contract
```
stance clip ingest [--image]      # stdin + CLIPBOARD_STATE (wl-paste --watch)
stance clip list [--kind text|link|image] [--query Q] [--limit N] [--json]
stance clip get ID                # text to stdout / image path
stance clip copy ID
stance clip pin|unpin ID…
stance clip rm ID…
stance clip undo
stance clip prune
stance clip clear [--all]         # unpinned only; --all wipes everything
stance clip config [show|put --data JSON] [--json]
```
`list --json` (golden file: `tests/fixtures/clip-list.json`):
```
{"ok":true,"schema":1,"data":{"rev":42,"enabled":true,"items":[
 {"id":"417","kind":"text|link|image","created":1731300000,"updated":1731300100,
  "pinned":false,"preview":"…","host":"example.com"|null,"size":1234,
  "mime":"text/plain"|"image/png"|…,"path":"/abs/blobs/<sha>.png"|null,
  "w":1920|null,"h":1080|null}]}}
```
Sort: pinned first, then `updated` descending. `preview` has control characters
replaced by spaces. `path` is set for images only and always inside the clip dir.
Mutations print `{"ok":true,"schema":1,"data":{"rev":43}}`; errors are the usual
`{"ok":false,"error":{"code":…,"message":…}}` with a non-zero exit.
`config show --json`: `{"ok":true,"schema":1,"data":{"enabled":false,"text":true,
"image":true,"retention_days":30}}`.

## Security
No network. No shell strings. Blob names are hashes, ids are digits. Images are
opened with O_NOFOLLOW; the panel only shows paths under the clip dir. QML shows
clip text with `Text.PlainText` (never rich text). Nothing is stored unless
enabled; sensitive copies never are.
