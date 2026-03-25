# CLAUDE.md

## Role

You are working in the `godex` repository, a personal upstream-first fork of
official `openai/codex`.

Your job is not only to implement changes, but to preserve the repository's
ability to absorb upstream Codex updates with low friction.

## Binding Project Constitution

Treat these files as binding policy in this order:

1. `AGENTS.md`
2. `docs/godex-fork-guidelines.md`
3. `docs/godex-fork-manifest.md`
4. `docs/godex-maintenance.md`

If you propose a change that conflicts with them, update the policy documents
first or ask for an explicit decision.

## Primary Objectives

1. Keep `godex` close to official Codex.
2. Preserve only intentional, documented fork-specific behavior.
3. Prevent unnecessary expansion of the fork surface.
4. Make every upstream sync safer, smaller, and more repeatable.

## Hard Rules

1. Never merge official upstream directly into `main`.
2. Never treat `main` as an experimental branch.
3. Never preserve a fork difference just because it already exists.
4. Never add meaningful fork behavior without recording it in
   `docs/godex-fork-manifest.md`.
5. Never broaden divergence through large cross-cutting edits when a thin
   adapter will do.
6. Never do broad internal crate renames for branding purposes.

## Required Branch Model

Use:

- `upstream-main` for the local mirror of official Codex
- `main` for the validated `godex` line
- `sync/<upstream-sha-or-date>` for one upstream integration
- `feat/<topic>` for one independent feature or fix

Required sync flow:

1. clean worktree
2. fetch upstream
3. refresh `upstream-main`
4. create `sync/<...>` from `main`
5. merge `upstream-main`
6. resolve conflicts without adding new work
7. run acceptance gates
8. merge back to `main` only after validation

## Fork Patch Policy

Long-lived differences should stay within these patch groups:

- branding
- config namespace
- update governance
- distribution
- maintenance tooling

Prefer changing:

- `README.md`
- `CHANGELOG.md`
- `VERSION`
- `.codex/config.toml`
- maintenance scripts
- install scripts
- explicit fork boundary files

Be very cautious changing:

- protocol schema internals
- app-server transport internals
- core runtime internals
- broad TUI behavior unrelated to fork identity

## Hot Files

These are known conflict hotspots and should carry only thin fork adapters:

- `codex-rs/cli/src/main.rs`
- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/branding.rs`
- `codex-rs/tui/src/updates.rs`
- `codex-rs/tui_app_server/src/updates.rs`
- `codex-rs/tui/src/tooltips.rs`
- `codex-rs/tui_app_server/src/tooltips.rs`

When touching them:

1. keep the diff minimal
2. avoid unrelated cleanup
3. prefer extracting helper logic over expanding inline divergence

## Acceptance Gates

Every upstream sync must pass:

- `bash scripts/godex-maintain.sh status`
- `bash scripts/godex-maintain.sh sync --dry-run`
- `bash scripts/godex-maintain.sh check`
- `bash scripts/godex-maintain.sh smoke`
- `bash scripts/godex-maintain.sh release-preflight`

Also verify:

- `godex --version`
- default Codex-compatible config behavior
- `godex -g` isolated config behavior
- fork release source points to `LeonSGP43/godex`
- upstream source points to `openai/codex`

## Conflict Resolution Policy

When upstream and fork behavior collide:

1. prefer upstream by default
2. re-apply fork behavior only if it is manifest-listed
3. shrink divergence if upstream now offers a better native path
4. do not paste old fork code back wholesale

Ask:

- Is this still a required fork behavior?
- Can it move into a smaller helper?
- Can upstream now replace it?

## Documentation And Manifest Rules

When fork behavior changes:

1. update `docs/godex-fork-manifest.md`
2. update `docs/godex-fork-guidelines.md` if policy changed
3. update `docs/godex-maintenance.md` if workflow changed
4. update `CHANGELOG.md`
5. update `README.md` if user-facing behavior changed

If a difference is not documented, do not assume it should survive sync.

## Engineering Rules

For `codex-rs` changes:

- follow existing Rust style and lint conventions
- run `just fmt` after Rust code changes
- run focused tests for the touched crate
- keep TUI and `tui_app_server` behavior aligned when they mirror each other
- add or update snapshot coverage for intentional UI changes
- update generated schemas and Bazel lockfiles when required by the change

## Final Principle

Optimize for a fork that is easy to rebase, easy to audit, and easy to shrink.

If a change makes `godex` more unique but also makes future upstream absorption
materially harder, reject or redesign the change unless there is a clear,
documented product reason.
