# Changelog

All notable changes to this fork are documented in this file.

## [Unreleased]

### Fixed

- What changed: added `--fast-release` to `scripts/install/install-godex-from-source.sh` so local source installs can stay on the `release` path while overriding Cargo's default `fat LTO` profile with faster local build settings (`lto=off`, `codegen-units=16`) and skipping the macOS Homebrew-LLVM/Rust-LLD compatibility wrapper in favor of the native linker. Documented the flag in the install guide.
- Why: on this machine the native-linker release build no longer failed outright, but the final `godex` release link under the upstream `fat LTO` profile remained impractically slow for day-to-day local installation.
- Impact: developer machines now have an explicit release-mode install path that is much faster for local use, without changing the repository's default release profile or published artifact settings.
- Verification: `bash scripts/install/install-godex-from-source.sh --fast-release`, `~/.local/bin/godex --version`
- Files: `scripts/install/install-godex-from-source.sh`, `docs/install.md`, `CHANGELOG.md`

- What changed: taught `scripts/install/install-godex-from-source.sh` to retry the release build with the native macOS linker when the Homebrew `clang` plus Rust `ld64.lld` compatibility path fails with the known `libwebrtc_sys` versus `libv8` duplicate-symbol conflict, and to remove a pre-existing `godex` symlink in the install dir before copy-install so the installer does not follow an old Homebrew/npm link target. Documented the linker fallback in the install guide.
- Why: on this machine the source installer could repeatedly fail near the end of the `godex` release build, leaving the old npm-managed `0.2.13` binary on `PATH` even though the repo itself had already advanced to `0.2.21`. The existing install path was also a symlink, which made direct copy-install unsafe.
- Impact: source installs on macOS keep the faster compatibility linker when it works, but no longer hard-fail on this upstream dependency collision; they automatically retry with the native linker so `godex` can still be built and installed, and they safely replace a symlinked install path with a real copied binary.
- Verification: `bash scripts/install/install-godex-from-source.sh`, `godex --version`, `godex4 --version`
- Files: `scripts/install/install-godex-from-source.sh`, `docs/install.md`, `CHANGELOG.md`

## [0.2.21] - 2026-04-14

### Fixed

- What changed: fixed the TUI release-build regression in `codex-rs/tui/src/updates.rs` by converting the `codex_home.join("updates.json")` result back into a `PathBuf`, which matches the function contract after upstream typed `codex_home` as an absolute-path wrapper.
- Why: the initial `0.2.20` release publication attempt exposed this mismatch only during the full release build, which blocked artifact packaging and npm publication even though earlier compile checks had passed.
- Impact: the release pipeline can build `codex-tui` and `godex` cleanly again, so `0.2.21` becomes the first fully publishable version on top of the new `rust-v0.120.0` upstream baseline.
- Verification: `cargo check -p codex-tui --manifest-path codex-rs/Cargo.toml`, `cargo check -p codex-cli --bin godex --manifest-path codex-rs/Cargo.toml`, `bash scripts/godex-maintain.sh release-preflight`
- Files: `codex-rs/tui/src/updates.rs`, `VERSION`, `codex-rs/Cargo.toml`, `codex-rs/Cargo.lock`, `README.md`, `CHANGELOG.md`

## [0.2.20] - 2026-04-14

### Changed

- What changed: rebased the fork onto official release `rust-v0.120.0` (`65319eb1400cbd2890c43d572263dabd25f18ba9`), merged the subsequently fetched `upstream/main` commits through `a6b03a22cc35b36d46065185c7982cd02bb82c4e`, resolved the follow-up conflict in `codex-rs/core/src/codex.rs`, and preserved the fork-owned version line and release identity.
- Why: the fork should track the latest official stable Codex release rather than continuing to advertise the older `rust-v0.118.0` baseline.
- Impact: the repository now truthfully reports official release baseline `rust-v0.120.0`, still contains the current fetched `upstream/main`, and keeps `godex` on its own fork release line instead of collapsing to upstream package versioning.
- Verification: `git merge-base --is-ancestor rust-v0.120.0 HEAD`, `git merge-base --is-ancestor upstream/main HEAD`, `git rev-list --left-right --count HEAD...rust-v0.120.0`, `git rev-list --left-right --count HEAD...upstream/main`, `cargo check -p codex-cli --bin godex --manifest-path codex-rs/Cargo.toml`, `bash scripts/godex-maintain.sh refresh-upstream-metadata --dry-run`, `bash scripts/godex-maintain.sh release-preflight`
- Files: `VERSION`, `codex-rs/Cargo.toml`, `codex-rs/Cargo.lock`, `UPSTREAM_VERSION`, `UPSTREAM_COMMIT`, `UPSTREAM_HEAD_COMMIT`, `README.md`, `docs/godex-fork-manifest.md`, `CHANGELOG.md`

## [0.2.19] - 2026-04-14

### Changed

- What changed: merged the current official `upstream/main` snapshot through commit `3b24a9a53264f96e7caeea0577b994b0d10a8c6f`, resolved the post-merge TUI and CLI compile boundary mismatches plus the follow-up async API drift in thread spawn and MCP dependency refresh, and restored a green `cargo check -p codex-cli --bin godex`.
- Why: the fork needs to stay caught up with the latest official Codex code while keeping the fork-owned backend, memory, namespace, and branding seams intact.
- Impact: this sync branch now fully contains the latest fetched official upstream `main`, `godex` compiles again on top of that code, and the remaining drift is fork-owned divergence rather than an incomplete upstream merge.
- Verification: `git merge-base --is-ancestor upstream/main HEAD`, `git rev-list --left-right --count HEAD...upstream/main`, `cargo check -p codex-cli --bin godex --manifest-path codex-rs/Cargo.toml`
- Files: `codex-rs/core/src/thread_manager.rs`, `codex-rs/core/src/mcp_skill_dependencies.rs`, `codex-rs/core/src/tools/runtimes/shell.rs`, `codex-rs/core/src/tools/network_approval.rs`, `codex-rs/app-server/src/codex_message_processor.rs`, `codex-rs/tui/src/lib.rs`, `codex-rs/cli/src/mcp_cmd.rs`, `codex-rs/app-server-client/src/lib.rs`

### Changed

- What changed: expanded the machine-readable upstream tracking metadata so the fork now records both the latest merged official release tag (`UPSTREAM_VERSION` and `UPSTREAM_COMMIT`) and the exact merged `upstream/main` head (`UPSTREAM_HEAD_COMMIT`), and updated the maintainer scripts plus baseline docs to validate both views together.
- Why: recording only the latest release tag was no longer sufficient once the fork started syncing beyond stable release tags onto newer upstream `main` commits.
- Impact: maintainers can now tell at a glance both which official release line the fork includes and which exact upstream `main` commit has been merged, while `release-preflight` blocks stale or misleading metadata.
- Verification: `bash scripts/godex-maintain.sh refresh-upstream-metadata --dry-run`, `bash scripts/godex-maintain.sh release-preflight`
- Files: `UPSTREAM_VERSION`, `UPSTREAM_COMMIT`, `UPSTREAM_HEAD_COMMIT`, `README.md`, `docs/godex-fork-manifest.md`, `scripts/godex-maintain.sh`, `docs/godex-maintenance.md`, `docs/godex-fork-guidelines.md`

## [0.2.18] - 2026-04-13

### Changed

- What changed: pushed the fork further into a patch-layer architecture for the main fork-owned surfaces. Real provider execution is now documented and routed around `[agent_backends.<name>]` plus external worker samples instead of provider-branded prompt-role shims, memory-specific scope/query/path glue is concentrated behind `core/src/fork_patch/memory.rs` and `state/src/fork_patch/memory_repo.rs`, and the remaining login/MCP/proxy/runtime-ui bootstrap residue is reduced to thinner adapters.
- Why: the fork needs to keep its custom backend, memory, and bootstrap behavior without forcing every upstream sync to reopen the same hot-path conflicts.
- Impact: future merges should concentrate conflict risk in a smaller set of fork-owned seams, external backend providers can be registered without recompiling the Codex binary, and the current memory/bootstrap features remain available with less direct coupling to upstream internals.
- Verification: `bash scripts/godex-maintain.sh check`, `cargo test -p codex-core spawn_agent_with_command_backend_uses_backend_default_model_and_backend_id --manifest-path codex-rs/Cargo.toml`, `cargo test -p codex-cli login_status_reports_api_key_auth --manifest-path codex-rs/Cargo.toml`, `cargo test -p codex-cli logout_reports_removed_credentials --manifest-path codex-rs/Cargo.toml`, `cargo test -p codex-state --lib --manifest-path codex-rs/Cargo.toml`, `cargo test -p codex-app-server --tests --no-run --manifest-path codex-rs/Cargo.toml`, `cargo test -p codex-tui resume_picker_thread_names_snapshot --manifest-path codex-rs/Cargo.toml`
- Files: `codex-rs/core/src/agent/backend.rs`, `codex-rs/core/src/fork_patch/memory.rs`, `codex-rs/state/src/fork_patch/memory_repo.rs`, `codex-rs/cli/src/root_cli_policy.rs`, `codex-rs/core/src/network_proxy_loader.rs`, `docs/external-agent-backends.md`, `docs/godex-fork-manifest.md`, `docs/godex-memory-mvp-closure.md`

### Docs

- What changed: refreshed the fork manifest, inventory ledger, patch master plan, memory patch-layer plan, memory MVP closure, and external-backend guidance so the current patch groups, owner files, validation commands, and upstream-replacement triggers are tracked explicitly.
- Why: once the fork is maintained as a patch layer, these docs become the source of truth for sync decisions, patch deletion, and future upstream replacement checks.
- Impact: maintainers can audit which differences are durable versus migrate/shrink lanes, freeze non-MVP residue instead of expanding it again, and decide more safely when an upstream feature should replace a fork patch.
- Verification: `bash scripts/godex-maintain.sh status`, `bash scripts/godex-maintain.sh release-preflight`
- Files: `docs/agent-roles.md`, `docs/external-agent-backends.md`, `docs/godex-fork-inventory-ledger.md`, `docs/godex-fork-manifest.md`, `docs/godex-fork-patch-master-plan.md`, `docs/godex-memory-mvp-closure.md`, `docs/godex-memory-patch-layer-plan.md`

## [0.2.17] - 2026-04-06

### Docs

- What changed: tightened the memory-scope documentation in the README, config guide, and memory architecture spec so the docs now spell out scope precedence, project-root detection (`project_root_markers`, default `.git`), storage layout under both `~/.codex` and `~/.godex`, and what exactly remains isolated in project mode.
- Why: the feature already existed, but the operator docs still left room for confusion around which launch setting wins, how directory-level memory partitions are derived, and whether project scope also isolates summary and recall artifacts.
- Impact: users can now choose `global` versus `project` memory with a precise mental model, which reduces accidental cross-project memory use and makes startup troubleshooting much faster.
- Verification: `cargo test -p codex-core --test memory_scope_smoke launch_overrides_resolve_distinct_memory_roots --manifest-path codex-rs/Cargo.toml -- --exact`
- Files: `README.md`, `docs/config.md`, `docs/godex-memory-system.md`, `CHANGELOG.md`

### Test

- What changed: added a dedicated `memory_scope_smoke` integration test that proves launch-time `global` and `project` memory scopes resolve to different roots, reuse the same project scope directory across repeated launches, and keep `MEMORY.md` writes isolated between the two scopes.
- Why: the memory-scope feature needed an explicit smoke test at the launch/config boundary so future refactors cannot silently collapse project-local memory back into the shared global root.
- Impact: maintainers now have a repeatable verification step for the most important operator guarantee in this feature: choosing `project` at startup must not read or write the same memory root as `global`.
- Verification: `cargo test -p codex-core --test memory_scope_smoke launch_overrides_resolve_distinct_memory_roots --manifest-path codex-rs/Cargo.toml -- --exact`
- Files: `codex-rs/core/tests/memory_scope_smoke.rs`

## [0.2.16] - 2026-04-06

### Docs

- What changed: expanded the memory-scope documentation across the README, config guide, and memory architecture doc so the fork now explains how `memories.scope` and `godex --memory-scope global|project` interact, where project-partitioned artifacts live, how `-g` changes the storage root, and which operator patterns are recommended.
- Why: the feature itself was shipped, but the operator-facing docs still left gaps around precedence, path layout, and when to choose global versus project-local memory.
- Impact: maintainers and users now have a clearer startup and troubleshooting path for memory scope selection, with less ambiguity about temporary overrides versus persistent config defaults.
- Verification: `bash scripts/godex-maintain.sh release-preflight`
- Files: `README.md`, `docs/config.md`, `docs/godex-memory-system.md`, `CHANGELOG.md`, `VERSION`, `codex-rs/Cargo.toml`

## [0.2.15] - 2026-04-05

### Added

- What changed: added a global `godex --memory-scope global|project` startup flag that injects a launch-only `memories.scope` override before command dispatch, so interactive mode and subcommands can switch between shared and project-partitioned memory roots without editing `config.toml`.
- Why: the new project-scoped memory system needed a fast operational toggle at startup so the user can decide per launch whether to use global recall or directory-scoped recall while keeping the configured default intact.
- Impact: operators can now keep a stable default in `[memories].scope` and temporarily flip a single run to `project` or `global`, which is especially useful when they want to avoid cross-project memory injection without permanently changing repo-wide settings.
- Verification: `cargo test -p codex-cli memory_scope_flag_parses_before_subcommand --manifest-path codex-rs/Cargo.toml`, `cargo test -p codex-cli memory_scope_flag_parses_after_subcommand --manifest-path codex-rs/Cargo.toml`, `cargo test -p codex-cli --manifest-path codex-rs/Cargo.toml`, `cargo fmt --all --manifest-path codex-rs/Cargo.toml`, `python3 tools/argument-comment-lint/run-prebuilt-linter.py`, `bash scripts/godex-maintain.sh release-preflight`
- Files: `codex-rs/cli/src/main.rs`, `docs/config.md`, `docs/godex-memory-system.md`, `docs/godex-fork-manifest.md`, `README.md`, `CHANGELOG.md`, `VERSION`, `codex-rs/Cargo.toml`, `codex-rs/Cargo.lock`

## [0.2.14] - 2026-04-05

### Added

- What changed: added selectable memory scope modes so `godex` can keep using the legacy global memory root or partition memories per project root, and threaded the chosen scope through config resolution, rollout/session metadata, state persistence, phase-1 extraction, phase-2 consolidation, and read-path injection. The memory read path also now honors a configurable `summary_token_limit` instead of a hard-coded summary truncation ceiling.
- Why: the fork needed a practical way to avoid cross-project memory pollution and reduce unnecessary context consumption without breaking the existing global memory workflow.
- Impact: maintainers can now choose between shared global memory and project-scoped memory partitions, where project mode only reads, writes, and consolidates conversations from the detected project root while keeping memory artifacts stored under the same global `CODEX_HOME` tree.
- Verification: `cargo test -p codex-core memories:: -- --nocapture`, `cargo test -p codex-app-server --tests --no-run --manifest-path codex-rs/Cargo.toml`, `cargo fmt --all --manifest-path codex-rs/Cargo.toml`, `python3 tools/argument-comment-lint/run-prebuilt-linter.py`
- Files: `codex-rs/core/src/config/types.rs`, `codex-rs/core/src/config/mod.rs`, `codex-rs/core/src/memories/scope.rs`, `codex-rs/core/src/memories/prompts.rs`, `codex-rs/core/src/memories/phase1.rs`, `codex-rs/core/src/memories/phase2.rs`, `codex-rs/core/src/codex.rs`, `codex-rs/protocol/src/protocol.rs`, `codex-rs/rollout/src/config.rs`, `codex-rs/rollout/src/metadata.rs`, `codex-rs/state/src/model/thread_metadata.rs`, `codex-rs/state/src/runtime/memories.rs`, `codex-rs/state/src/runtime/threads.rs`, `codex-rs/state/migrations/0023_threads_memory_scope.sql`, `codex-rs/core/config.schema.json`, `docs/config.md`, `docs/godex-memory-system.md`, `docs/godex-fork-manifest.md`, `CHANGELOG.md`, `VERSION`, `codex-rs/Cargo.toml`

## [0.2.13] - 2026-04-03

### Fixed

- What changed: the local `godex` release packaging helper now reads single-file `tar.gz` payloads directly instead of relying on `TarFile.extract(..., filter="data")`, and it adds a focused Python unittest for the ripgrep staging path.
- Why: local release staging was crashing under the maintainer machine's Python 3.10 runtime because that stdlib API does not accept the `filter` keyword, which blocked `godex 0.2.12` tarball staging until the extraction path was made interpreter-compatible.
- Impact: local stage/publish flows can vendor ripgrep successfully on Python 3.10 and still reject non-regular tar members, making future GitHub release uploads more reliable from this machine.
- Verification: `python3 codex-cli/scripts/test_install_native_deps.py`, `HTTP_PROXY=http://127.0.0.1:10808 HTTPS_PROXY=http://127.0.0.1:10808 ALL_PROXY=http://127.0.0.1:10808 bash .codex/skills/godex-release-distributor/scripts/run.sh local-stage`
- Files: `codex-cli/scripts/install_native_deps.py`, `codex-cli/scripts/test_install_native_deps.py`, `CHANGELOG.md`

## [0.2.12] - 2026-04-02

### Changed

- What changed: `godex -g` now initializes the isolated global config root before startup, so first-run invocations create `~/.godex` automatically instead of failing on a missing directory, while explicit bad config-home overrides still remain fatal on config-bearing commands.
- Why: the fork's isolated config mode should be bootstrappable like the default `.codex` flow, but the CLI previously set `CODEX_HOME=~/.godex` before any directory creation and then immediately tripped over its own missing-path validation.
- Impact: users can start `godex -g` on a clean machine without manually creating `~/.godex`, and incorrect manually supplied config-home environment variables still surface as hard errors instead of being silently rewritten when a command resolves config.
- Verification: `cargo test -p codex-cli godex_home_flag_parses --manifest-path codex-rs/Cargo.toml`, `cargo test -p codex-cli --test godex_home --manifest-path codex-rs/Cargo.toml`
- Files: `codex-rs/cli/src/main.rs`, `codex-rs/cli/tests/godex_home.rs`, `docs/config.md`, `docs/godex-fork-manifest.md`, `CHANGELOG.md`

## [0.2.11] - 2026-04-02

### Changed

- What changed: merged official Codex upstream `rust-v0.118.0` into `godex`, resolved the fork hot-file conflicts against the new `tui` layout, preserved the fork-specific config/update surfaces, and fixed the follow-up compile, test, and lint compatibility issues uncovered during verification.
- Why: the fork is governed as an upstream-first release line, so the `rust-v0.118.0` baseline needed to be absorbed promptly without reintroducing broad divergence in runtime or UI internals.
- Impact: `godex` now carries the official `rust-v0.118.0` workspace updates while still preserving the manifest-listed fork behavior for config namespace split, fork update source, and source-repo sync handling.
- Verification: `cargo test -p codex-cli godex_home_flag_parses --manifest-path codex-rs/Cargo.toml`, `cargo test -p codex-cli sync_upstream_subcommand_parses --manifest-path codex-rs/Cargo.toml`, `cargo test -p codex-cli reject_remote_mode_for_non_interactive_subcommands --manifest-path codex-rs/Cargo.toml`, `cargo test -p codex-tui detects_update_action_without_env_mutation --manifest-path codex-rs/Cargo.toml`, `cargo test -p codex-utils-home-dir find_godex_home_without_env_uses_godex_dir --manifest-path codex-rs/Cargo.toml`, `bash scripts/godex-maintain.sh status`, `bash scripts/godex-maintain.sh sync --dry-run`, `bash scripts/godex-maintain.sh check`, `bash scripts/godex-maintain.sh smoke`, `bash scripts/godex-maintain.sh release-preflight`, `cargo fmt --all --manifest-path codex-rs/Cargo.toml`, and `python3 tools/argument-comment-lint/run.py --workspace`.
- Files: `codex-rs/cli/src/main.rs`, `codex-rs/core/src/tools/spec.rs`, `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs`, `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs`, `codex-rs/tui/src/update_action.rs`, `codex-rs/utils/home-dir/src/lib.rs`, `codex-rs/core/src/agent/backend.rs`, `codex-rs/core/src/agent/control_tests.rs`, `codex-rs/core/src/codex.rs`

- What changed: bumped the fork release metadata to `0.2.11` and updated the main README install guidance to state that daily-use `godex` should come from the published npm package, while source install remains a maintainer/development helper.
- Why: `main` is not allowed to advance under the same version, and the README still implied that source install was the primary recommended runtime path even though the repository policy requires the published npm channel for normal local use.
- Impact: the release line is now push-eligible again, and the top-level project documentation matches the enforced distribution and release-governance rules.
- Verification: reviewed `README.md`, `CHANGELOG.md`, `VERSION`, and `codex-rs/Cargo.toml` together, then reran `bash scripts/godex-maintain.sh check`, `bash scripts/godex-maintain.sh smoke`, and `bash scripts/godex-maintain.sh release-preflight`.
- Files: `README.md`, `CHANGELOG.md`, `VERSION`, `codex-rs/Cargo.toml`

## [0.2.10] - 2026-03-31

### Changed

- What changed: added a dedicated memory-system developer guide, linked it from the main README highlights, and added a direct docs/config entrypoint for memory tuning and runtime behavior reference.
- Why: the new QMD hybrid memory path and related knobs needed a single canonical document so future contributors can operate and extend the memory stack without reverse-engineering scattered code paths.
- Impact: maintainers now have an explicit operating manual for indexing, retrieval, configuration switches, and validation flow, reducing onboarding time and lowering regression risk during follow-up memory work.
- Verification: reviewed document links and section references with `rg -n "godex-memory-system|Memory System" README.md docs/config.md CHANGELOG.md` and confirmed the guide file is tracked at `docs/godex-memory-system.md`.
- Files: `docs/godex-memory-system.md`, `README.md`, `docs/config.md`

## [0.2.9] - 2026-03-31

### Added

- What changed: introduced a QMD-hybrid-lite memory recall pipeline that combines BM25 + vector + RRF + rerank, and wired new memory tuning knobs (`qmd_hybrid_enabled`, `qmd_query_expansion_enabled`, `qmd_rerank_limit`) through config types, schema, prompts, and memory artifacts.
- Why: prior memory recall behavior depended on a simpler semantic path and needed a stronger retrieval foundation aligned with OpenClaw-style hybrid memory expectations while staying incremental on the existing framework.
- Impact: memory retrieval now has explicit hybrid signals and richer index artifacts (`memory_index.qmd`, `vector_index.json`), improving recall resilience and future tunability without requiring external embedding services.
- Verification: `cargo test -p codex-core memories:: -- --nocapture`, `cargo test -p codex-core config::tests::test_toml_parsing -- --exact --nocapture`, `cargo run -p codex-core --bin codex-write-config-schema`, `cargo check -p codex-core --lib`, and runtime validation with `c`-prefixed commands produced refreshed memory artifacts under `~/.codex/memories`.
- Files: `codex-rs/core/src/memories/semantic_index.rs`, `codex-rs/core/src/memories/prompts.rs`, `codex-rs/core/src/memories/tests.rs`, `codex-rs/core/src/memories/prompts_tests.rs`, `codex-rs/core/src/config/types.rs`, `codex-rs/core/src/config/config_tests.rs`, `codex-rs/core/config.schema.json`, `codex-rs/core/src/memories/README.md`, `codex-rs/core/templates/memories/read_path.md`, `docs/config.md`

### Fixed

- What changed: fixed source installer cleanup handling for Bash `set -u` by making empty `TEMP_FILES` cleanup safe.
- Why: installer runs could complete installation successfully but still return exit code `1` on shells that treat empty arrays as unbound under strict mode.
- Impact: local install script now exits cleanly after successful install, eliminating false-failure outcomes in automation and release-adjacent flows.
- Verification: `bash scripts/install/install-godex-from-source.sh --debug --copy --no-path --dry-run` and `bash scripts/install/install-godex-from-source.sh --debug --copy --no-path`.
- Files: `scripts/install/install-godex-from-source.sh`

### Changed

- What changed: added a required repository policy that local runnable `godex` must come from published npm distribution and must not be replaced by ad-hoc `target/*` binaries.
- Why: maintainers requested a single release-grade runtime channel so local usage stays aligned with npm/release governance and avoids drift from unpublished binaries.
- Impact: release discipline is now explicit in the project constitution, and future operational guidance must follow npm-first distribution rules.
- Verification: policy text added and reviewed in repository governance document.
- Files: `AGENTS.md`

## [0.2.8] - 2026-03-29

### Fixed

- What changed: hardened the fork release pipeline by switching zsh macOS builds to standard runners, resolving native artifact downloads from the actual workflow repository, skipping unavailable fork Windows npm artifacts, filtering staged npm targets by required native components, and adding fork npm publish token fallback.
- Why: `0.2.7` publishing repeatedly failed due upstream-only release assumptions, missing fork Windows artifacts, and npm auth path mismatches.
- Impact: tagged fork releases can now complete GitHub release packaging and npm publishing for `@leonsgp43/godex` without depending on upstream-only release infrastructure.
- Verification: GitHub Actions run `23702070015` completed with `release=success` and `publish-npm=success`; `npm view @leonsgp43/godex version` returns `0.2.7`.
- Files: `.github/workflows/rust-release-zsh.yml`, `.github/workflows/rust-release.yml`, `codex-cli/scripts/install_native_deps.py`, `scripts/stage_npm_packages.py`, `CHANGELOG.md`, `VERSION`, `codex-rs/Cargo.toml`.

## [0.2.7] - 2026-03-28

### Changed

- What changed: merged the validated `rust-v0.117.0` upstream sync into `main`, including the upstream crate splitouts, plugin and app-server updates, tooling changes, and the fork-side conflict resolutions needed to keep `godex` coherent on top of that release.
- Why: `godex` needs to stay close to official Codex releases while preserving the fork's versioning, config namespace behavior, update governance, and Grok integration.
- Impact: `main` now follows the official `rust-v0.117.0` release line, keeps `godex 0.2.7` as the fork release identity, and remains compatible with both default Codex-style config paths and explicit `godex -g` isolation.
- Verification: `cargo test -p codex-cli sync_upstream_subcommand_parses`, `cargo test -p codex-cli godex_home_flag_parses`, `cargo test -p codex-cli reject_remote_mode_for_non_interactive_subcommands`, `bash scripts/godex-maintain.sh check`, `bash scripts/godex-maintain.sh sync --dry-run`, `bash scripts/godex-maintain.sh release-preflight`, and `codex-rs/target/debug/godex --version`.
- Files: `VERSION`, `codex-rs/Cargo.toml`, `CHANGELOG.md`

## [0.2.6] - 2026-03-26

### Fixed

- What changed: synchronized `codex-rs/Cargo.lock` with the current workspace package version so the tracked workspace crates now resolve as `0.2.5` instead of the stale `0.2.0` entries left in the committed lockfile.
- Why: `VERSION` and `codex-rs/Cargo.toml` were already bumped to `0.2.5`, but the checked-in lockfile still described most workspace packages as `0.2.0`, which left the release metadata and the committed Rust lock state out of sync.
- Impact: `cargo check` and future release verification now run against a lockfile that matches the declared fork version, reducing confusion during smoke checks and making the repository state release-ready again.
- Verification: `bash scripts/godex-maintain.sh check` passed after the lockfile refresh, and `git diff -- codex-rs/Cargo.lock` now shows the workspace package versions aligned to `0.2.5`.
- Files: `codex-rs/Cargo.lock`, `CHANGELOG.md`

- What changed: taught the source installer to keep `release` as the default macOS build profile while automatically routing the linker through Homebrew `clang` plus Rust's bundled `ld64.lld` when that compatibility path is available.
- Why: the fork should keep local source installs on the same `release` profile used for real distribution instead of requiring a fallback test profile on machines where Apple's default linker path chokes on the current Rust/LLVM LTO output.
- Impact: local `godex` installs on compatible macOS machines keep using the standard `cargo build --release` path by default, while avoiding the linker mismatch that previously forced an ad hoc `snapshot-test` workaround.
- Verification: the compatibility wrapper was exercised locally by running `cargo build -p codex-cli --bin godex --release --manifest-path codex-rs/Cargo.toml` with the generated linker wrapper, and the build advanced normally into the heavy `release` crate graph instead of failing immediately with the prior Apple linker / SDK mismatch errors.
- Files: `scripts/install/install-godex-from-source.sh`, `docs/install.md`, `CHANGELOG.md`

## [0.2.5] - 2026-03-25

### Changed

- What changed: redesigned the repository landing page into a fork-owned `godex` homepage, clarified the difference from official Codex, added an explicit acknowledgment section, and aligned install guidance with the currently reliable source-install path.
- Why: the README and install docs should present `godex` as a credible public fork without overstating distribution channels that are not yet consistently live.
- Impact: GitHub visitors now get a clearer explanation of what `godex` is, which fork-specific additions it keeps, how it differs from upstream, and which install path should be trusted today.
- Verification: reviewed `README.md`, `docs/install.md`, and release-governance docs together so install-channel statements and fork-positioning claims no longer conflict.
- Files: `README.md`, `docs/install.md`, `CHANGELOG.md`

## [0.2.4] - 2026-03-25

### Added

- What changed: added local-first release wrapper commands so `godex` can be staged and published from the maintainer machine with `scripts/godex-release.sh`, while keeping an explicit remote fallback path for future use.
- Why: this fork should default to local compilation and local packaging instead of depending on GitHub Actions as the primary build system.
- Impact: future releases can use a simple local entrypoint for stage/publish/status/verify, and remote release commands remain available as a secondary path instead of the default.
- Verification: `sh -n scripts/godex-release.sh scripts/godex-release-local.sh scripts/godex-release-remote.sh`, `bash scripts/godex-release.sh status`, and `bash scripts/godex-release.sh remote verify`.
- Files: `.codex/skills/godex-release-distributor/SKILL.md`, `.codex/skills/godex-release-distributor/scripts/godex_release_distributor.py`, `scripts/godex-release.sh`, `scripts/godex-release-local.sh`, `scripts/godex-release-remote.sh`, `CHANGELOG.md`

### Changed

- What changed: replaced the upstream Codex splash on the repository welcome section with a fork-owned `godex` hero image stored in the repo.
- Why: the GitHub landing page should present the fork's own visual identity instead of inheriting upstream branding at the top of the README.
- Impact: visitors opening the repository on GitHub now see the `godex` hero graphic as the pinned top image.
- Verification: the local image was resized into `.github/godex-readme-hero.jpg`, and the README top `<img>` source now points at that repository asset.
- Files: `.github/godex-readme-hero.jpg`, `README.md`, `CHANGELOG.md`

## [0.2.3] - 2026-03-25

### Fixed

- What changed: granted the `build-windows` caller job the `id-token: write` permission required by the reusable Windows release workflow.
- Why: GitHub rejected the `rust-v0.2.2` tag at workflow startup because the called workflow requested `id-token: write` but the caller job did not allow it.
- Impact: the fork release workflow can now start the reusable Windows release path without failing GitHub's workflow permission validation.
- Verification: the workflow startup error on the `rust-v0.2.2` run was inspected from GitHub Actions and matched the caller-job permission gap fixed in `.github/workflows/rust-release.yml`.
- Files: `.github/workflows/rust-release.yml`, `CHANGELOG.md`, `VERSION`, `codex-rs/Cargo.toml`

## [0.2.2] - 2026-03-25

### Fixed

- What changed: made the GitHub release workflow degrade safely inside the `godex` fork by skipping upstream-only signing, Windows release runners, DotSlash publication, WinGet publication, and latest-alpha branch updates when the repository is not `openai/codex`.
- Why: the first `rust-v0.2.1` tag in the fork hit a GitHub Actions startup failure because the upstream release workflow assumed OpenAI-only Windows signing secrets and runner infrastructure.
- Impact: tagged releases in `LeonSGP43/godex` can now proceed to GitHub Release and npm publication without depending on unavailable upstream-private release resources.
- Verification: `python3 - <<'PY' ... yaml.safe_load(...) ... PY` validated the edited workflow files locally, and the broken `rust-v0.2.1` tag run was traced to the release workflow before this fix.
- Files: `.github/workflows/rust-release.yml`, `.github/workflows/rust-release-windows.yml`, `CHANGELOG.md`, `VERSION`, `codex-rs/Cargo.toml`

## [0.2.1] - 2026-03-25

### Added

- What changed: added two repo-local Codex skills for `godex` maintenance, one to fetch and review official Codex upstream changes into a Markdown decision report and one to inspect/publish fork releases.
- Why: maintaining `godex` as an upstream-first fork needed reusable in-repo automation instead of relying on ad hoc terminal memory for upstream review and release distribution.
- Impact: future maintenance can start from `.codex/skills/godex-upstream-reviewer` and `.codex/skills/godex-release-distributor`, making upstream analysis, release verification, npm readiness checks, and operator handoff more repeatable.
- Verification: `python3 -m py_compile .codex/skills/godex-upstream-reviewer/scripts/godex_upstream_report.py .codex/skills/godex-release-distributor/scripts/godex_release_distributor.py`, `bash .codex/skills/godex-upstream-reviewer/scripts/run.sh --no-fetch --output /tmp/godex-upstream-review-repo-test.md`, and `bash .codex/skills/godex-release-distributor/scripts/run.sh status`.
- Files: `.codex/skills/godex-upstream-reviewer/SKILL.md`, `.codex/skills/godex-upstream-reviewer/scripts/godex_upstream_report.py`, `.codex/skills/godex-upstream-reviewer/scripts/run.sh`, `.codex/skills/godex-release-distributor/SKILL.md`, `.codex/skills/godex-release-distributor/scripts/godex_release_distributor.py`, `.codex/skills/godex-release-distributor/scripts/run.sh`

### Changed

- What changed: tightened both skills so they follow the repository constitution, including `sync/<upstream-sha-or-date>` integration branches and release gating on validated `main`.
- Why: the new automation would be dangerous if it allowed direct upstream merge advice on `main` or hid release gate failures from the operator.
- Impact: the upstream review skill now recommends creating a sync branch before any merge, and the release skill surfaces `release-preflight` status so push and publish flows stay consistent with `AGENTS.md` and `CLAUDE.md`.
- Verification: `bash .codex/skills/godex-upstream-reviewer/scripts/run.sh --no-fetch --output /tmp/godex-upstream-review-repo-test.md && rg -n 'git checkout -b sync/' /tmp/godex-upstream-review-repo-test.md` and `bash .codex/skills/godex-release-distributor/scripts/run.sh status`.
- Files: `.codex/skills/godex-upstream-reviewer/SKILL.md`, `.codex/skills/godex-upstream-reviewer/scripts/godex_upstream_report.py`, `.codex/skills/godex-release-distributor/SKILL.md`, `.codex/skills/godex-release-distributor/scripts/godex_release_distributor.py`

## [0.2.0] - 2026-03-25

### Added

- What changed: added a first-class npm distribution path for the fork with the published package family `@leonsgp43/godex`, `godex-npm-*` release tarballs, fork-owned install scripts, and corrected in-app npm upgrade hints.
- Why: the fork had already renamed the CLI and release governance, but install and update flows still pointed at upstream `@openai/codex`, which made global installs and self-update guidance inconsistent.
- Impact: `godex` can now be installed and upgraded through npm and GitHub release installers without sending users back to the official Codex package, while the underlying native artifact layout stays close to upstream for easier syncs.
- Verification: `python3 scripts/stage_npm_packages.py --release-version 0.2.0 --package codex --output-dir /tmp/godex-npm-stage`, `bash scripts/godex-maintain.sh release-preflight`, and `cargo test -p codex-tui update_prompt -- --nocapture` should validate tarball naming, release metadata, and updated upgrade prompts.
- Files: `codex-cli/package.json`, `codex-cli/bin/codex.js`, `codex-cli/scripts/build_npm_package.py`, `scripts/stage_npm_packages.py`, `scripts/install/install.sh`, `scripts/install/install.ps1`, `.github/workflows/ci.yml`, `.github/workflows/rust-release.yml`, `codex-rs/core/src/branding.rs`, `codex-rs/tui/src/update_action.rs`, `codex-rs/tui_app_server/src/update_action.rs`, `README.md`, `docs/install.md`, `CHANGELOG.md`, `VERSION`, `codex-rs/Cargo.toml`

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
