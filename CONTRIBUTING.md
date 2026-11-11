# Contributing

Thanks for helping. A few rules keep stance safe and small:

- **Never run risky display changes in your own session while testing.**
  `tests/nested.sh lua target/release/stance` starts a throwaway nested Hyprland
  with headless outputs; QML is tried the same way (see docs/quickshell.md).
- `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test` and the
  nested tests must pass.
- No shell strings: external tools run by absolute path with an argument list
  and a timeout (`src/tool.rs`). Modes and phone buttons are whitelisted
  actions, never commands.
- Every `--json` reply keeps the envelope in docs/protocol.md; breaking changes
  bump `schema`.
- New user-visible panel text goes through `StanceI18n` with a Turkish
  translation (or leave the English and say so in the PR).
- Security problems: report privately (GitHub security advisory), not in an issue.

The phone app lives in [stance-deck-android](https://github.com/H0dg4m/stance-deck-android);
protocol changes need both repositories and new test vectors.
