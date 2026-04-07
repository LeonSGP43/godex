# godex Memory Patch-Layer Plan

This document defines how to shrink the current memory-system intrusion so the fork can keep its required behavior while making future upstream syncs materially easier.

## Problem Statement

The current memory work is functional, but it is not yet shaped like a long-lived fork patch. Today the behavior crosses too many upstream hot paths at once:

- config surface: `codex-rs/core/src/config/types.rs`, `codex-rs/core/src/config/mod.rs`, `codex-rs/core/config.schema.json`
- CLI activation surface: `codex-rs/cli/src/main.rs`
- startup pipeline surface: `codex-rs/core/src/memories/phase1.rs`, `codex-rs/core/src/memories/phase2.rs`, `codex-rs/core/src/memories/storage.rs`
- read-path prompt surface: `codex-rs/core/src/codex.rs`, `codex-rs/core/src/memories/prompts.rs`, `codex-rs/core/templates/memories/read_path.md`
- retrieval/index surface: `codex-rs/core/src/memories/semantic_index.rs`
- rollout/state persistence surface: `codex-rs/rollout/src/config.rs`, `codex-rs/rollout/src/metadata.rs`, `codex-rs/state/src/runtime/memories.rs`, `codex-rs/state/src/runtime/threads.rs`, `codex-rs/state/src/model/thread_metadata.rs`, `codex-rs/state/migrations/0023_threads_memory_scope.sql`

That means memory is currently one of the largest merge-cost multipliers in the fork. The behavior is valuable, but the shape is not yet good enough.

## Target Architecture

The memory system should move toward a three-layer fork architecture:

### Layer 1: Thin upstream hook layer

- Keep only minimal call sites inside upstream-hot files.
- Those call sites should delegate into one fork-owned facade instead of embedding policy directly.
- Hot files should mostly do argument passing, not memory-specific decision-making.

Primary target hooks:
- `codex-rs/cli/src/main.rs`: parse `--memory-scope`, then hand off to a fork-memory override resolver.
- `codex-rs/core/src/config/mod.rs`: deserialize config, then call a fork-memory validation/defaulting helper.
- `codex-rs/core/src/codex.rs`: call a single `build_memory_context_fragment(...)` facade instead of assembling summary/semantic logic inline.
- `codex-rs/state/src/runtime/memories.rs`: call a smaller fork-memory repository/policy unit for scoped selection and ranking rules.

### Layer 2: Fork-owned memory patch facade

Create a dedicated fork patch namespace so future syncs can see the custom memory behavior in one place. Recommended layout:

```text
codex-rs/core/src/fork_patch/memory/
  mod.rs
  config.rs             # memory-specific config validation, defaults, clamps
  scope_binding.rs      # cwd -> scope key/root resolution and CLI override merge
  read_path.rs          # summary + semantic recall assembly facade
  recall.rs             # semantic recall orchestration
  artifact_contract.rs  # root path contract and file naming
  migration_flags.rs    # compatibility toggles / deprecation guards

codex-rs/state/src/fork_patch/
  memory_repo.rs        # scoped SQL selection helpers and snapshot bookkeeping

codex-rs/rollout/src/fork_patch/
  memory_metadata.rs    # rollout metadata projection / binding helpers
```

Why this matters: the fork-specific logic becomes visibly separate from upstream-owned modules, even if the thin hook layer still lives in upstream paths.

### Layer 3: Fork memory engine/adapters

- Keep the semantic/QMD engine and scope-specific selection logic behind the fork patch facade.
- Treat state DB access, artifact filesystem access, and prompt-assembly as adapters behind that facade.
- Avoid spreading memory rules across config, rollout, state, and prompt files independently.

## What To Extract From Hot Paths First

| Current hot file | What is currently embedded there | What should remain | What should move out |
| --- | --- | --- | --- |
| `codex-rs/cli/src/main.rs` | CLI override parsing plus memory-scope policy wiring | argument parsing and handoff | scope override merge + defaulting logic |
| `codex-rs/core/src/config/mod.rs` | memory config normalization and schema-facing validation | generic config loading | memory-specific validation/clamps/default rules |
| `codex-rs/core/src/codex.rs` | developer-instruction memory injection orchestration | a single facade call | summary truncation + semantic recall assembly |
| `codex-rs/state/src/runtime/memories.rs` | selection policy, snapshot bookkeeping, and scope-aware phase2 rules | SQL plumbing entrypoints | policy/ranking helpers and scope-specific selection logic |
| `codex-rs/rollout/src/config.rs` / `metadata.rs` | memory scope metadata projection | metadata storage contract | derivation/binding helpers |

## Proposed Patch Split Inside Memory

The current memory implementation should be split into smaller fork patch groups instead of remaining one large blob:

- `memory/contract`: config keys, CLI override shape, rollout metadata shape, DB schema columns.
- `memory/scope-policy`: project/global scope resolution, artifact-root contract, and scope-key binding.
- `memory/pipeline`: Phase 1 / Phase 2 orchestration and selected-input rules.
- `memory/recall-engine`: semantic index + QMD hybrid ranking + read-path recall fragment.
- `memory/docs-and-tests`: docs, smoke tests, and acceptance checks.

This split makes it easier to delete or replace one part when upstream catches up without reworking the entire memory lane.

## Upstream Replacement Strategy

For every memory sub-group, define the deletion trigger now:

- If upstream adds project-scoped memory roots or thread partitioning, delete the fork scope-policy layer first and map old data forward.
- If upstream adds a good native recall/indexing pipeline, delete `semantic_index.rs`-style custom ranking before touching the rest of memory.
- If upstream adds a native CLI/config override equivalent to `--memory-scope`, collapse the fork CLI/config hooks into upstream behavior.
- If upstream adds a better persistence contract for memory metadata, migrate the fork-specific DB/repository adapter and retire custom fields where possible.

The rule is: replace the smallest patch group first, not the entire memory system in one risky rewrite.

## Implementation Roadmap

Recommended atomic refactor sequence:

1. Create `fork_patch/memory` facades with no behavior change; only move call-site wiring behind one interface.
2. Move scope resolution and artifact-root rules into `scope_binding.rs` and `artifact_contract.rs`.
3. Move read-path summary truncation and semantic hint assembly into `read_path.rs` / `recall.rs`.
4. Split state-side scoped selection helpers into `state/src/fork_patch/memory_repo.rs` while keeping SQL semantics unchanged.
5. Move rollout metadata derivation into `rollout/src/fork_patch/memory_metadata.rs`.
6. Re-run targeted memory acceptance tests and update the fork inventory ledger after each structural move.

Each step above should be one independent commit. No step should mix behavioral change with pure file movement unless absolutely necessary.

## Non-Goals

- Do not re-implement memory as an external service just to achieve patch isolation.
- Do not deepen coupling inside `core/src/codex.rs`, `core/src/config/mod.rs`, or `state/src/runtime/memories.rs` while refactoring.
- Do not preserve the current file layout simply because it already works.

## Success Criteria

- Future sync conflicts in memory work are concentrated in thin hook files plus the dedicated `fork_patch` modules.
- The fork inventory ledger can point to one obvious memory patch namespace instead of many scattered hot files.
- Upstream replacement decisions can be made sub-group by sub-group (`scope-policy`, `recall-engine`, `contract`, `pipeline`).
- Memory behavior remains testable with the current command set: `cargo test -p codex-core memories:: -- --nocapture`, `cargo test -p codex-core prompts::tests::memory_quick_pass_instructions_remain_stable`, and `cargo test -p codex-app-server --tests --no-run --manifest-path codex-rs/Cargo.toml`.

## Relationship To The Broader Fork Strategy

This is not only a memory refactor. It is the template for future fork work:

- first write the inventory ledger entry,
- then define the patch-layer facade,
- then keep upstream hot-file edits thin,
- then define the upstream replacement trigger before adding more feature depth.

That same pattern should later be applied to provider backends, maintenance helpers, and any future custom backend/runtime work.

