# godex Fork Maintenance Guidelines

This document defines the long-term maintenance rules for keeping `godex`
close to official `openai/codex` while preserving fork-specific behavior.

## Target State

`godex` should behave like an upstream-first fork:

- official Codex remains the primary product baseline
- `godex` keeps only a small, explicit patch layer on top
- upstream updates are absorbed continuously instead of in large catch-up jumps
- every fork-specific feature has a named owner area and verification step

## Core Rules

1. Prefer additive fork layers over invasive core rewrites.
2. Keep fork-specific logic localized to a small set of files or modules.
3. Do not rename internal `codex-*` crates just for branding.
4. Treat `main` as a validated release line, not a scratch branch.
5. Never merge official upstream directly into `main`.
6. Every meaningful fork feature must be listed in the fork manifest.
7. Every upstream sync must pass acceptance gates before entering `main`.

## Branch Strategy

Use this branch model consistently:

- `origin/main`
  - the published `godex` release line
- `upstream-main`
  - local mirror of `upstream/main`
  - no manual commits
- `sync/<upstream-sha-or-date>`
  - temporary integration branch for one upstream sync
- `feat/<topic>`
  - fork feature branch for a single change

Required flow:

1. fetch official upstream
2. refresh `upstream-main`
3. branch from `main` into `sync/<...>`
4. merge `upstream-main` into the sync branch
5. resolve conflicts without adding new product work
6. run acceptance gates
7. merge validated result back into `main`

## Fork Patch Groups

All long-lived fork changes should belong to one of these patch groups:

- `fork/branding`
  - app display name, repo slug, release links, announcement source
- `fork/config-namespace`
  - default Codex-compatible mode and explicit `.godex` isolated mode
- `fork/update-governance`
  - `godex_updates`, upstream gap reporting, release metadata
- `fork/provider-backends`
  - external spawned-agent backends, provider bridge examples, and backend
    runtime documentation
- `fork/distribution`
  - install script, release versioning, local packaging
- `fork/maintenance`
  - sync helpers, project-local config, maintenance runbooks

If a change does not clearly fit one of these groups, challenge whether it
should live in the fork at all.

## Allowed Change Surfaces

Fork-specific changes should preferentially stay in:

- repository metadata files
  - `README.md`
  - `CHANGELOG.md`
  - `VERSION`
- maintenance and distribution files
  - `.codex/config.toml`
  - `scripts/godex-maintain.sh`
  - `scripts/install/install-godex-from-source.sh`
  - `docs/godex-*.md`
- product boundary files that already carry fork behavior
  - branding
  - config namespace selection
  - update/release plumbing
  - explicit fork announcement sources

Avoid expanding the fork surface in:

- protocol schema churn
- app-server transport internals
- core agent runtime internals
- provider-specific native tool handlers and provider-branded built-in roles
- broad UI behavior unrelated to fork identity

Those areas change rapidly upstream and raise merge cost.

For provider integrations:

- prefer `backend = "<provider>_worker"` plus `[agent_backends.<name>]`
- keep provider HTTP/auth/retry logic outside upstream hot paths whenever
  possible
- treat any native provider-specific tool or built-in provider role as a legacy
  compatibility shim that needs an extraction plan

## Hot Files

The following files are currently known hot-overlap areas between `godex` and
recent upstream Codex updates:

- `codex-rs/cli/src/main.rs`
- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/branding.rs`
- `codex-rs/tui/src/updates.rs`
- `codex-rs/tui_app_server/src/updates.rs`
- `codex-rs/tui/src/tooltips.rs`
- `codex-rs/tui_app_server/src/tooltips.rs`

Rules for hot files:

1. Keep only thin fork adapters here.
2. Move durable fork policy into dedicated helper modules when possible.
3. Do not mix unrelated feature work into hot-file edits.
4. When upstream changes these files, resolve conflicts in the sync branch only.

## Sync Discipline

Before each upstream sync:

1. ensure the worktree is clean
2. ensure current fork changes are committed
3. inspect upstream drift
4. run a dry-run sync command
5. create a dedicated `sync/<...>` branch

During sync:

1. merge upstream into the sync branch only
2. resolve conflicts with the goal of preserving fork behavior through the
   smallest possible adapter changes
3. do not add new features, refactors, or formatting-only edits

After sync:

1. run acceptance gates
2. review the changed fork touchpoints
3. update changelog if the fork behavior changed
4. if `main` will be pushed, bump the fork version and promote release notes out of `Unreleased`
5. merge back to `main` only after validation passes

## Acceptance Gates

Every upstream sync must pass all required gates.

Required gates:

1. `bash scripts/godex-maintain.sh status`
2. `bash scripts/godex-maintain.sh sync --dry-run`
3. `bash scripts/godex-maintain.sh check`
4. `bash scripts/godex-maintain.sh smoke`
5. `bash scripts/godex-maintain.sh release-preflight`

Push rule for `main`:

- if `main` is ahead of `origin/main`, `VERSION` must also move forward
- `codex-rs/Cargo.toml` must match `VERSION`
- `CHANGELOG.md` must contain `## [<version>]` for that version
- `## [Unreleased]` must be empty for the release being pushed

Version cadence:

- use `0.1.x` for routine upstream absorption, fork governance, docs/scripts work, and small fixes
- use `0.2.0` only when fork-specific product behavior takes a real step forward
- use `1.0.0` only when the fork is ready to be treated as a stable long-term personal line

Required behavior checks:

1. `godex --version` reports the expected fork version
2. default `godex` still uses Codex-compatible config locations
3. `godex -g` still uses isolated `.godex` config locations
4. fork update source still points at `LeonSGP43/godex`
5. upstream gap and sync plumbing still reference `openai/codex`

Recommended gate:

- compare `upstream-main...main` diff against the fork manifest
  - if changes spread beyond expected fork touchpoints, stop and review manually

## Conflict Resolution Rules

When conflicts occur:

1. prefer upstream behavior by default
2. re-apply fork-specific behavior only where the manifest says it is required
3. avoid copying old fork code back wholesale
4. if a fork behavior can be reintroduced as a smaller adapter after merge, do
   that instead of preserving a large divergent block

When a conflict touches a hot file, ask:

- is this still a real fork requirement?
- can this fork behavior move into a smaller helper or provider?
- is the upstream implementation now good enough to replace the old fork patch?

## Feature Admission Rules

Before adding a new fork feature:

1. define which patch group it belongs to
2. list the owner files
3. define one concrete verification command or smoke check
4. record it in `docs/godex-fork-manifest.md`

Reject or redesign features that:

- require wide edits across volatile upstream internals
- permanently expand the hot-file surface without a strong reason
- cannot be validated by a repeatable check

## Commit and PR Discipline

For fork maintenance work:

- keep one commit equal to one independent change
- do not mix upstream sync, new feature work, and maintenance refactors
- prefer commit subjects like:
  - `chore(godex): add maintenance gate`
  - `fix(godex): preserve codex-compatible config namespace`
  - `docs(godex): record fork patch policy`

## Manifest Requirement

The fork manifest is the source of truth for what `godex` is allowed to keep
different from official Codex.

Whenever fork behavior changes:

1. update the manifest
2. update verification steps if needed
3. keep the manifest small and explicit

If a difference is not in the manifest, it should not survive upstream sync by
default.
