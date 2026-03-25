---
name: godex-release-distributor
description: Push and verify a godex release so GitHub release notices and npm-based updates can be checked end to end.
---

Use this skill when the user wants to publish the latest `godex`, push tags, verify release visibility, or check whether clients can update through npm.

Repo default: `/Users/leongong/Desktop/LeonProjects/codex`

Binding governance:

- `AGENTS.md`
- `CLAUDE.md`
- `docs/godex-fork-guidelines.md`
- `docs/godex-maintenance.md`

Core workflow:

1. Confirm repo state first:
   - `git -C /Users/leongong/Desktop/LeonProjects/codex status --short --branch`
2. Prefer the bundled one-command entrypoint:
   - `bash .codex/skills/godex-release-distributor/scripts/run.sh status`
   - `bash .codex/skills/godex-release-distributor/scripts/run.sh publish`
   - `bash .codex/skills/godex-release-distributor/scripts/run.sh verify`
3. The script should run `bash scripts/godex-maintain.sh release-preflight` before any push or tag action.
4. Required release inputs:
   - `VERSION`
   - `CHANGELOG.md`
   - release notes under `docs/`
5. This skill only publishes validated `main`.
   - never publish directly from `sync/...`
   - never publish from a dirty worktree
   - never bypass the release gate in `AGENTS.md` and `CLAUDE.md`
6. After a publish step, verify both channels separately:
   - GitHub release/tag in `LeonSGP43/godex`
   - npm registry package `@leonsgp43/godex`
7. Do not claim that npm updates work until:
   - `npm view @leonsgp43/godex version` returns the release version
8. Do not claim that in-app update notice is live until:
   - the release tag exists
   - the GitHub release exists

Important rules:

- Never push with a dirty worktree.
- Never publish from any branch other than `main`.
- Never tag a version that is not recorded in `VERSION` and `CHANGELOG.md`.
- If npm still returns `404`, say npm distribution is not ready even if GitHub release is live.
- If GitHub release is missing but npm exists, say release notice path is not fully confirmed.

Primary commands:

- `bash .codex/skills/godex-release-distributor/scripts/run.sh status`
- `bash .codex/skills/godex-release-distributor/scripts/run.sh publish`
- `bash .codex/skills/godex-release-distributor/scripts/run.sh verify`
- `python3 .codex/skills/godex-release-distributor/scripts/godex_release_distributor.py status`
- `python3 .codex/skills/godex-release-distributor/scripts/godex_release_distributor.py publish`
- `python3 .codex/skills/godex-release-distributor/scripts/godex_release_distributor.py verify`

Expected output:

- current repo version
- target release tag
- whether GitHub release exists
- whether npm package exists
- whether distribution status is ready / blocked / partially live
