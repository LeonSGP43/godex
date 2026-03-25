# Changelog

All notable changes to this fork are documented in this file.

## [Unreleased]

## [0.1.1] - 2026-03-25

### Added

- What changed: promoted the fork policy into root-level constitutional files for Codex and Claude Code with aligned governance, sync discipline, manifest requirements, and engineering rules.
- Why: the repository needed one durable, agent-readable constitution so future maintenance and upstream sync work follows the same legal surface regardless of which coding agent is operating in the repo.
- Impact: both `AGENTS.md` and `CLAUDE.md` now enforce the same upstream-first fork model, hot-file discipline, branch policy, acceptance gates, and documentation obligations.
- Verification: the new constitutional documents were checked against the fork guidelines, manifest, maintenance workflow, and current root documentation links.
- Files: `AGENTS.md`, `CLAUDE.md`, `README.md`, `CHANGELOG.md`

- What changed: documented long-term fork maintenance policy with a dedicated guidelines document and an explicit fork manifest.
- Why: keeping `godex` close to upstream Codex requires a stable branch model, a bounded fork surface, and a written source of truth for which differences are allowed to survive upstream sync.
- Impact: future maintainers now have a concrete policy for sync branches, hot-file handling, acceptance gates, patch groups, and fork-owned behavior review before merging upstream updates back into `main`.
- Verification: policy docs were reviewed against the current maintenance workflow and the known hot-overlap files in the fork.
- Files: `docs/godex-fork-guidelines.md`, `docs/godex-fork-manifest.md`, `docs/godex-maintenance.md`, `README.md`, `CHANGELOG.md`

- What changed: established a repo-local maintenance baseline for the `godex` fork with a committed `.codex/config.toml`, a `godex-maintain.sh` wrapper, and a dedicated maintenance runbook.
- Why: long-term fork health depends on having one repeatable path for upstream sync, release metadata checks, and source rebuilds instead of relying on scattered manual commands or global-only config.
- Impact: this checkout can now track `LeonSGP43/godex` and `openai/codex` with project-local defaults, and maintainers have a single script entrypoint for status, dry-run sync, compile checks, smoke checks, and release preflight.
- Verification: shell syntax and command-path validation will be exercised with `bash -n scripts/godex-maintain.sh`, `bash scripts/godex-maintain.sh status`, `bash scripts/godex-maintain.sh sync --dry-run`, and `bash scripts/godex-maintain.sh release-preflight`.
- Files: `.codex/config.toml`, `scripts/godex-maintain.sh`, `docs/godex-maintenance.md`, `README.md`, `CHANGELOG.md`

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

- What changed: merged official Codex upstream through `e590fad50b83` into `godex`, then fast-forwarded `main` to that validated sync result.
- Why: the fork needs continuous, constitutional upstream absorption instead of letting divergence grow around a stale baseline.
- Impact: `godex` now carries the newer plugin, app-server, protocol, multi-agent, sandboxing, code-mode, and CI changes from official Codex while preserving only the manifest-listed fork behavior.
- Verification: `bash scripts/godex-maintain.sh status`, `bash scripts/godex-maintain.sh sync --dry-run`, `bash scripts/godex-maintain.sh check`, `bash scripts/godex-maintain.sh smoke`, and `bash scripts/godex-maintain.sh release-preflight` all passed on the sync branch before mergeback.
- Files: `codex-rs/`, `.github/`, `MODULE.bazel`, `patches/`, `scripts/test-remote-env.sh`

- What changed: added a hard pre-push version gate for `main`, prepared formal release notes for `0.1.1`, and promoted the pending changelog entries into a real release section.
- Why: release metadata must advance with the code, otherwise future `main` pushes can silently publish significant fork changes under an old version number.
- Impact: pushing `main` now requires a fresh `VERSION`, aligned Cargo workspace versioning, a matching changelog release heading, and an emptied `Unreleased` section for the release being published.
- Verification: `bash scripts/godex-maintain.sh release-preflight` now checks version alignment and rejects a push-ready `main` if it still carries the same version as `origin/main`.
- Files: `VERSION`, `codex-rs/Cargo.toml`, `CHANGELOG.md`, `scripts/godex-maintain.sh`, `docs/godex-release-0.1.1.md`, `AGENTS.md`, `CLAUDE.md`, `README.md`, `docs/godex-maintenance.md`

### Fixed

- What changed: limited the legacy `agent/backend` module to test-only compilation.
- Why: upstream runtime changes left that backend layer as test support only, but it was still entering production builds and generating dead-code warnings.
- Impact: normal `godex` builds no longer emit the old `core/src/agent/backend.rs` dead-code warning set, while tests keep the helper code they still need.
- Verification: `bash scripts/godex-maintain.sh check` completed without the previous `backend.rs` dead-code warnings, and `bash scripts/godex-maintain.sh smoke` still reported `godex 0.1.1`.
- Files: `codex-rs/core/src/agent/mod.rs`

## [0.1.0] - 2026-03-23

### Added

- What changed: bootstrapped first-class version governance for the fork with `VERSION`, a structured changelog, and a documented versioning policy.
- Why: the upstream repository did not provide fork-local release governance suitable for independent `godex` releases.
- Impact: future releases can align repository metadata, binary version output, and changelog entries from a single SemVer baseline.
- Verification: repository metadata is checked locally against the workspace Cargo version and changelog structure.
- Files: `VERSION`, `CHANGELOG.md`, `README.md`, `codex-rs/Cargo.toml`
