# godex Fork Inventory Ledger

This ledger is the current-state inventory of how this fork differs from official `openai/codex` at the current comparison point. It is meant to be the working source of truth for future syncs, patch-layer refactors, and upstream replacement decisions.

## Snapshot

- Generated on: `2026-04-08`
- Compared head: `1c2b2c1b2f`
- Compared upstream base: `upstream/main` at `d47b755aa2`
- Divergence: `behind 212` / `ahead 97`
- Diff surface: `169 files changed, 18565 insertions(+), 1163 deletions(-)`
- Current top hot directories:
  - `codex-rs/core/src`: 44 changed paths
  - `codex-rs/tui/src`: 29 changed paths
  - `codex-rs/state/src`: 8 changed paths
  - `codex-rs/rollout/src`: 6 changed paths
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
  - `ee670278a0 refactor(config): extract home namespace policy adapter`
- Owner files / globs:
  - `codex-rs/cli/src/main.rs`
  - `codex-rs/cli/tests/godex_home.rs`
  - `codex-rs/core/src/config/**`
  - `codex-rs/core/src/config_loader/**`
  - `codex-rs/core/config.schema.json`
  - `codex-rs/utils/home-dir/src/lib.rs`
  - `docs/config.md`
- Current verification:
  - `cargo test -p codex-cli --test godex_home --manifest-path codex-rs/Cargo.toml -- --nocapture`
  - `cargo test -p codex-core home_policy --manifest-path codex-rs/Cargo.toml -- --nocapture`
  - `godex --memory-scope project --version`
  - `manual smoke with godex and godex -g`
- Upstream replacement possibility: Low for namespace behavior; medium for adjacent config parsing details if upstream adds equivalent hooks.
- Current recommendation: Keep the policy, but continue extracting fork-specific behavior out of `cli/src/main.rs` and `core/src/config/mod.rs` into dedicated adapters.
- Notes:
  - `ee670278a0` advanced `patch/config-home-namespace` by moving namespace selection, default-home inference, isolated-home bootstrap, and config-home resolution behind `core/src/config/home_policy.rs`, reducing fork-policy ownership in both `cli/src/main.rs` and `core/src/config/mod.rs`.

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
  - `0f918e28c5` advanced `patch/backend-contract` by centralizing spawn-time backend resolution, backend id normalization, and backend-specific model override selection in `multi_agents_common.rs`, removing duplicated policy from both `multi_agents/spawn.rs` and `multi_agents_v2/spawn.rs`.
  - `f46eb549bd` advanced `patch/backend-contract` again by moving spawned-agent backend resolution, archived backend config restore, and handle construction behind helpers in `core/src/agent/backend.rs`, thinning backend-specific glue in `core/src/agent/control.rs`.
  - Targeted verification for that cut:
    - `cargo test -p codex-core spawn_agent_uses_explorer_role_and_preserves_approval_policy --manifest-path codex-rs/Cargo.toml -- --nocapture`
    - `cargo test -p codex-core spawn_agent_with_command_backend --manifest-path codex-rs/Cargo.toml -- --nocapture`
  - Targeted verification for the latest cut:
    - `cargo test -p codex-core command_backend_spawn_wait_and_close_round_trip --manifest-path codex-rs/Cargo.toml -- --nocapture`
    - `cargo test -p codex-core close_agent_persists_closed_edge_for_claude_code_backend --manifest-path codex-rs/Cargo.toml -- --nocapture`
    - `cargo test -p codex-core resume_thread_subagent_restores_stored_nickname_and_role --manifest-path codex-rs/Cargo.toml -- --nocapture`

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
  - `codex-rs/core/src/fork_patch/memory.rs`
  - `codex-rs/core/src/fork_patch/mod.rs`
  - `codex-rs/core/templates/memories/**`
  - `codex-rs/state/src/fork_patch/**`
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
  - Progress since the previous ledger snapshot:
    - `ce26159c05` and `9695d4fc05` advanced `patch/memory-facade` and `patch/memory-artifact-contract` by moving scope/artifact-root helpers and artifact path helpers behind `fork_patch::memory`.
    - `ce89803488` advanced `patch/memory-artifact-contract` by moving empty-consolidation artifact cleanup target ownership out of `core/src/memories/storage.rs` and into `core/src/fork_patch/memory.rs`.
    - `dc682e8edf` advanced `patch/memory-artifact-contract` by moving rollout-summary file-name and relative-path naming behind `core/src/fork_patch/memory.rs`, reducing repeated path formatting in `storage.rs`, `prompts.rs`, and `semantic_index.rs`.
    - `1710630a84` advanced `patch/memory-artifact-contract` again by moving rollout-summary `.md` suffix parsing behind `core/src/fork_patch/memory.rs`, reducing suffix-policy ownership in `storage.rs` and aligning targeted tests with the facade.
    - `1c2b2c1b2f` advanced `patch/memory-artifact-contract` again by moving semantic-index `vector_index.json` metadata naming behind `core/src/fork_patch/memory.rs`; the audit for this cut confirmed `memory_index.qmd` file-path generation was already centralized through the facade.
    - `59a7ab0b15` and `cc2dfc1341` advanced `patch/memory-read-path` by moving read-path helper logic into the facade and removing a leftover wrapper from `prompts.rs`.
    - `11941f87e6` and `658efc8c03` advanced `patch/memory-state-runtime` by moving scope helpers into `state/src/fork_patch/memory_repo.rs` and centralizing phase2 enqueue scope fetches.
    - `cfa344646c` advanced `patch/memory-state-runtime` again by moving duplicated thread scope persistence binding out of `state/src/runtime/threads.rs` and into `state/src/fork_patch/memory_repo.rs`.
    - `c16b1e033f` advanced `patch/memory-state-runtime` by centralizing repeated scope-query binding from `state/src/runtime/memories.rs` into `state/src/fork_patch/memory_repo.rs`.
    - `b040468cca` advanced `patch/memory-state-runtime` by centralizing repeated phase2 job-key binding from `state/src/runtime/memories.rs` into `state/src/fork_patch/memory_repo.rs`.
    - `e3ba29987e` advanced `patch/memory-state-runtime` by centralizing phase2 selection-state queries from `state/src/runtime/memories.rs` into `state/src/fork_patch/memory_repo.rs`.
    - `5b3b550614` advanced `patch/memory-state-runtime` by moving phase2 enqueue helpers, including thread-to-scope enqueue glue, out of `state/src/runtime/memories.rs` and into `state/src/fork_patch/memory_repo.rs`.
  - Latest verification snapshot:
    - `cargo test -p codex-state --lib --manifest-path codex-rs/Cargo.toml` passed with `84 passed; 0 failed`.
    - `cargo test -p codex-app-server --tests --no-run --manifest-path codex-rs/Cargo.toml` passed after `1547c00c83 fix(app-server): restore thread config snapshot test`, after `c16b1e033f refactor(memory): extract runtime scope query binding`, after `b040468cca refactor(memory): extract phase2 job key binding`, after `e3ba29987e refactor(memory): extract phase2 selection queries`, and after `5b3b550614 refactor(memory): extract phase2 enqueue helpers`.
    - `cargo test -p codex-core 'memories::tests::phase2::dispatch_with_empty_stage1_outputs_rebuilds_local_artifacts' --manifest-path codex-rs/Cargo.toml -- --exact --nocapture` passed after `ce89803488 refactor(memory): extract artifact cleanup targets`.
    - `cargo test -p codex-core 'memories::tests::rebuild_raw_memories_file_adds_canonical_rollout_summary_file_header' --manifest-path codex-rs/Cargo.toml -- --exact --nocapture` passed after `1710630a84 refactor(memory): extract rollout summary suffix contract`.
    - `cargo test -p codex-core 'memories::prompts::tests::build_consolidation_prompt_renders_embedded_template' --manifest-path codex-rs/Cargo.toml -- --exact --nocapture` passed after `dc682e8edf refactor(memory): extract rollout summary path contract`.
    - `cargo test -p codex-core 'memories::tests::sync_rollout_summaries_and_raw_memories_file_keeps_latest_memories_only' --manifest-path codex-rs/Cargo.toml -- --exact --nocapture` passed after `1c2b2c1b2f refactor(memory): extract vector index metadata contract`.
    - `cargo test -p codex-core 'memories::tests::write_memory_index_qmd_empty_uses_canonical_vector_index_file_name' --manifest-path codex-rs/Cargo.toml -- --exact --nocapture` passed after `1c2b2c1b2f refactor(memory): extract vector index metadata contract`.

### `patch/memory-state-runtime` file-level ledger

| File | Patch role now | Completed extractions | Remaining safe extractions | Not recommended now |
| --- | --- | --- | --- | --- |
| `codex-rs/state/src/fork_patch/memory_repo.rs` | Fork-owned scope and memory-state query seam | thread scope binding, runtime scope query binding, thread scope fetch, phase2 job-key binding, phase2 selection-state queries, phase2 enqueue helpers | thin helpers for any future leftover scope lookup or query binding glue | moving the stage1/phase2 runtime state machine itself |
| `codex-rs/state/src/runtime/memories.rs` | Upstream-hot runtime owner for job lifecycle and selection semantics | hot-path glue is materially reduced around scope binding, phase2 job-key binding, phase2 selection-state queries, and phase2 enqueue glue | none currently required for this patch subgroup; reopen only if later refactors expose a new fork-specific glue seam | broad abstractions like `MemoryScopeRef`, generic job adapters, or moving SQL state transitions wholesale |
| `codex-rs/state/src/runtime/threads.rs` | Thread metadata write path | thread scope persistence helper already extracted | little to none unless new fork-only scope writes appear | generic thread lifecycle refactors for patch-layer symmetry |
| `codex-rs/state/src/model/thread_metadata.rs` | Metadata contract seam | keeps required memory-scope contract fields | little to none until upstream changes the contract | metadata churn without an upstream-compatibility reason |

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

## Patch-Layer Decomposition Ledger

This section is the higher-resolution ledger for future fork-patch work. It maps the current diff to narrower patch groups that can each be carried, validated, migrated, or deleted independently during upstream sync.

| Patch subgroup | Parent group | Current role | Primary owner files / seams | Verification | Upstream replacement trigger |
| --- | --- | --- | --- | --- | --- |
| `patch/backend-contract` | `fork/provider-backends` | Define the external spawned-agent backend contract, lifecycle, and config seam. | `codex-rs/core/src/agent/backend.rs`, `codex-rs/core/src/agent/mod.rs`, `codex-rs/core/src/agent/control.rs`, `codex-rs/core/src/tools/handlers/multi_agents*.rs`, `codex-rs/core/src/tools/spec.rs` | `cargo check -p codex-core --lib`; spawn-agent smoke with a configured external backend | Upstream lands a stable external backend/plugin seam that can express command/json-stdio backends without fork-only types |
| `patch/backend-builtins` | `fork/provider-backends` | Keep fork-owned role metadata thin and backend-oriented instead of provider-role impersonation. | `codex-rs/core/src/agent/builtins/claude-style.toml`, `codex-rs/core/src/agent/role.rs`, `codex-rs/core/src/agent/role_tests.rs`, `docs/agent-roles.md` | inspect built-in roles against backend docs; `cargo test -p codex-core role --manifest-path codex-rs/Cargo.toml` | Upstream adds equivalent role-extension metadata or the fork migrates all provider identity to pure config |
| `patch/backend-examples` | `fork/provider-backends` | Ship runnable sample backends and operator docs so provider workers live outside the binary. | `codex-rs/examples/external_agent_backends/**`, `docs/external-agent-backends.md` | readme/example parity review; sample backend smoke against `[agent_backends.*]` | Upstream publishes first-party external backend examples or the fork moves examples to a separate maintainer repo |
| `patch/backend-legacy-grok-shim` | `fork/native-grok-legacy` | Temporary compatibility layer for native Grok naming/tooling while real provider calls move to external backends. | `codex-rs/core/src/agent/builtins/grok.toml`, `codex-rs/core/src/tools/handlers/grok_research.rs`, `docs/config.md` | inspect registration and migration docs | External `grok_worker` is the only supported real Grok path and native shim usage falls to zero |
| `patch/memory-facade` | `fork/memory-system` | Thin fork seam that gathers memory-only policy behind `fork_patch::memory`. | `codex-rs/core/src/fork_patch/memory.rs`, `codex-rs/core/src/fork_patch/mod.rs` | `cargo check -p codex-core --lib`; targeted prompt/storage tests | Once all remaining fork-only memory policy has moved behind the facade and hot upstream call sites only call the facade |
| `patch/memory-artifact-contract` | `fork/memory-system` | Centralize memory artifact path naming, layout rules, cleanup target ownership, rollout-summary file/path contract, rollout-summary suffix parsing, and semantic-index vector metadata naming. | `codex-rs/core/src/fork_patch/memory.rs`, `codex-rs/core/src/memories/mod.rs`, `codex-rs/core/src/memories/storage.rs`, `codex-rs/core/src/memories/semantic_index.rs`, `codex-rs/core/src/memories/prompts.rs`, `codex-rs/core/src/memories/tests.rs`, `codex-rs/core/src/memories/prompts_tests.rs` | `cargo test -p codex-core 'memories::prompts::tests::build_consolidation_prompt_renders_embedded_template' --manifest-path codex-rs/Cargo.toml -- --exact --nocapture`; `cargo test -p codex-core 'memories::tests::sync_rollout_summaries_and_raw_memories_file_keeps_latest_memories_only' --manifest-path codex-rs/Cargo.toml -- --exact --nocapture`; `cargo test -p codex-core 'memories::tests::rebuild_raw_memories_file_adds_canonical_rollout_summary_file_header' --manifest-path codex-rs/Cargo.toml -- --exact --nocapture`; `cargo test -p codex-core 'memories::tests::write_memory_index_qmd_empty_uses_canonical_vector_index_file_name' --manifest-path codex-rs/Cargo.toml -- --exact --nocapture` | Artifact names/locations become upstream-native or fully encapsulated behind a stable memory adapter |
| `patch/memory-scope-policy` | `fork/memory-system` | Own global vs project scope selection, CLI override policy, and root resolution. | `codex-rs/core/src/memories/scope.rs`, `codex-rs/cli/src/main.rs`, `codex-rs/core/src/config/**`, `codex-rs/core/tests/memory_scope_smoke.rs`, `codex-rs/cli/tests/godex_home.rs` | `cargo test -p codex-cli godex_home -- --nocapture`; `cargo test -p codex-core memory_scope_smoke --manifest-path codex-rs/Cargo.toml` | Upstream ships equivalent scoped-memory semantics with compatible operator controls |
| `patch/memory-read-path` | `fork/memory-system` | Control quick-pass instructions, summary embedding, and recall hint assembly. | `codex-rs/core/src/fork_patch/memory.rs`, `codex-rs/core/src/memories/prompts.rs`, `codex-rs/core/src/memories/prompts_tests.rs`, `codex-rs/core/templates/memories/read_path.md` | `cargo test -p codex-core prompts::tests::memory_quick_pass_instructions_remain_stable --manifest-path codex-rs/Cargo.toml` | Upstream adds comparable prompt/read-path behavior or the fork can express the same policy purely as data/templates |
| `patch/memory-recall-engine` | `fork/memory-system` | Own semantic index, QMD export, hybrid retrieval, and recall tuning. | `codex-rs/core/src/memories/semantic_index.rs`, `codex-rs/core/src/memories/usage.rs`, `codex-rs/core/src/memories/tests.rs`, `codex-rs/core/templates/memories/consolidation.md` | `cargo test -p codex-core memories:: -- --nocapture`; semantic recall fixture tests | Upstream introduces a better recall/indexing engine and the fork can delete or delegate the QMD/vector implementation |
| `patch/memory-state-runtime` | `fork/memory-system` | Persist scope metadata and thread/runtime hooks needed by scoped memories. | `codex-rs/state/migrations/0023_threads_memory_scope.sql`, `codex-rs/state/src/fork_patch/mod.rs`, `codex-rs/state/src/fork_patch/memory_repo.rs`, `codex-rs/state/src/model/thread_metadata.rs`, `codex-rs/state/src/runtime/memories.rs`, `codex-rs/state/src/runtime/threads.rs`, `codex-rs/rollout/src/**`, `codex-rs/protocol/src/protocol.rs` | `cargo test -p codex-app-server --tests --no-run --manifest-path codex-rs/Cargo.toml`; rollout/state targeted suites | Upstream state/runtime grows a compatible memory-scope model and the fork can map onto native metadata |
| `patch/config-home-namespace` | `fork/config-namespace-home` | Keep `godex`/`godex -g` home behavior, config namespace policy, and schema exposure coherent. | `codex-rs/cli/src/main.rs`, `codex-rs/cli/tests/godex_home.rs`, `codex-rs/core/src/config/home_policy.rs`, `codex-rs/core/src/config/mod.rs`, `codex-rs/core/src/config_loader/**`, `codex-rs/utils/home-dir/src/lib.rs`, `codex-rs/core/config.schema.json`, `docs/config.md` | `cargo test -p codex-cli --test godex_home --manifest-path codex-rs/Cargo.toml -- --nocapture`; `cargo test -p codex-core home_policy --manifest-path codex-rs/Cargo.toml -- --nocapture`; `godex --memory-scope project --version` | Upstream exposes the same namespace/home policy hooks or the fork splits this into a dedicated wrapper crate |
| `patch/release-distribution` | `fork/distribution-release` | Own fork package names, installers, npm staging, and release workflows. | `codex-cli/**`, `scripts/install/**`, `scripts/godex-release*.sh`, `.github/workflows/rust-release*.yml`, `codex-rs/Cargo.toml`, `codex-rs/Cargo.lock`, `VERSION`, `CHANGELOG.md` | `bash scripts/godex-maintain.sh release-preflight`; install dry-run; version/changelog gate | Only partial replacement is possible; artifact naming and release channels stay fork-owned |
| `patch/bootstrap-login-auth` | `fork/bootstrap-residue` | Residual auth/login divergence that should either migrate into a thinner adapter or disappear. The onboarding copy layer now starts in `codex-rs/tui/src/onboarding/bootstrap_copy.rs`, and CLI login copy now starts in `codex-rs/cli/src/login_copy.rs` instead of staying inline in hot widgets/commands. | `codex-rs/cli/src/login.rs`, `codex-rs/cli/src/login_copy.rs`, `codex-rs/login/src/**`, `codex-rs/tui/src/onboarding/bootstrap_copy.rs`, `codex-rs/tui/src/onboarding/**` | `cargo check -p codex-cli --manifest-path codex-rs/Cargo.toml`; `cargo test -p codex-tui welcome_ --manifest-path codex-rs/Cargo.toml -- --nocapture`; `cargo test -p codex-tui cancel_active_attempt_notifies_device_code_login --manifest-path codex-rs/Cargo.toml -- --nocapture`; targeted login smoke after sync; diff review around auth entrypoints | Upstream closes the behavior gap or fork-specific auth UX moves to isolated adapters |
| `patch/bootstrap-runtime-ui` | `fork/bootstrap-residue` | Residual TUI/runtime presentation drift not yet grouped into a durable fork patch. | `codex-rs/tui/src/app.rs`, `codex-rs/tui/src/history_cell.rs`, `codex-rs/tui/src/status/**`, `codex-rs/tui/src/slash_command.rs`, related snapshots | targeted TUI snapshot review and smoke after each sync | Fork either deletes the drift or promotes a subset into a named durable patch group |
| `patch/bootstrap-proxy-mcp` | `fork/bootstrap-residue` | Residual proxy/MCP hot-path changes that still sit directly in upstream-heavy files. | `codex-rs/core/src/network_proxy_loader.rs`, `codex-rs/core/src/mcp_connection_manager.rs`, `codex-rs/cli/src/mcp_cmd.rs` | targeted smoke for proxy + MCP flows; sync diff review | Upstream adds equivalent hooks or fork-specific behavior moves to dedicated adapters |

### Immediate Extraction Order

- `1. patch/memory-artifact-contract`: keep moving residual path/layout rules into `fork_patch::memory`, but only when a hot-path file still owns fork-specific naming policy.
- `2. patch/memory-read-path`: treat the current facade move as stabilization work; only reopen this lane if summary/semantic hint assembly leaks back into hot upstream files.
- `3. patch/bootstrap-proxy-mcp` and `patch/bootstrap-login-auth`: split residue into explicit adapters or delete it where upstream already covers the behavior.
- `4. patch/backend-contract`: keep growing only the external backend seam; do not reintroduce fake provider roles into the role layer.
- `5. patch/memory-state-runtime`: treat this lane as complete for now; only reopen if later refactors expose new fork-specific runtime glue.

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
