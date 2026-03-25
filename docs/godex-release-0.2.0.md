# godex 0.2.0 Release Notes

Release date: 2026-03-25

## Summary

`godex 0.2.0` absorbs the official Codex upstream baseline at `e590fad50b83`
while keeping the fork constitutional layer intact. This release is the first
large upstream catch-up performed under the new fork-governance model.

## Release Headlines

- synced `main` to upstream `openai/codex` commit `e590fad50b83`
- preserved `godex` config namespace behavior and fork release identity
- kept fork divergence bounded to the manifest-listed areas
- added a hard pre-push version gate for future `main` releases

## Upstream Changes Pulled In

The absorbed upstream range is dominated by:

- plugin and marketplace expansion
- app-server v2 and protocol growth
- multi-agent v2 refactors and new list/watcher behavior
- sandboxing and platform compatibility improvements
- new code-mode and `v8-poc` runtime groundwork
- CI and release workflow changes, including zsh release artifacts

## Fork Decisions During Sync

Retained fork behavior:

- default `godex` keeps Codex-compatible config roots
- `godex -g` keeps isolated `.godex` roots
- fork release/version governance remains separate from upstream release tracking

Dropped old fork-only behavior:

- smart-approvals alias migration logic
- external backend path in legacy `multi_agents` spawn handling

## Verification

The synchronized branch and the post-sync cleanup were validated with:

- `bash scripts/godex-maintain.sh status`
- `bash scripts/godex-maintain.sh sync --dry-run`
- `bash scripts/godex-maintain.sh check`
- `bash scripts/godex-maintain.sh smoke`
- `bash scripts/godex-maintain.sh release-preflight`

Observed runtime result:

- `godex --version` => `godex 0.2.0`

## Follow-Up

Future upstream absorption should keep using:

- `upstream-main` as the read-only upstream mirror
- `sync/<upstream-sha-or-date>` as the only integration branch
- `main` as the validated release line
