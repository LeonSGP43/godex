# godex Maintenance Workflow

This repository is maintained as a long-lived `godex` fork of official
`openai/codex`.

## Branch layout

- `origin` points to your fork: `LeonSGP43/godex`
- `upstream` points to official Codex: `openai/codex`
- `upstream-main` is a local mirror branch for `upstream/main`
- `main` is the release line for your `godex` fork

Keep `upstream-main` free of manual commits. Treat it as the local baseline that
tracks the latest official Codex state.

## Repo-local config baseline

This repo commits a project-local `.codex/config.toml`
so `godex sync-upstream` and fork update prompts work from this checkout
without editing global config.

If you move the repository to a new absolute path, update
`[upstream_updates].repo_root` in that file.

## Maintenance commands

Use the repo wrapper instead of ad-hoc git/cargo sequences:

```bash
# Inspect remotes, branch drift, config, and local install state
bash scripts/godex-maintain.sh status

# Inspect which patch groups and hot files a diff range touches
bash scripts/godex-maintain.sh review-scope

# Preview the next upstream sync without changing the repo
bash scripts/godex-maintain.sh sync --dry-run

# Merge upstream/main into the current branch and rebuild godex
bash scripts/godex-maintain.sh sync

# Refresh the committed upstream baseline metadata without doing a merge
bash scripts/godex-maintain.sh refresh-upstream-metadata

# Require a fast-forward-only sync
bash scripts/godex-maintain.sh sync --ff-only

# Compile check the fork
bash scripts/godex-maintain.sh check

# Verify a runnable godex binary prints its version
bash scripts/godex-maintain.sh smoke

# Validate fork release metadata before push or tag
bash scripts/godex-maintain.sh release-preflight
```

`release-preflight` is the hard gate before any push from `main`.
If `main` is ahead of `origin/main`, it now requires all of the following:

- `VERSION` has been bumped beyond the version on `origin/main`
- `codex-rs/Cargo.toml` matches `VERSION`
- `CHANGELOG.md` has a `## [<version>]` section for that version
- `## [Unreleased]` no longer carries the release entries being pushed
- `UPSTREAM_VERSION`, `UPSTREAM_COMMIT`, and `UPSTREAM_HEAD_COMMIT` all exist
- `README.md`, `docs/godex-fork-manifest.md`, and the root upstream metadata
  files all agree on the same upstream baseline tag and commit
- `UPSTREAM_COMMIT` resolves to the commit pointed to by `UPSTREAM_VERSION`
- `UPSTREAM_HEAD_COMMIT` resolves to the locally fetched `upstream/main` head and is already merged into `HEAD`

Recommended version cadence:

- `0.1.x` for upstream syncs and maintenance-layer improvements
- `0.2.0` for actual fork feature steps or meaningful default behavior changes
- `1.0.0` only when you want to stand behind `godex` as a stable long-term personal release line

## Recommended update loop

1. Start from a clean `main` worktree.
2. Run `bash scripts/godex-maintain.sh status`.
3. Review the reported patch-group overlap and hot-file hits, or run
   `bash scripts/godex-maintain.sh review-scope` directly for a custom range.
4. Refresh `upstream-main`.
5. Create `sync/<upstream-sha-or-date>` from `main`.
6. Run `bash scripts/godex-maintain.sh sync --dry-run`.
7. If the plan looks right, merge upstream on the sync branch.
8. Run `bash scripts/godex-maintain.sh refresh-upstream-metadata` so the
   committed baseline files and docs match the merged upstream tag.
9. Run all acceptance gates from `docs/godex-fork-guidelines.md`.
10. Reinstall with `bash scripts/install/install-godex-from-source.sh`.
11. Merge back to `main` only after validation passes.
12. Before pushing `main`, bump the version and pass `bash scripts/godex-maintain.sh release-preflight`.

## Policy Documents

Use these documents together:

- `docs/godex-fork-guidelines.md`
  - branch policy, patch isolation rules, acceptance gates
- `docs/godex-fork-manifest.md`
  - source of truth for long-lived fork-specific behavior
- `docs/godex-sync-review-checklist.md`
  - keep/adapt/delete procedure for touched patch groups during sync
- `docs/godex-fork-acceptance-matrix.md`
  - verification rows to run for each touched patch group

## Why this layout

This fork intentionally keeps fork-specific behavior near:

- branding and release metadata
- config namespace behavior (`.codex` vs `.godex`)
- upstream sync helpers
- install and release scripts

Avoid broad internal renames or large cross-cutting code moves unless there is a
clear product reason. That keeps future upstream merges cheaper.
