# Changelog

All notable changes to this fork are documented in this file.

## [Unreleased]

### Changed

- What changed: introduced `godex` fork governance work, including parallel config namespace support, fork-specific release tracking, and official Codex upstream monitoring.
- Why: this fork needs to run beside official Codex while still making upstream merges and fork releases manageable.
- Impact: `godex` can distinguish default `.codex` compatibility mode from isolated `-g` mode, and release/version management now has a dedicated home in the repository.
- Verification: targeted `cargo check`, CLI parse tests, config loader tests, and snapshot-oriented TUI verification are used while iterating on this work.
- Files: `codex-rs/cli/src/main.rs`, `codex-rs/core/src/config/mod.rs`, `codex-rs/tui/src/updates.rs`, `docs/config.md`

- What changed: added a dedicated source installer for `godex` and documented the parallel install path.
- Why: the fork now ships a `godex` binary, but a local source build still needs a stable install step so `godex` is directly invokable without overwriting official `codex`.
- Impact: local development builds can be installed into a user bin directory with a single script, while official `codex` remains available side-by-side.
- Verification: installer help and dry-run paths can be exercised without mutating the shell environment.
- Files: `scripts/install/install-godex-from-source.sh`, `docs/install.md`, `README.md`

- What changed: retargeted fork metadata, feedback links, and remote announcement plumbing to the public `LeonSGP43/godex` repository.
- Why: the fork is being published under the authenticated GitHub account, so in-product links and startup announcements need to resolve against the actual live repository.
- Impact: release notes, issue links, and remote announcement tips can now be served from the final public fork instead of placeholder repo names.
- Verification: repository resolution was checked with `gh repo view`, and raw announcement URLs are expected to resolve after the first push to `LeonSGP43/godex`.
- Files: `codex-rs/core/src/branding.rs`, `codex-rs/tui/src/tooltips.rs`, `codex-rs/tui_app_server/src/tooltips.rs`, `announcement_tip.toml`

## [0.1.0] - 2026-03-23

### Added

- What changed: bootstrapped first-class version governance for the fork with `VERSION`, a structured changelog, and a documented versioning policy.
- Why: the upstream repository did not provide fork-local release governance suitable for independent `godex` releases.
- Impact: future releases can align repository metadata, binary version output, and changelog entries from a single SemVer baseline.
- Verification: repository metadata is checked locally against the workspace Cargo version and changelog structure.
- Files: `VERSION`, `CHANGELOG.md`, `README.md`, `codex-rs/Cargo.toml`
