# godex Fork Inventory Ledger

This ledger is the current-state inventory of how this fork differs from official `openai/codex` at the current comparison point. It is meant to be the working source of truth for future syncs, patch-layer refactors, and upstream replacement decisions.

## Snapshot

- Generated on: `2026-04-07`
- Compared head: `529be4aa63`
- Compared upstream base: `upstream/main` at `89f1a44afa`
- Divergence: `behind 177` / `ahead 69`
- Diff surface: `162 files changed, 17394 insertions(+), 918 deletions(-)`
- Current top hot directories:
  - `codex-rs/core/src`: 42 changed paths
  - `codex-rs/tui/src`: 29 changed paths
  - `codex-rs/rollout/src`: 6 changed paths
  - `codex-rs/state/src`: 5 changed paths
  - `codex-rs/examples/external_agent_backends`: 4 changed paths
  - `codex-rs/login/src`: 4 changed paths
  - `.codex/skills/godex-release-distributor`: 3 changed paths
  - `.codex/skills/godex-upstream-reviewer`: 3 changed paths
  - `codex-rs/cli/src`: 3 changed paths
  - `codex-rs/core/tests`: 3 changed paths

## Reading Rules

- This is relative to the current `upstream/main`, not just the most recent local commits.
- A path in this ledger can be one of three things: a durable fork patch, a temporary compatibility shim, or mixed bootstrap residue that should be shrunk.
- `Owner files` means the files we currently have to consciously protect or revisit during sync; it does not automatically mean the current implementation is the best long-term shape.
- The goal is not to preserve every line forever. The goal is to preserve required behavior while deleting unnecessary divergence whenever upstream catches up.

## Patch Group Summary

| Patch group | Class | Current diff paths | Direction |
| --- | --- | ---: | --- |
| `fork/identity-governance` | durable fork patch | 23 | Keep as a small explicit patch group. Do not let unrelated UI/runtime work accumulate here. |
| `fork/distribution-release` | durable fork patch | 26 | Keep, but isolate version bumps and release metadata from feature commits. |
| `fork/maintenance-automation` | durable fork patch | 8 | Keep. This should become the canonical entrypoint for future sync/release work. |
| `fork/config-namespace-home` | durable fork patch with hot-file overlap | 11 | Keep the policy, but continue extracting fork-specific behavior out of `cli/src/main.rs` and `core/src/config/mod.rs` into dedicated adapters. |
| `fork/provider-backends` | durable fork patch | 21 | Keep. This is the right long-term direction for provider integration work. |
| `fork/native-grok-legacy` | legacy compatibility patch | 0 | Freeze and migrate out. Do not expand this surface further. |
| `fork/memory-system` | durable fork patch with the highest merge cost | 30 | Keep the behavior, but refactor it into a fork patch-layer before the next large upstream sync. |
| `fork/bootstrap-residue` | mixed bootstrap residue | 43 | Shrink aggressively. This is the main non-systematic residue that still inflates merge cost. |

## Identity, branding, and fork governance (`fork/identity-governance`)

- Patch class: durable fork patch
- Purpose: Keep `godex` visibly distinct from official Codex and maintain fork-specific release/update governance.
- Representative commits:
  - `f6f9d17207 feat(godex): publish fork cli and release workflow`
  - `40fb0bac03 fix(tooltips): point announcement source to public fork branch`
  - `e436a9c476 fix(tooltips): load remote announcements on first startup`
  - `fc55a35940 chore(godex): establish fork maintenance constitution`
- Owner files / globs:
  - `README.md`
  - `CHANGELOG.md`
  - `VERSION`
  - `announcement_tip.toml`
  - `docs/godex-*.md`
  - `docs/reports/upstream-review-*.md`
  - `codex-rs/core/src/branding.rs`
  - `codex-rs/tui/src/tooltips.rs`
  - `codex-rs/tui/src/updates.rs`
  - `codex-rs/tui/src/update_action.rs`
  - `codex-rs/tui/src/update_prompt.rs`
- Current verification:
  - `godex --version`
  - `bash scripts/godex-maintain.sh status`
  - `inspect fork announcement/update source wiring`
- Upstream replacement possibility: Low. Branding, release identity, and fork announcement sources are fork-owned by definition.
- Current recommendation: Keep as a small explicit patch group. Do not let unrelated UI/runtime work accumulate here.

## Distribution, install, and release packaging (`fork/distribution-release`)

- Patch class: durable fork patch
- Purpose: Ship `godex` without overwriting `codex`, including npm packaging, install wrappers, and release workflows.
- Representative commits:
  - `dce4881497 feat(distribution): add godex npm install and update flow`
  - `4ef51f44c9 feat(skills): add local-first godex release flow`
  - `56618e580b fix(release): use token fallback for fork npm publish`
  - `763c0657f7 chore(release): bump version to 0.2.17`
- Owner files / globs:
  - `codex-cli/**`
  - `.github/workflows/rust-release*.yml`
  - `scripts/install/**`
  - `scripts/godex-release*.sh`
  - `scripts/stage_npm_packages.py`
  - `codex-rs/Cargo.toml`
  - `codex-rs/Cargo.lock`
  - `codex-rs/cli/Cargo.toml`
  - `codex-rs/README.md`
  - `docs/install.md`
- Current verification:
  - `bash scripts/install/install-godex-from-source.sh --dry-run`
  - `bash scripts/godex-maintain.sh release-preflight`
  - `npm package staging smoke through scripts/godex-release*.sh`
- Upstream replacement possibility: Low to medium. Packaging mechanics may evolve upstream, but fork-specific artifact names and distribution channels remain fork-owned.
- Current recommendation: Keep, but isolate version bumps and release metadata from feature commits.

## Maintenance automation and local maintainer skills (`fork/maintenance-automation`)

- Patch class: durable fork patch
- Purpose: Provide repeatable upstream-sync, release, and operator workflows for maintaining the fork.
- Representative commits:
  - `5a7ad127d8 feat(skills): add godex upstream and release skills`
  - `f77aa8cbbc fix(skills): align godex automation with repo governance`
  - `fc55a35940 chore(godex): establish fork maintenance constitution`
- Owner files / globs:
  - `.codex/config.toml`
  - `.codex/skills/godex-*/**`
  - `scripts/godex-maintain.sh`
- Current verification:
  - `bash scripts/godex-maintain.sh status`
  - `bash scripts/godex-maintain.sh sync --dry-run`
  - `open the skill runbooks and confirm commands still match repo layout`
- Upstream replacement possibility: Low. This is fork-operator machinery, not product behavior.
- Current recommendation: Keep. This should become the canonical entrypoint for future sync/release work.

## Config namespace, home selection, and CLI policy (`fork/config-namespace-home`)

- Patch class: durable fork patch with hot-file overlap
- Purpose: Preserve `godex` default compatibility with `~/.codex` while supporting isolated `godex -g` / `.godex`, plus CLI-level memory scope override.
- Representative commits:
  - `f6f9d17207 feat(godex): publish fork cli and release workflow`
  - `61d8d2f7b2 fix(godex): initialize isolated home on first -g run`
  - `709c379ab2 feat(godex): add cli memory scope override`
- Owner files / globs:
  - `codex-rs/cli/src/main.rs`
  - `codex-rs/cli/tests/godex_home.rs`
  - `codex-rs/core/src/config/**`
  - `codex-rs/core/src/config_loader/**`
  - `codex-rs/core/config.schema.json`
  - `codex-rs/utils/home-dir/src/lib.rs`
  - `docs/config.md`
- Current verification:
  - `cargo test -p codex-cli godex_home -- --nocapture`
  - `godex --memory-scope project --version`
  - `manual smoke with godex and godex -g`
- Upstream replacement possibility: Low for namespace behavior; medium for adjacent config parsing details if upstream adds equivalent hooks.
- Current recommendation: Keep the policy, but continue extracting fork-specific behavior out of `cli/src/main.rs` and `core/src/config/mod.rs` into dedicated adapters.

## External spawned-agent backends (`fork/provider-backends`)

- Patch class: durable fork patch
- Purpose: Add a real external backend seam so spawned agents can bridge to provider runtimes outside the Codex binary.
- Representative commits:
  - `ecb6259ca2 feat(agent): extend spawned-agent backends and roles`
  - `e9eb86a5f8 feat(agent): wire configurable claude_code backend runtime`
  - `2283300cee feat(agent): add external spawned-agent backends`
  - `529be4aa63 feat(agent): add grok and gemini worker samples`
- Owner files / globs:
  - `codex-rs/core/src/agent/**`
  - `codex-rs/core/src/tools/handlers/multi_agents*.rs`
  - `codex-rs/core/src/tools/spec.rs`
  - `docs/agent-roles.md`
  - `docs/external-agent-backends.md`
  - `codex-rs/examples/external_agent_backends/**`
- Current verification:
  - `cargo check -p codex-core --lib`
  - `inspect docs/external-agent-backends.md examples against the [agent_backends.<name>] schema`
  - `spawn-agent smoke against a configured command backend`
- Upstream replacement possibility: Medium. If upstream later ships a stable external backend/plugin model, this patch should collapse toward that native seam.
- Current recommendation: Keep. This is the right long-term direction for provider integration work.
- Notes:
  - This is the real seam for external provider runtimes. It is the correct place for Gemini/Grok/Leonai-style workers.
  - Provider identity should live in backend configuration or backend commands, not in fake role names.

## Legacy native Grok compatibility shim (`fork/native-grok-legacy`)

- Patch class: legacy compatibility patch
- Purpose: Preserve the current built-in Grok surface only as a temporary compatibility lane while external backends take over real provider work.
- Representative commits:
  - `95b66462f9 feat(grok): unify native Grok spawn role and tool`
  - `6bb455b00d docs(godex): mark native grok as legacy patch`
  - `8962680f0f docs(agent): clarify provider roles versus backends`
- Owner files / globs:
  - `codex-rs/core/src/agent/builtins/grok.toml`
  - `codex-rs/core/src/agent/role.rs`
  - `codex-rs/core/src/tools/handlers/grok_research.rs`
  - `codex-rs/core/src/tools/spec.rs`
  - `docs/agent-roles.md`
  - `docs/config.md`
- Current verification:
  - `inspect Grok tool spec/handler registration`
  - `inspect docs for migration language toward backend = "grok_worker"`
- Upstream replacement possibility: High. This should be retired once external backends cover the real Grok path.
- Current recommendation: Freeze and migrate out. Do not expand this surface further.
- Notes:
  - This group intentionally overlaps with the provider-backend surface, but it is tracked separately because its desired end-state is deletion.

## Scoped memory pipeline and hybrid recall (`fork/memory-system`)

- Patch class: durable fork patch with the highest merge cost
- Purpose: Add project-scoped memories, CLI scope override, summary-token control, semantic recall indexing, and the QMD hybrid recall pipeline.
- Representative commits:
  - `d99cb743ed feat(memories): add configurable semantic recall indexing`
  - `d9dbaa029e feat(memories): add qmd hybrid recall pipeline`
  - `d4f8a7ca30 feat(godex): add project-scoped memory mode`
  - `709c379ab2 feat(godex): add cli memory scope override`
- Owner files / globs:
  - `codex-rs/core/src/memories/**`
  - `codex-rs/core/templates/memories/**`
  - `codex-rs/state/src/runtime/memories.rs`
  - `codex-rs/state/src/runtime/threads.rs`
  - `codex-rs/state/src/model/thread_metadata.rs`
  - `codex-rs/rollout/src/**`
  - `codex-rs/state/migrations/0023_threads_memory_scope.sql`
  - `docs/godex-memory-system.md`
- Current verification:
  - `cargo test -p codex-core memories:: -- --nocapture`
  - `cargo test -p codex-core prompts::tests::memory_quick_pass_instructions_remain_stable`
  - `cargo test -p codex-app-server --tests --no-run --manifest-path codex-rs/Cargo.toml`
- Upstream replacement possibility: Medium to high. Upstream may eventually add scoped memories or better recall; the semantic/QMD engine is the most likely patch to become replaceable.
- Current recommendation: Keep the behavior, but refactor it into a fork patch-layer before the next large upstream sync.
- Notes:
  - This is the highest merge-cost patch group because it crosses config, CLI, rollout metadata, state schema/runtime, prompt assembly, and retrieval logic.
  - The follow-up design for shrinking this intrusion is documented in `docs/godex-memory-patch-layer-plan.md`.

## Bootstrap mixed residue and shared hot-path drift (`fork/bootstrap-residue`)

- Patch class: mixed bootstrap residue
- Purpose: Capture files that still differ from upstream mainly because the initial fork-publish commit mixed branding, login/auth, proxy, TUI, and other hot-path edits into one large patch.
- Representative commits:
  - `f6f9d17207 feat(godex): publish fork cli and release workflow`
  - `dce4881497 feat(distribution): add godex npm install and update flow`
- Owner files / globs:
  - `codex-rs/cli/src/login.rs`
  - `codex-rs/cli/src/mcp_cmd.rs`
  - `codex-rs/login/src/**`
  - `codex-rs/core/src/network_proxy_loader.rs`
  - `codex-rs/core/src/mcp_connection_manager.rs`
  - `codex-rs/tui/src/app.rs`
  - `codex-rs/tui/src/history_cell.rs`
  - `codex-rs/tui/src/status/**`
- Current verification:
  - `manual diff review during sync branches`
  - `targeted TUI/login smoke after each upstream merge`
- Upstream replacement possibility: High. Most of this should either migrate into thinner adapters or disappear once upstream sync catches up.
- Current recommendation: Shrink aggressively. This is the main non-systematic residue that still inflates merge cost.
- Notes:
  - The initial `feat(godex): publish fork cli and release workflow` commit mixed too many concerns into one patch. This ledger treats that mixed residue as first-class debt rather than as a good pattern to preserve.

## Complete Current Diff Inventory By Primary Group

Shared hot files can be mentioned in multiple detailed patch groups above, but in the inventory below each file appears exactly once under its primary owner group for sync review.

### Identity, branding, and fork governance (23 paths)

- `.github/godex-readme-hero.jpg`
- `AGENTS.md`
- `CHANGELOG.md`
- `CLAUDE.md`
- `README.md`
- `VERSION`
- `announcement_tip.toml`
- `codex-rs/core/src/branding.rs`
- `codex-rs/tui/src/tooltips.rs`
- `codex-rs/tui/src/update_action.rs`
- `codex-rs/tui/src/update_prompt.rs`
- `codex-rs/tui/src/updates.rs`
- `docs/godex-development-plan.md`
- `docs/godex-fork-guidelines.md`
- `docs/godex-fork-manifest.md`
- `docs/godex-maintenance.md`
- `docs/godex-memory-system.md`
- `docs/godex-release-0.1.1.md`
- `docs/godex-release-0.2.0.md`
- `docs/godex-release-0.2.12.md`
- `docs/godex-release-0.2.13.md`
- `docs/godex-release-0.2.6.md`
- `docs/reports/upstream-review-2026-04-02.md`

### Distribution, install, and release packaging (26 paths)

- `.github/workflows/ci.yml`
- `.github/workflows/rust-release-windows.yml`
- `.github/workflows/rust-release-zsh.yml`
- `.github/workflows/rust-release.yml`
- `.gitignore`
- `codex-cli/README.md`
- `codex-cli/bin/codex.js`
- `codex-cli/package.json`
- `codex-cli/scripts/README.md`
- `codex-cli/scripts/build_npm_package.py`
- `codex-cli/scripts/install_native_deps.py`
- `codex-cli/scripts/test_install_native_deps.py`
- `codex-rs/Cargo.lock`
- `codex-rs/Cargo.toml`
- `codex-rs/README.md`
- `codex-rs/arg0/src/lib.rs`
- `codex-rs/cli/Cargo.toml`
- `docs/install.md`
- `scripts/godex-release-local.sh`
- `scripts/godex-release-remote.sh`
- `scripts/godex-release.sh`
- `scripts/install/install-godex-from-source.sh`
- `scripts/install/install.ps1`
- `scripts/install/install.sh`
- `scripts/stage_npm_packages.py`
- `tools/argument-comment-lint/README.md`

### Maintenance automation and local maintainer skills (8 paths)

- `.codex/config.toml`
- `.codex/skills/godex-release-distributor/SKILL.md`
- `.codex/skills/godex-release-distributor/scripts/godex_release_distributor.py`
- `.codex/skills/godex-release-distributor/scripts/run.sh`
- `.codex/skills/godex-upstream-reviewer/SKILL.md`
- `.codex/skills/godex-upstream-reviewer/scripts/godex_upstream_report.py`
- `.codex/skills/godex-upstream-reviewer/scripts/run.sh`
- `scripts/godex-maintain.sh`

### Config namespace, home selection, and CLI policy (11 paths)

- `codex-rs/cli/src/main.rs`
- `codex-rs/cli/tests/godex_home.rs`
- `codex-rs/core/config.schema.json`
- `codex-rs/core/src/config/config_tests.rs`
- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/config/service.rs`
- `codex-rs/core/src/config/types.rs`
- `codex-rs/core/src/config_loader/mod.rs`
- `codex-rs/core/src/config_loader/tests.rs`
- `codex-rs/utils/home-dir/src/lib.rs`
- `docs/config.md`

### External spawned-agent backends (21 paths)

- `codex-rs/core/src/agent/backend.rs`
- `codex-rs/core/src/agent/builtins/claude-style.toml`
- `codex-rs/core/src/agent/builtins/grok.toml`
- `codex-rs/core/src/agent/control.rs`
- `codex-rs/core/src/agent/control_tests.rs`
- `codex-rs/core/src/agent/mod.rs`
- `codex-rs/core/src/agent/role.rs`
- `codex-rs/core/src/agent/role_tests.rs`
- `codex-rs/core/src/codex_thread.rs`
- `codex-rs/core/src/tools/handlers/grok_research.rs`
- `codex-rs/core/src/tools/handlers/mod.rs`
- `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs`
- `codex-rs/core/src/tools/handlers/multi_agents_common.rs`
- `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs`
- `codex-rs/core/src/tools/spec.rs`
- `codex-rs/examples/external_agent_backends/python_grok_responses_v1/README.md`
- `codex-rs/examples/external_agent_backends/python_grok_responses_v1/backend.py`
- `codex-rs/examples/external_agent_backends/python_json_stdio_v1/README.md`
- `codex-rs/examples/external_agent_backends/python_json_stdio_v1/backend.py`
- `docs/agent-roles.md`
- `docs/external-agent-backends.md`

### Legacy native Grok compatibility shim (0 primary paths; overlap-only)

- This group is tracked as an overlap-only lifecycle group. Its files are primarily listed under `fork/provider-backends` and `fork/config-namespace-home`.


### Scoped memory pipeline and hybrid recall (30 paths)

- `codex-rs/app-server/tests/common/rollout.rs`
- `codex-rs/app-server/tests/suite/v2/thread_resume.rs`
- `codex-rs/core/src/memories/README.md`
- `codex-rs/core/src/memories/mod.rs`
- `codex-rs/core/src/memories/phase1.rs`
- `codex-rs/core/src/memories/phase2.rs`
- `codex-rs/core/src/memories/prompts.rs`
- `codex-rs/core/src/memories/prompts_tests.rs`
- `codex-rs/core/src/memories/scope.rs`
- `codex-rs/core/src/memories/semantic_index.rs`
- `codex-rs/core/src/memories/storage.rs`
- `codex-rs/core/src/memories/tests.rs`
- `codex-rs/core/src/memories/usage.rs`
- `codex-rs/core/src/rollout.rs`
- `codex-rs/core/templates/memories/consolidation.md`
- `codex-rs/core/templates/memories/read_path.md`
- `codex-rs/core/tests/memory_scope_smoke.rs`
- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/rollout/src/config.rs`
- `codex-rs/rollout/src/metadata.rs`
- `codex-rs/rollout/src/metadata_tests.rs`
- `codex-rs/rollout/src/recorder.rs`
- `codex-rs/rollout/src/recorder_tests.rs`
- `codex-rs/rollout/src/tests.rs`
- `codex-rs/state/migrations/0023_threads_memory_scope.sql`
- `codex-rs/state/src/extract.rs`
- `codex-rs/state/src/model/thread_metadata.rs`
- `codex-rs/state/src/runtime/memories.rs`
- `codex-rs/state/src/runtime/test_support.rs`
- `codex-rs/state/src/runtime/threads.rs`

### Bootstrap mixed residue and shared hot-path drift (43 paths)

- `codex-rs/app-server/src/codex_message_processor.rs`
- `codex-rs/cli/src/login.rs`
- `codex-rs/cli/src/mcp_cmd.rs`
- `codex-rs/cloud-tasks/src/lib.rs`
- `codex-rs/core/src/codex.rs`
- `codex-rs/core/src/codex_tests.rs`
- `codex-rs/core/src/lib.rs`
- `codex-rs/core/src/mcp_connection_manager.rs`
- `codex-rs/core/src/network_proxy_loader.rs`
- `codex-rs/core/src/personality_migration_tests.rs`
- `codex-rs/core/src/realtime_context_tests.rs`
- `codex-rs/core/src/util.rs`
- `codex-rs/core/tests/suite/personality_migration.rs`
- `codex-rs/core/tests/suite/sqlite_state.rs`
- `codex-rs/login/src/assets/error.html`
- `codex-rs/login/src/assets/success.html`
- `codex-rs/login/src/device_code_auth.rs`
- `codex-rs/login/src/server.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/bottom_pane/feedback_view.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/chatwidget/snapshots/codex_tui__chatwidget__tests__personality_selection_popup.snap`
- `codex-rs/tui/src/cli.rs`
- `codex-rs/tui/src/history_cell.rs`
- `codex-rs/tui/src/lib.rs`
- `codex-rs/tui/src/onboarding/auth.rs`
- `codex-rs/tui/src/onboarding/welcome.rs`
- `codex-rs/tui/src/slash_command.rs`
- `codex-rs/tui/src/snapshots/codex_tui__app__tests__clear_ui_after_long_transcript_fresh_header_only.snap`
- `codex-rs/tui/src/snapshots/codex_tui__app__tests__clear_ui_header_fast_status_gpt54_only.snap`
- `codex-rs/tui/src/snapshots/codex_tui__history_cell__tests__session_info_availability_nux_tooltip_snapshot.snap`
- `codex-rs/tui/src/snapshots/codex_tui__update_prompt__tests__update_prompt_modal.snap`
- `codex-rs/tui/src/snapshots/codex_tui_app_server__update_prompt__tests__update_prompt_modal.snap`
- `codex-rs/tui/src/status/card.rs`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_cached_limits_hide_credits_without_flag.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_includes_credits_and_limits.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_includes_forked_from.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_includes_monthly_limit.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_includes_reasoning_details.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_empty_limits_message.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_missing_limits_message.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_stale_limits_message.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_truncates_in_narrow_terminal.snap`

## Immediate Governance Conclusions

- Keep: `fork/provider-backends`, `fork/config-namespace-home`, `fork/identity-governance`, `fork/distribution-release`, and `fork/maintenance-automation`. These define the fork on purpose.
- Freeze then migrate out: `fork/native-grok-legacy`. It should not receive new product work.
- Refactor next: `fork/memory-system`. The behavior should stay, but the implementation should move behind a thinner patch-layer.
- Shrink aggressively: `fork/bootstrap-residue`. This is the biggest source of avoidable future merge pain.
- Future best practice: every new fork feature should land in a named patch group first, then in a narrow module tree, and only then touch hot upstream files through thin adapters.

