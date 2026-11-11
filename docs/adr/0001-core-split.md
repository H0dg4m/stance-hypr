# ADR 0001 — Core split: desktop backends, local IPC, versioned config (0.4.0)

Status: accepted, revised after review · 2026-11-11 (see "Revision" at the end)

## Context
stance is Hyprland-only today, but the maintainer plans KDE, GNOME and Windows
after 1.0. Windows and GNOME have no layer-shell/Quickshell, so their UI will be
a different program. Before 1.0 freezes anything, the core must not assume
Hyprland and the panel must not assume "a QML shell that spawns the CLI".

Today:
- `trait Backend` (src/backend.rs) exists with one real implementation
  (`Hyprland`) and a test `Fake` in apply.rs, but Hyprland types leak:
  `OptKey` is a list of hyprlang paths, `Parser` (lua/legacy) is a Hyprland
  concept, `main.rs` calls `Hyprland::connect()` for 17 commands, the daemon
  reads Hyprland's event socket directly, `persist` writes Hyprland config.
- The panel talks to the core by spawning `stance … --json` per action and by
  watching state files (`pending.json`, `clip.rev`).
- Config files have no version: panel.json, clip.json, features.json,
  deck.toml, rules.toml, modes/*.toml, memory.json (index.json and themes have
  one).

## Decisions

### 1. Backend = capabilities, chosen at runtime
- `backend::connect() -> Result<Box<dyn Backend>>` picks the backend from the
  session (`HYPRLAND_INSTANCE_SIGNATURE` → Hyprland; later
  `KDE_FULL_SESSION`/`XDG_CURRENT_DESKTOP` → KDE/GNOME). `--backend NAME` and
  `STANCE_BACKEND` override (tests, debugging).
- `Backend::capabilities() -> Caps` (`displays`, `mirror`, `vrr`,
  `transform`, `workspaces`, `compositor_options`, `persist`, `events`) —
  commands that need a missing capability fail with code `unsupported`; the
  panel hides what is not there (`stance caps --json`).
- Hyprland-only things move behind optional sub-traits/capabilities:
  compositor options (animations/blur/tearing/gaps → `CompositorOptions`
  with *neutral* keys: `animations`, `blur`, `tearing`, `gaps_in`,
  `gaps_out`; the Hyprland backend maps them to hyprlang paths), config
  persistence (`Persist`), the Lua/legacy parser (inside the Hyprland module).
- Events: `Backend::events() -> Box<dyn Iterator<Item = Event>>` with neutral
  events (`MonitorAdded{name}`, `MonitorRemoved{name}`, `ConfigReloaded`);
  the daemon consumes those instead of Hyprland's socket2 lines.
- Mode/rule files keep their format; Hyprland-specific keys stay valid and are
  reported as `unsupported` on other backends.
- A `Fake` backend lives in `src/backend/fake.rs` (cfg(test) + `--backend fake`
  for integration tests without a compositor).

### 2. Local IPC with an event stream (the daemon owns it)
- `stance daemon` (already optional and long-running) also serves
  `$XDG_RUNTIME_DIR/stance/ipc.sock` (0600, same-UID check with SO_PEERCRED).
- Protocol: newline-delimited JSON. Request `{"id":N,"cmd":[...argv...]}` →
  reply `{"id":N, …same envelope as --json…}`; `{"id":N,"cmd":["subscribe"]}`
  → stream of `{"event":"monitors"|"pending"|"clip"|"features"|"theme"|…}`.
  The argv is the CLI argv (same parser, same allowlist; no new command
  surface). Commands run in-process with the same code path as the CLI.
- Clients: QML panel uses `Quickshell.Io.Socket` when the socket exists and
  falls back to spawning the CLI (zero-daemon setups keep working). A Windows/
  GNOME UI later speaks the same protocol (named pipe on Windows).
- Not exposed beyond the user: no TCP, no D-Bus service in 0.4.

### 3. Every config file carries `schema`
- JSON: top-level `"schema": 1`; TOML: `schema = 1`. Missing = 1 (all 0.2/0.3
  files). Readers accept ≤ current; newer → refuse to *write* (read-only use
  allowed where safe) with code `newer_schema`. Writers always write current.
- `src/migrate.rs` holds per-file upgrade steps (none needed for v1) and a
  test that loads 0.2.0/0.3.0 fixtures.

### 4. Follow the system light/dark preference
- `stance theme system` → `dark` | `light` | `unknown`, read from the portal
  (`org.freedesktop.portal.Settings.ReadOne org.freedesktop.appearance
  color-scheme` via `busctl --user`), falling back to
  `gsettings get org.gnome.desktop.interface color-scheme`.
- panel.json: `"theme": "auto"` with `"themeLight"`, `"themeDark"` (defaults
  stance-light / stance-dark); the daemon emits a `theme` event when the
  portal setting changes (`busctl --user monitor` child, only while
  subscribed), else the panel re-reads on open.
  (`auto` is reserved since 0.3 — no user theme can take it.)

### 5. Windows / GNOME UI (decision only)
- The QML panel stays the Linux layer-shell UI (Hyprland, later KDE/wlroots).
- GNOME and Windows get a separate small UI client over the IPC of §2 (toolkit
  chosen when that work starts: Tauri, Slint or Qt Widgets — Rust core is
  reused as a library or as the daemon).
- Consequence now: no business logic in QML (validation and state live in the
  core; QML only renders and calls commands).

## Out of scope for 0.4
KDE/GNOME/Windows backends themselves, D-Bus service, protocol v2 for the phone.

## Revision (after review, 2026-11-11) — what 0.4.0 actually does
Review found that commands print directly (no `fn(args) -> Value` core), that
`Backend` is not `Send`, and that an argv-over-socket server in the daemon
would add a resident attack surface and threading for a ~ms win. So:

- **§2 IPC socket is deferred.** 0.4 adds `stance events` instead: a command
  that streams JSON lines (backend events + state-file changes) as a child
  process of any UI. A socket comes only when a non-QML client exists, as
  *typed* commands (`{"cmd":"mode.apply","args":{…}}`) on top of the core
  split, never CLI argv; `daemon`, `guardian`, `deck serve`, `clip ingest`,
  `setup`, `doctor --report` would never be callable through it.
- **§4** keeps `stance theme system`; no daemon/busctl watcher — the panel
  re-reads on open and can follow `Qt.styleHints.colorScheme`.
- `Persist` stays Hyprland-only behind a capability (no trait yet).

Ordered steps (each its own commit, tests green):
1. `src/backend/fake.rs` (from apply.rs tests, seedable from JSON, state in its
   own file) + `backend::connect()` + `--backend` / `STANCE_BACKEND`; swap the
   `Hyprland::connect()` call sites; integration tests with `--backend fake`.
2. Hyprland option paths (`OptKey::path`, lua/legacy rendering, parsing) into
   the Hyprland module; `OptKey` neutral; `Caps` + `stance caps --json`;
   error code `unsupported`.
3. Core/presentation split in main.rs: commands return serializable results,
   one renderer prints (start with monitors/status/set/confirm/revert).
4. night/brightness/daemon/guardian off `hypr::Instance`;
   `Backend::compat_check()` replaces the doctor/`--force-version` gate.
5. `Backend::events()` with neutral `Event`s; daemon consumes them (scripted
   event source + injected clock in tests).
6. `schema` in every config file, `src/migrate.rs`, 0.2/0.3 fixtures,
   `newer_schema` refusal (check strict readers first).
7. `stance theme system`; panel `theme: "auto"` + `themeLight`/`themeDark`.
8. `stance events`; panel may use it instead of folder watches.
