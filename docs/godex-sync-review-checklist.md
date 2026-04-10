# godex Sync Review Checklist

Use this checklist before and during every upstream sync branch.

The goal is to review the fork as explicit patch groups, not as one large diff.

## Pre-sync

Run these first:

```bash
bash scripts/godex-maintain.sh status
bash scripts/godex-maintain.sh review-scope
bash scripts/godex-maintain.sh sync --dry-run
```

Capture:

- current branch
- upstream drift
- touched patch groups
- hot-overlap files reported by `review-scope`

If the worktree is dirty, stop and clean it before sync work.

## Patch-group review

For every touched patch group, classify it as:

- `keep`: upstream did not replace this fork behavior and the current seam is still correct
- `adapt`: the behavior is still needed, but the hook, adapter, or verification needs to change
- `delete`: upstream is now equal or better, so the fork patch should be retired

Use this template for each touched group:

```text
patch_group:
decision: keep | adapt | delete
why:
hot_overlap_files:
upstream_replacement_check:
disable_strategy_check:
verification_to_run:
notes:
```

## Decision rules by lane

Apply these defaults unless the current sync evidence proves otherwise:

- `fork/provider-backends`
  - default: `keep`
  - switch to `adapt` only if upstream changed the spawned-agent backend seam
  - switch to `delete` only if upstream now provides the same external backend model
- `fork/config-namespace-home`
  - default: `keep`
  - switch to `adapt` if upstream changed config-home resolution or CLI override hooks
  - switch to `delete` only if upstream now supports equivalent `.codex` / `.godex` semantics
- `fork/native-grok-legacy`
  - default: `delete` if the external backend path now fully covers the use case
  - otherwise `adapt` only for migration or retirement work
- `fork/memory-system`
  - default: `keep`
  - reopen only when sync conflicts, validation failures, or upstream replacement signals justify it
- `fork/bootstrap-residue`
  - default: `adapt`
  - do not add new product behavior here during sync
- `fork/identity-governance`, `fork/distribution-release`, `fork/maintenance-automation`
  - default: `keep`
  - review only the touched files and keep them small

## Hot-file rules

If `review-scope` reports a hot-overlap file:

1. prefer upstream behavior first
2. restore only the minimum fork-owned seam
3. do not copy old fork blocks back wholesale
4. update the manifest or ledger if ownership changed

## Required verification

After decisions are made, run the patch-group commands from:

- [godex Fork Acceptance Matrix](./godex-fork-acceptance-matrix.md)

Only run the rows that correspond to touched patch groups, plus release checks if `main` will be pushed.

## Exit condition

A sync branch is ready to merge back only when:

- every touched patch group has a `keep`, `adapt`, or `delete` decision
- the required verification rows passed
- any deleted patch group is also removed from manifest/ledger ownership
- no new anonymous fork residue was introduced
