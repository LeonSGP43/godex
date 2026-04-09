# godex Fork Manifest

This file is the durable feature manifest for how `godex` intentionally differs
from official `openai/codex`.

Use it together with:

- `docs/godex-fork-patch-master-plan.md` for the target architecture and
  lifecycle rules
- `docs/godex-fork-inventory-ledger.md` for the current diff inventory and
  file-level ownership

Anything not listed here should be treated as upstream-owned behavior by
default during sync and conflict resolution.

## Lifecycle Classes

- `durable`: intentional long-lived fork behavior we expect to keep unless
  upstream becomes clearly equal or better
- `migrate`: compatibility or transition behavior that should move to a better
  seam and eventually disappear
- `shrink`: residue that exists today but should be reduced or deleted rather
  than expanded

## Patch Groups

### `fork/identity-governance`

- Patch class: `durable`
- Purpose:
  - keep `godex` visibly distinct from official Codex
  - preserve fork-owned release identity, announcement routing, and update
    governance decisions
- Owner files:
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
- Hot overlap files:
  - `codex-rs/core/src/branding.rs`
  - `codex-rs/tui/src/tooltips.rs`
  - `codex-rs/tui/src/updates.rs`
- Verification:
  - `godex --version`
  - `bash scripts/godex-maintain.sh status`
  - inspect fork announcement and update source wiring
- Disable strategy:
  - manifest-level deletion only; no runtime toggle is required for fork
    branding and release identity
- Upstream replacement trigger:
  - none by default for branding itself
  - reconsider only if a sub-piece becomes generic infrastructure rather than
    fork identity
- Deletion condition:
  - delete only if `godex` stops being a separate maintained fork

### `fork/distribution-release`

- Patch class: `durable`
- Purpose:
  - ship `godex` without overwriting `codex`
  - preserve fork-owned package names, installers, and release workflows
- Owner files:
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
  - `VERSION`
  - `CHANGELOG.md`
- Hot overlap files:
  - `codex-rs/Cargo.toml`
  - `codex-rs/Cargo.lock`
  - `codex-rs/cli/Cargo.toml`
- Verification:
  - `bash scripts/install/install-godex-from-source.sh --dry-run`
  - `bash scripts/godex-maintain.sh release-preflight`
  - package staging smoke through `scripts/godex-release*.sh`
- Disable strategy:
  - build and release path selection; distribution behavior is not a normal
    runtime toggle
- Upstream replacement trigger:
  - upstream packaging changes may replace shared mechanics, but fork artifact
    names and release channels remain fork-owned
- Deletion condition:
  - delete only if `godex` no longer ships separate install and release assets

### `fork/maintenance-automation`

- Patch class: `durable`
- Purpose:
  - keep fork maintenance, upstream-sync, and release operations repeatable
  - preserve repo-local maintainer runbooks and skills
- Owner files:
  - `.codex/config.toml`
  - `.codex/skills/godex-*/**`
  - `scripts/godex-maintain.sh`
  - `docs/godex-maintenance.md`
  - `docs/godex-fork-guidelines.md`
- Hot overlap files:
  - `.codex/config.toml`
  - `scripts/godex-maintain.sh`
- Verification:
  - `bash scripts/godex-maintain.sh status`
  - `bash scripts/godex-maintain.sh sync --dry-run`
  - open the skill runbooks and confirm commands still match repo layout
- Disable strategy:
  - manifest-level deletion or operator choice to stop using the local
    maintenance surface
- Upstream replacement trigger:
  - only if upstream later offers equally capable fork-maintainer automation,
    which is unlikely because this is fork-operator machinery
- Deletion condition:
  - delete only if the fork drops its dedicated maintainer workflow surface

### `fork/config-namespace-home`

- Patch class: `durable`
- Purpose:
  - keep default `godex` compatibility with `~/.codex` and project `.codex`
  - provide explicit isolated mode with `godex -g` and `.godex`
  - keep CLI-level memory scope override available without embedding more fork
    policy in hot config paths
- Owner files:
  - `codex-rs/cli/src/main.rs`
  - `codex-rs/cli/tests/godex_home.rs`
  - `codex-rs/core/src/config/**`
  - `codex-rs/core/src/config_loader/**`
  - `codex-rs/core/config.schema.json`
  - `codex-rs/utils/home-dir/src/lib.rs`
  - `docs/config.md`
- Hot overlap files:
  - `codex-rs/cli/src/main.rs`
  - `codex-rs/core/src/config/mod.rs`
  - `codex-rs/core/src/config_loader/mod.rs`
- Verification:
  - `cargo test -p codex-cli godex_home -- --nocapture`
  - `godex --memory-scope project --version`
  - manual smoke with `godex` and `godex -g`
- Disable strategy:
  - runtime and CLI policy selection through `godex` vs `godex -g`, plus config
    values such as `memories.scope`
- Upstream replacement trigger:
  - upstream exposes equivalent namespace and home-selection hooks with
    compatible operator control
- Deletion condition:
  - delete the fork-owned implementation if upstream provides the same behavior
    or a clearly better supported equivalent

### `fork/provider-backends`

- Patch class: `durable`
- Purpose:
  - define the real external spawned-agent backend seam
  - keep provider-specific runtime work outside the Codex binary whenever
    possible
- Owner files:
  - `codex-rs/core/src/agent/**`
  - `codex-rs/core/src/tools/handlers/multi_agents*.rs`
  - `codex-rs/core/src/tools/spec.rs`
  - `docs/agent-roles.md`
  - `docs/external-agent-backends.md`
  - `codex-rs/examples/external_agent_backends/**`
- Hot overlap files:
  - `codex-rs/core/src/agent/backend.rs`
  - `codex-rs/core/src/agent/mod.rs`
  - `codex-rs/core/src/agent/control.rs`
  - `codex-rs/core/src/tools/spec.rs`
  - `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs`
  - `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs`
- Verification:
  - `cargo check -p codex-core --lib`
  - inspect `docs/external-agent-backends.md` against the
    `[agent_backends.<name>]` schema
  - spawn-agent smoke against a configured external backend
- Disable strategy:
  - runtime config and registration disable through `[agent_backends.*]` and
    backend selection; provider workers stay optional external processes
- Upstream replacement trigger:
  - upstream lands a stable external backend or plugin seam that can express
    command or json-stdio workers without fork-only contract types
- Deletion condition:
  - delete or collapse the fork-only contract when upstream can host the same
    provider-worker model with equal or better ergonomics

### `fork/native-grok-legacy`

- Patch class: `migrate`
- Purpose:
  - preserve only the minimum compatibility surface for the legacy native
    `grok` role and tool while real provider calls move to external backends
- Owner files:
  - `codex-rs/core/src/agent/builtins/grok.toml`
  - `codex-rs/core/src/agent/role.rs`
  - `codex-rs/core/src/tools/handlers/grok_research.rs`
  - `codex-rs/core/src/tools/spec.rs`
  - `docs/agent-roles.md`
  - `docs/config.md`
- Hot overlap files:
  - `codex-rs/core/src/agent/role.rs`
  - `codex-rs/core/src/tools/spec.rs`
- Verification:
  - inspect Grok tool spec and handler registration
  - inspect docs for migration language toward `backend = "grok_worker"`
- Disable strategy:
  - registration disable or manifest-level deletion; no new product expansion is
    allowed here
- Upstream replacement trigger:
  - external `grok_worker` becomes the only supported real Grok path, or
    upstream provides a better native equivalent
- Deletion condition:
  - delete once compatibility demand is gone and provider work fully lives in
    external backends

### `fork/memory-system`

- Patch class: `durable`
- Purpose:
  - preserve project-scoped memories, memory scope override, summary token
    control, semantic recall, and the QMD hybrid recall pipeline
  - continue moving fork-specific memory policy behind `fork_patch` seams
    instead of keeping it embedded in hot upstream files
- Owner files:
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
  - `docs/godex-memory-patch-layer-plan.md`
  - `docs/godex-memory-mvp-closure.md`
- Hot overlap files:
  - `codex-rs/cli/src/main.rs`
  - `codex-rs/core/src/config/mod.rs`
  - `codex-rs/core/src/memories/prompts.rs`
  - `codex-rs/core/src/memories/storage.rs`
  - `codex-rs/state/src/runtime/memories.rs`
  - `codex-rs/state/src/runtime/threads.rs`
- Verification:
  - `cargo test -p codex-core memories:: --manifest-path codex-rs/Cargo.toml -- --nocapture`
  - `cargo test -p codex-state --lib --manifest-path codex-rs/Cargo.toml`
  - `cargo test -p codex-app-server --tests --no-run --manifest-path codex-rs/Cargo.toml`
- Disable strategy:
  - runtime config disable for scope selection and summary sizing where
    applicable; patch subgroups may also be deleted individually when upstream
    replacements arrive
- Upstream replacement trigger:
  - upstream lands memory partitioning, recall, or runtime metadata features
    that match or beat the fork implementation
- Deletion condition:
  - delete overlapping subgroups patch-by-patch instead of preserving them for
    historical reasons once upstream is equal or better

### `fork/bootstrap-residue`

- Patch class: `shrink`
- Purpose:
  - track remaining mixed hot-path fork drift that still exists because the
    original fork bootstrap mixed multiple concerns together
  - prevent new feature depth from being added to residue files
- Owner files:
  - `codex-rs/cli/src/login.rs`
  - `codex-rs/cli/src/mcp_cmd.rs`
  - `codex-rs/login/src/**`
  - `codex-rs/core/src/network_proxy_loader.rs`
  - `codex-rs/core/src/mcp_connection_manager.rs`
  - `codex-rs/tui/src/app.rs`
  - `codex-rs/tui/src/history_cell.rs`
  - `codex-rs/tui/src/status/**`
  - other files listed under `fork/bootstrap-residue` in
    `docs/godex-fork-inventory-ledger.md`
- Hot overlap files:
  - `codex-rs/cli/src/login.rs`
  - `codex-rs/cli/src/mcp_cmd.rs`
  - `codex-rs/core/src/network_proxy_loader.rs`
  - `codex-rs/core/src/mcp_connection_manager.rs`
  - `codex-rs/tui/src/app.rs`
- Verification:
  - manual diff review during sync branches
  - targeted login, proxy, MCP, and TUI smoke after each upstream merge
- Current shrink status:
  - CLI login copy now has a thin adapter at `codex-rs/cli/src/login_copy.rs`
  - onboarding copy now has a thin adapter at
    `codex-rs/tui/src/onboarding/bootstrap_copy.rs`
  - remaining residue should continue shrinking out of inline auth/login hot
    paths instead of adding new fork-specific copy there
- Disable strategy:
  - isolate behind thinner adapters or delete entirely; no new fork product
    depth should remain here
- Upstream replacement trigger:
  - upstream catches up on the behavior, or the fork extracts the remaining
    logic into a named durable patch group
- Deletion condition:
  - delete as residue is burned down or reclassified into explicit bounded patch
    groups

## Hot Overlap Files

These files need extra care during upstream sync because they bridge fork-owned
policy into upstream-heavy code:

- `codex-rs/cli/src/main.rs`
- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/branding.rs`
- `codex-rs/core/src/agent/backend.rs`
- `codex-rs/core/src/agent/mod.rs`
- `codex-rs/core/src/agent/control.rs`
- `codex-rs/core/src/tools/spec.rs`
- `codex-rs/core/src/tools/handlers/multi_agents/spawn.rs`
- `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs`
- `codex-rs/core/src/memories/prompts.rs`
- `codex-rs/core/src/memories/storage.rs`
- `codex-rs/state/src/runtime/memories.rs`
- `codex-rs/state/src/runtime/threads.rs`
- `codex-rs/tui/src/updates.rs`
- `codex-rs/tui/src/tooltips.rs`
- `codex-rs/tui_app_server/src/updates.rs`
- `codex-rs/tui_app_server/src/tooltips.rs`

Rules:

- keep only the minimum fork-specific logic in these files
- move fork policy into `fork_patch`, adapters, scripts, docs, or external
  workers whenever possible
- do not expand provider-specific runtime work here when it can be expressed as
  an external backend
- do not add new product depth to `fork/bootstrap-residue`

## Sync Review Checklist

For every upstream sync branch, answer these questions before merging back to
`main`:

1. Did any patch group's owner files change upstream?
2. Did any hot-overlap file require manual conflict resolution?
3. Does the resulting diff still match the patch groups above?
4. Did every affected patch group's verification commands pass?
5. Does any patch group now qualify for `delete` because upstream is equal or
   better?

If the answer to question 3 is no, stop and reduce the fork surface before
merging.
