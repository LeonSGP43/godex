---
name: godex-upstream-reviewer
description: Fetch official Codex upstream changes, summarize what changed, and write a decision report before merging them into godex.
---

Use this skill when the user wants a read-only upstream review before deciding whether to sync `openai/codex` into this fork.

Repo default: `/Users/leongong/Desktop/LeonProjects/codex`

Binding governance:

- `AGENTS.md`
- `CLAUDE.md`
- `docs/godex-fork-guidelines.md`
- `docs/godex-fork-manifest.md`

Core workflow:

1. Confirm repo state first:
   - `git -C /Users/leongong/Desktop/LeonProjects/codex status --short --branch`
   - `bash /Users/leongong/Desktop/LeonProjects/codex/scripts/godex-maintain.sh status`
2. Prefer the bundled one-command entrypoint:
   - `bash .codex/skills/godex-upstream-reviewer/scripts/run.sh`
3. The script should:
   - fetch `upstream --tags` by default
   - compare `main..upstream/main`
   - summarize commit themes, diff stat, and fork-sensitive file overlap
   - write a Markdown report into `docs/reports/`
4. Stop after the report. Do not merge or rebase upstream in this skill.
5. If the user approves sync, do not sync on `main`. Follow the constitutional branch model:
   - create `sync/<upstream-sha-or-date>` from `main`
   - run the sync only inside that `sync/...` branch
   - validate there
   - merge validated result back into `main`
6. Hand off to repo maintenance commands only after the sync branch exists:
   - `git checkout -b sync/<upstream-sha> main`
   - `bash scripts/godex-maintain.sh sync --dry-run`
   - `bash scripts/godex-maintain.sh sync`
   - `bash scripts/godex-maintain.sh check`
   - `bash scripts/godex-maintain.sh smoke`

Important rules:

- This skill is report-first, not sync-first.
- Never recommend merging official upstream directly into `main`.
- If the worktree is dirty, call it out clearly in the report.
- Explicitly flag these hot files if upstream touches them:
  - `codex-rs/core/src/branding.rs`
  - `codex-rs/cli/src/main.rs`
  - `codex-rs/tui/src/update_action.rs`
  - `codex-rs/tui_app_server/src/update_action.rs`
  - `codex-cli/package.json`
  - `codex-cli/bin/codex.js`
  - `.github/workflows/rust-release.yml`
  - `scripts/install/install.sh`
  - `scripts/install/install.ps1`
  - `scripts/godex-maintain.sh`
  - `VERSION`
  - `CHANGELOG.md`

Primary commands:

- `bash .codex/skills/godex-upstream-reviewer/scripts/run.sh`
- `bash .codex/skills/godex-upstream-reviewer/scripts/run.sh --output /tmp/upstream-review.md`
- `python3 .codex/skills/godex-upstream-reviewer/scripts/godex_upstream_report.py`
- `python3 .codex/skills/godex-upstream-reviewer/scripts/godex_upstream_report.py --no-fetch`

Expected report sections:

- Snapshot
- Executive Summary
- Commit Themes
- Representative Upstream Commits
- Fork Impact
- Author Summary
- Diff Stat
- Recommendation
- If Approved, Next Commands
