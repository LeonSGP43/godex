# AGENTS.md

## Constitutional Status

This file is the root constitution for agent work in this repository.

It applies to all Codex work unless a deeper-scope `AGENTS.md` adds stricter
rules for a subdirectory. Lower-scope instructions may refine this file but may
not weaken it.

This repository is maintained as `godex`, an upstream-first fork of official
`openai/codex`.

Canonical policy documents:

- `docs/godex-fork-guidelines.md`
- `docs/godex-fork-manifest.md`
- `docs/godex-maintenance.md`

If any instruction conflicts with those documents, update the documents first or
treat them as the source of truth for fork governance.

## Mission

Build and maintain `godex` as a personal fork of Codex that:

- absorbs official upstream changes quickly and continuously
- keeps only a small, explicit, durable fork patch layer
- preserves fork-specific behavior through repeatable verification
- avoids unnecessary long-term divergence from official Codex

## Non-Negotiable Rules

1. Official Codex is the product baseline. Fork behavior must justify its
   existence explicitly.
2. Do not merge official upstream directly into `main`.
3. Treat `main` as a validated fork release line, not a scratch branch.
4. Keep fork-specific logic localized to a small set of modules or files.
5. Do not broadly rename internal `codex-*` crates for branding purposes.
6. Every meaningful fork-specific difference must be recorded in
   `docs/godex-fork-manifest.md`.
7. Every upstream sync must pass the required acceptance gates before it can
   return to `main`.
8. If a difference is not in the manifest, prefer upstream behavior by default.

## Branch And Sync Governance

Required branch model:

- `origin/main`
  - published `godex` release line
- `upstream-main`
  - local mirror of `upstream/main`
  - never modified manually
- `sync/<upstream-sha-or-date>`
  - temporary integration branch for one upstream sync
- `feat/<topic>`
  - one independent fork feature or fix

Required upstream sync flow:

1. ensure current work is committed or stashed
2. fetch official upstream
3. refresh `upstream-main`
4. create `sync/<...>` from `main`
5. merge `upstream-main` into the sync branch
6. resolve conflicts without adding new product work
7. run acceptance gates
8. merge validated result back into `main`

Never do these in the same change:

- upstream sync
- new feature work
- unrelated refactor
- formatting-only cleanup

## Fork Patch Policy

Long-lived fork changes should belong to one of these patch groups:

- `fork/branding`
- `fork/config-namespace`
- `fork/update-governance`
- `fork/distribution`
- `fork/maintenance`

Allowed fork surfaces:

- `README.md`
- `CHANGELOG.md`
- `VERSION`
- `.codex/config.toml`
- `scripts/godex-maintain.sh`
- `scripts/install/install-godex-from-source.sh`
- `docs/godex-*.md`
- explicit branding, config namespace, update, and announcement-source files

Avoid expanding fork surface in:

- protocol schema churn
- app-server transport internals
- core agent runtime internals
- broad UI behavior unrelated to fork identity

## Hot Files

The following files are hot-overlap areas and require special care:

- `codex-rs/cli/src/main.rs`
- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/branding.rs`
- `codex-rs/tui/src/updates.rs`
- `codex-rs/tui_app_server/src/updates.rs`
- `codex-rs/tui/src/tooltips.rs`
- `codex-rs/tui_app_server/src/tooltips.rs`

Rules for hot files:

1. Keep only thin fork adapters in these files.
2. Move durable fork policy into smaller helpers when possible.
3. Do not mix unrelated work into hot-file edits.
4. Resolve hot-file conflicts only inside a dedicated sync branch.

## Manifest Requirement

Before adding or changing fork behavior:

1. define the patch group
2. list owner files
3. define at least one repeatable verification step
4. update `docs/godex-fork-manifest.md`

If a fork behavior cannot be verified repeatably, redesign it before landing it.

## Acceptance Gates

Every upstream sync must pass all of the following:

1. `bash scripts/godex-maintain.sh status`
2. `bash scripts/godex-maintain.sh sync --dry-run`
3. `bash scripts/godex-maintain.sh check`
4. `bash scripts/godex-maintain.sh smoke`
5. `bash scripts/godex-maintain.sh release-preflight`

Required behavior checks:

1. `godex --version` reports the expected fork version
2. default `godex` still uses Codex-compatible config locations
3. `godex -g` still uses isolated `.godex` config locations
4. fork update source still targets `LeonSGP43/godex`
5. upstream gap and sync plumbing still target `openai/codex`

Recommended gate:

- compare `upstream-main...main` against the fork manifest
- if changes spread beyond expected fork touchpoints, stop and review manually

## Release And Push Gate

Do not push `main` unless version governance is complete:

1. bump `VERSION`
2. update `codex-rs/Cargo.toml` to the same version
3. move release notes out of `## [Unreleased]` into `## [<version>]`
4. keep `## [Unreleased]` empty for the next cycle
5. run `bash scripts/godex-maintain.sh release-preflight`

If `main` is ahead of `origin/main` and `VERSION` has not changed, pushing is forbidden.

Version strategy:

- `0.1.x` for upstream syncs, governance hardening, documentation/script updates, and small fixes
- `0.2.0` only when the fork adds clear user-facing capabilities or materially changes default behavior
- `1.0.0` only when `godex` is being treated as a stable long-term personal distribution

## Distribution Channel Policy (Required)

Local runnable `godex` must come from the published npm package only.

Required rules:

1. Do not run or distribute a local source-built runtime as the normal local install target.
2. Do not replace active runtime paths (for example `~/.local/bin/godex`) with ad-hoc binaries from `codex-rs/target/*`.
3. Treat `scripts/install/install-godex-from-source.sh` as a development helper, not an accepted end-user installation channel.
4. A version is considered installable for daily use only after all release gates pass, npm publish succeeds, and local installation is updated from npm.
5. If release is blocked, fix the release path itself; do not bypass it by switching users to an unpublished local binary.

## Conflict Resolution Policy

When resolving conflicts:

1. prefer upstream behavior by default
2. re-apply fork behavior only where the manifest says it is required
3. avoid copying old fork code back wholesale
4. if upstream now provides a better native solution, delete the fork patch
   instead of preserving divergence

Ask these questions for every hot-file conflict:

- is this still a real fork requirement
- can it move into a smaller helper or provider
- can the upstream implementation replace the old fork behavior

## Commit Discipline

One commit must equal one independent change.

Preferred commit types:

- `feat`
- `fix`
- `refactor`
- `docs`
- `test`
- `chore`

Preferred fork-maintenance subjects:

- `chore(godex): add maintenance gate`
- `fix(godex): preserve codex-compatible config namespace`
- `docs(godex): record fork patch policy`

Do not combine:

- upstream sync and new fork feature work
- refactor and functional change
- formatting-only edits with product behavior changes

## Documentation Discipline

When behavior changes, update the relevant docs in the same change.

Mandatory updates when applicable:

- `docs/godex-fork-guidelines.md`
- `docs/godex-fork-manifest.md`
- `docs/godex-maintenance.md`
- `CHANGELOG.md`
- `README.md`

## codex-rs Engineering Rules

The `codex-rs` workspace keeps the following engineering standards.

- Crate names are prefixed with `codex-`.
- When using `format!` and a variable can be inlined into `{}`, do so.
- Never add or modify code related to
  `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` or `CODEX_SANDBOX_ENV_VAR`.
- Always collapse if statements when appropriate.
- Always inline `format!` args when possible.
- Use method references over closures when possible.
- Avoid bool or ambiguous `Option` parameters that produce unclear callsites.
- When positional literals are unavoidable, use exact `/*param_name*/`
  argument comments per the repo lint convention.
- Prefer exhaustive `match` statements over wildcard arms when feasible.
- Prefer comparing whole objects in tests rather than field-by-field checks.
- If you change `ConfigToml` or nested config types, run
  `just write-config-schema`.
- If you change Rust dependencies, update `MODULE.bazel.lock` with
  `just bazel-lock-update` and then run `just bazel-lock-check`.
- If you add compile-time file reads such as `include_str!`,
  `include_bytes!`, or `sqlx::migrate!`, update Bazel data declarations too.
- Do not create small helper methods that are referenced only once.
- When running Rust commands such as `just fix` or `cargo test`, be patient
  and do not try to kill them by PID; Rust lock contention can make them slow.

## Module Size And Extraction Rules

- Prefer adding new modules instead of growing existing ones.
- Target Rust modules under roughly 500 LoC, excluding tests.
- If a file exceeds roughly 800 LoC, put new functionality in a new module
  unless there is a strong documented reason not to.
- When extracting code, move related tests and docs with it so invariants stay
  close to the owner implementation.

## Formatting, Lint, And Test Rules

When you change Rust code in `codex-rs`:

1. run `just fmt` in `codex-rs`
2. run focused tests for the changed project
3. if changes affect common, core, or protocol crates, ask before running the
   full suite with `cargo test` or `just test`
4. before finalizing a large Rust change, run `just fix -p <project>`
5. run `just argument-comment-lint`

Do not re-run tests after `fix` or `fmt` unless there is a specific reason.

## TUI Rules

- Follow `codex-rs/tui/styles.md`.
- When behavior exists in both `codex-rs/tui` and `codex-rs/tui_app_server`,
  keep them aligned unless there is a documented reason not to.
- Prefer ratatui `Stylize` helpers over manual style construction where
  practical.
- Always use the repository wrapping helpers for wrapped `Line` content.

## Snapshot Test Rules

Any intentional user-visible UI change must include snapshot coverage.

Typical workflow:

1. `cargo test -p codex-tui`
2. `cargo insta pending-snapshots -p codex-tui`
3. inspect `*.snap.new`
4. `cargo insta accept -p codex-tui` only when the changes are intentional

## App-Server API Rules

For `app-server` and protocol work:

1. all active API development belongs in v2
2. use `*Params`, `*Response`, and `*Notification` naming consistently
3. keep wire fields camelCase unless config APIs intentionally mirror
   `config.toml` snake_case
4. keep Rust and TypeScript rename metadata aligned
5. use explicit tagging for discriminated unions
6. prefer string IDs at the API boundary
7. use integer Unix seconds for timestamps
8. update docs and generated schemas when API shape changes

Validation workflow:

- `just write-app-server-schema`
- `cargo test -p codex-app-server-protocol`

## Final Standard

This repository should remain easy to rebase onto official Codex.

If a proposed change improves `godex` in the short term but makes future
upstream absorption materially harder, the default answer is no until the change
is redesigned.
