# Changelog

All notable changes to this fork are documented in this file.

## [Unreleased]

### Changed

- What changed: changed startup announcement loading to synchronously initialize the remote announcement cache on first tooltip use while keeping the prewarm path.
- Why: the previous async-only path could miss a freshly updated repository announcement on the first rendered startup screen.
- Impact: a fresh `godex` process now shows the latest public announcement tip from `LeonSGP43/godex` on first launch instead of falling back to a random local promo.
- Verification: `cargo check -p codex-cli --bin godex --message-format short` passed, and a fresh launch of `target/debug/godex` displayed `REMOTE SYNC VERIFIED 2026-03-25 from LeonSGP43/godex main`.
- Files: `codex-rs/tui/src/tooltips.rs`, `codex-rs/tui_app_server/src/tooltips.rs`, `CHANGELOG.md`

- What changed: refreshed the public `announcement_tip.toml` marker for a live godex remote-tip sync verification.
- Why: the fork now serves startup announcements from `LeonSGP43/godex`, so a fresh remote marker makes it easy to verify that new launches pick up the latest repository content instead of stale assumptions.
- Impact: new `godex` processes should resolve the updated public tip text from the fork repository on startup.
- Verification: the repository file is pushed to `main`, fetched back from `raw.githubusercontent.com`, and checked against the expected live marker string.
- Files: `announcement_tip.toml`, `CHANGELOG.md`

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
