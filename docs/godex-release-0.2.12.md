# godex 0.2.12 Release Notes

Release date: 2026-04-02

## Summary

`godex 0.2.12` fixes the first-run isolated config flow so `godex -g` now boots
cleanly on machines that do not already have a `~/.godex` directory.

## Release Headlines

- auto-initialize `~/.godex` before `godex -g` starts reading isolated config
- preserve strict errors for explicitly bad config-home overrides on config
  commands instead of silently masking them
- add CLI regression coverage for first-run isolated startup and invalid
  explicit config-home paths

## What Changed

- `codex-rs/cli/src/main.rs` now creates the isolated global config directory
  before exporting `CODEX_HOME`/`GODEX_HOME` for `-g`
- `codex-rs/cli/tests/godex_home.rs` verifies both first-run `.godex`
  creation and failure behavior for bad explicit config-home paths
- `docs/config.md` and `docs/godex-fork-manifest.md` now document that
  first-run `godex -g` initializes `~/.godex`

## Verification

- `cargo test -p codex-cli godex_home_flag_parses --manifest-path codex-rs/Cargo.toml`
- `cargo test -p codex-cli --test godex_home --manifest-path codex-rs/Cargo.toml`
- `cargo fmt --all --manifest-path codex-rs/Cargo.toml`

## Distribution Status

- GitHub release flow: ready after this release commit is pushed and tagged
- npm distribution: still blocked until this machine regains valid npm auth for
  `@leonsgp43/godex`
