# godex Fork Manifest

This file records the intended long-lived differences between `godex` and
official `openai/codex`.

Anything not listed here should be treated as upstream-owned behavior by
default during conflict resolution.

## Fork Patch Groups

### 1. Branding And Release Identity

- Purpose:
  - make the fork present itself as `godex` where required
  - point release metadata and remote announcement plumbing to
    `LeonSGP43/godex`
- Owner files:
  - `codex-rs/core/src/branding.rs`
  - `codex-rs/tui/src/tooltips.rs`
  - `codex-rs/tui_app_server/src/tooltips.rs`
  - `README.md`
  - `announcement_tip.toml`
- Verification:
  - `godex --version`
  - inspect startup announcement source behavior

### 2. Config Namespace Behavior

- Purpose:
  - keep default `godex` compatible with existing Codex config locations
  - provide explicit isolated mode with `godex -g`
- Owner files:
  - `codex-rs/cli/src/main.rs`
  - `codex-rs/core/src/config/mod.rs`
  - `docs/config.md`
- Required behavior:
  - `godex` uses `~/.codex` and project `.codex`
  - `godex -g` uses `~/.godex` and project `.godex`
  - first-run `godex -g` initializes the global `~/.godex` directory automatically
- Verification:
  - CLI parse tests
  - CLI integration test for first-run `godex -g`
  - config loader tests
  - manual smoke with `godex` and `godex -g`

### 3. Fork Update Governance

- Purpose:
  - separate `godex` release tracking from official Codex upstream tracking
- Owner files:
  - `codex-rs/tui/src/updates.rs`
  - `codex-rs/tui_app_server/src/updates.rs`
  - `codex-rs/core/src/config/mod.rs`
  - `.codex/config.toml`
  - `docs/config.md`
- Required behavior:
  - `godex_updates` targets `LeonSGP43/godex`
  - `upstream_updates` targets `openai/codex`
  - upstream gap and fork update checks remain separate
- Verification:
  - inspect effective config
  - `godex sync-upstream --dry-run`
  - targeted update-path smoke checks

### 4. Grok Research Integration

- Purpose:
  - preserve the fork-specific native `grok` research tool and related config
    surface
  - keep Grok-backed built-in agent roles and docs coherent when upstream
    tooling changes
- Owner files:
  - `codex-rs/core/src/tools/handlers/mod.rs`
  - `codex-rs/core/src/tools/spec.rs`
  - `docs/config.md`
- Required behavior:
  - native `grok` remains registered as a tool
  - Grok-focused built-in roles can rely on config-driven routing defaults
  - docs describe the `[grok]` config surface
- Verification:
  - inspect tool spec for `grok`
  - inspect effective config or schema for `[grok]`
  - targeted smoke of Grok tool registration and config loading

### 5. Distribution And Local Install

- Purpose:
  - allow `godex` to coexist with official `codex`
- Owner files:
  - `scripts/install/install-godex-from-source.sh`
  - `README.md`
  - `docs/install.md`
  - `VERSION`
  - `codex-rs/Cargo.toml`
  - `codex-rs/cli/Cargo.toml`
- Required behavior:
  - source install manages `godex` without overwriting `codex`
  - version metadata stays aligned
- Verification:
  - `bash scripts/install/install-godex-from-source.sh --dry-run`
  - `bash scripts/godex-maintain.sh release-preflight`

### 6. Maintenance Tooling

- Purpose:
  - make upstream sync and fork checks repeatable from this repo
- Owner files:
  - `.codex/config.toml`
  - `scripts/godex-maintain.sh`
  - `docs/godex-maintenance.md`
  - `docs/godex-fork-guidelines.md`
  - `docs/godex-fork-manifest.md`
- Required behavior:
  - repo-local maintenance defaults work without global-only configuration
  - maintainers have a standard sync and validation path
- Verification:
  - `bash scripts/godex-maintain.sh status`
  - `bash scripts/godex-maintain.sh sync --dry-run`
  - `bash scripts/godex-maintain.sh check`
  - `bash scripts/godex-maintain.sh smoke`

### 7. Memory Scope Partitioning

- Purpose:
  - keep the legacy global memory flow available while allowing `godex` to
    isolate memories by detected project root
  - limit startup memory injection size with a configurable summary token cap
- Owner files:
  - `codex-rs/core/src/config/types.rs`
  - `codex-rs/core/src/config/mod.rs`
  - `codex-rs/core/src/memories/scope.rs`
  - `codex-rs/core/src/memories/prompts.rs`
  - `codex-rs/core/src/memories/phase1.rs`
  - `codex-rs/core/src/memories/phase2.rs`
  - `codex-rs/state/src/runtime/memories.rs`
  - `codex-rs/state/src/runtime/threads.rs`
  - `docs/config.md`
  - `docs/godex-memory-system.md`
- Required behavior:
  - `memories.scope = "global"` keeps using `<CODEX_HOME>/memories`
  - `memories.scope = "project"` uses a project-partitioned root under
    `<CODEX_HOME>/memories/scopes/project/...`
  - project scope only selects, consolidates, and injects memories belonging to
    the same detected project root
  - `memories.summary_token_limit` controls how much `memory_summary.md` is
    injected into developer instructions
- Verification:
  - `cargo test -p codex-core memories:: -- --nocapture`
  - `cargo test -p codex-app-server --tests --no-run --manifest-path codex-rs/Cargo.toml`
  - inspect scoped memory artifacts under `~/.codex/memories/scopes/project/`

## Hot Overlap Files

These files require special care during upstream sync:

- `codex-rs/cli/src/main.rs`
- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/branding.rs`
- `codex-rs/tui/src/updates.rs`
- `codex-rs/tui_app_server/src/updates.rs`
- `codex-rs/tui/src/tooltips.rs`
- `codex-rs/tui_app_server/src/tooltips.rs`

Rule:

- keep only the minimum fork-specific logic in these files
- if fork behavior grows here, update this manifest and add verification

## Sync Review Checklist

For every upstream sync branch, answer these questions before merging back to
`main`:

1. Did any fork-owned file change?
2. Did any hot-overlap file require manual conflict resolution?
3. Does the resulting diff still match the patch groups above?
4. Did every required verification command pass?
5. Did upstream now provide a better native solution that lets us delete fork
   code?

If the answer to question 3 is no, stop and reduce the fork surface before
merging.
