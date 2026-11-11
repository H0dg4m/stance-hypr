# Security and bug scan checklist

Run by a separate agent (or person) after every version and every large
change. **Incremental by default:** scan only what changed since the last scan
(the commit recorded at the top of the previous `docs/scans/*.md`), never the
whole tree from scratch. The scanner may decide a full scan is warranted
(e.g. many small changes touching the hot files, a new attack surface, before
0.9/1.0) and must say why. Findings go to `docs/scans/<version>.md` with
severity, a reproducer and the commit range scanned; a different agent fixes;
the re-scan covers the fixes only.

Hot files (read when the diff touches them or what they call): `src/deck/*`, `src/clip.rs`, `src/apply.rs`,
`src/persist.rs`, `src/theme.rs`, `src/feature.rs`, `src/setup.rs`,
`src/tool.rs`, every QML `Process`.

Automated first (CI): `cargo audit`, `cargo clippy -D warnings`, tests, nested
tests (local), qmllint.

Judgement checklist:
- [ ] Process spawning: absolute path, argv list, timeout, no shell strings, no user text as a flag
- [ ] Paths: no traversal (ids validated before joins), O_NOFOLLOW on private files, symlink and TOCTOU checks
- [ ] State files: flock around read-modify-write, atomic rename, 0600/0700, crash leaves no dangling reference
- [ ] Input limits: every reader is size-capped; JSON with unknown keys refused where it configures behaviour
- [ ] Deck: Noise nonces/replay, pairing window 120 s one-shot, rate limit, only whitelisted actions, loopback mode
- [ ] Clipboard: sensitive / non-data states never stored, pause honoured, nothing logged
- [ ] QML: user data shown as PlainText, no string-built commands, no eval of config values
- [ ] Secrets/personal data never in logs, `--json` errors or `doctor --report`
- [ ] Error paths: no panic/unwrap reachable from input in daemon, guardian, deck server
- [ ] Docs match behaviour (threat model, limits)
