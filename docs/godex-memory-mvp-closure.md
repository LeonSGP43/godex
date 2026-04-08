# godex Memory MVP Closure

This document freezes the current memory fork-patch work at an MVP cutline.

The goal of this cutline is not to make the memory patch perfectly pure. The goal is to stop at the point where:

- the memory feature set is working,
- the hottest fork-specific seams already have a real patch-layer shape,
- validation has been run at the memory, state, and app-server layers,
- future refinement can continue later without blocking current delivery.

## MVP Decision

The current memory fork-patch work is considered MVP-complete for now.

That means the project should stop adding new non-blocking micro-refactors in the memory lane until a new bug, sync conflict, or explicit follow-up plan justifies reopening it.

## What Is Considered Done At The MVP Cutline

### 1. `patch/memory-state-runtime`

Treat this lane as complete for the MVP.

Already extracted behind fork-owned seams:

- default scope helpers
- thread scope persistence helper
- repeated runtime scope-query binding
- phase2 job-key binding
- phase2 selection-state queries
- phase2 enqueue helpers

Operational result:

- `state/src/runtime/memories.rs` is no longer carrying the worst repeated fork glue.
- `state/src/fork_patch/memory_repo.rs` is now the real owner for the main fork-specific scope/query helpers in this lane.

### 2. `patch/memory-read-path`

Treat this lane as complete for the MVP.

Already achieved:

- read-path helper logic moved behind `fork_patch::memory`
- leftover wrapper/shim removed from `prompts.rs`

Operational result:

- the read path is good enough for delivery
- no further prompt-path cleanup is required unless a real regression appears

### 3. `patch/memory-artifact-contract`

Treat this lane as MVP-sufficient, even though it is not perfectly finished.

Already extracted:

- scope and artifact root helpers
- artifact path helpers
- empty-consolidation cleanup targets
- rollout-summary file-name and relative-path naming
- rollout-summary suffix parsing
- semantic-index vector metadata naming

Operational result:

- the most important hot-path artifact-contract rules are already moving through `fork_patch::memory`
- remaining work in this lane is mostly cleanup or purity work, not MVP-blocking work

## What Is Explicitly Deferred

The following items are intentionally deferred until after MVP:

### Deferred A: residual artifact-contract cleanup

Examples:

- remaining filename constants in low-risk tests
- remaining filename constants in non-hot-path docs/templates
- residual path/name cleanup that does not change runtime ownership materially

Why deferred:

- this is merge-cost optimization work, not delivery-blocking work

### Deferred B: `memories/usage.rs` filename classification cleanup

Examples:

- `MEMORY.md`
- `memory_summary.md`
- `raw_memories.md`

Why deferred:

- this code is closer to usage classification than hot-path artifact generation
- changing it now would continue the micro-refactor pattern instead of closing the MVP

### Deferred C: broader fork residue outside the current memory lane

Examples:

- `patch/bootstrap-proxy-mcp`
- `patch/bootstrap-login-auth`
- later backend-contract refinement

Why deferred:

- these are separate follow-up tracks and should not hold the memory MVP hostage

## Reopen Rules

Do not reopen memory refactor work just to improve patch purity.

Reopen only if one of these happens:

- a validation command starts failing
- an upstream sync creates a real merge conflict in the current hot files
- a user-visible memory bug appears
- a later planned refinement phase is explicitly approved

## MVP Validation Evidence

The following commands passed at the MVP cutline:

- `cargo test -p codex-core memories:: --manifest-path codex-rs/Cargo.toml -- --nocapture`
- `cargo test -p codex-state --lib --manifest-path codex-rs/Cargo.toml`
- `cargo test -p codex-app-server --tests --no-run --manifest-path codex-rs/Cargo.toml`

Observed result summary:

- `codex-core memories::` passed with memory-layer unit and integration coverage green
- `codex-state --lib` passed with `84 passed; 0 failed`
- `codex-app-server --tests --no-run` finished successfully and produced all app-server test executables

## Exact Next Refinement Order After MVP

If and only if refinement is reopened later, use this order:

1. finish the smallest remaining `patch/memory-artifact-contract` cuts
2. touch `memories/usage.rs` only if it still helps reduce real future sync cost
3. move to broader fork residue cleanup outside memory
4. continue backend-contract refinement separately from the memory lane

## Stop Condition

At this point, the correct action is:

- stop micro-refining memory,
- keep the current validated shape,
- move on to other higher-priority MVP work.
