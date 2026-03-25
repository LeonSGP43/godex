#!/usr/bin/env python3
"""Generate a Markdown report for the upstream Codex delta against godex."""

from __future__ import annotations

import argparse
import collections
import subprocess
from dataclasses import dataclass
from datetime import date
from pathlib import Path


DEFAULT_REPO = Path("/Users/leongong/Desktop/LeonProjects/codex")
HOT_FILES = {
    "codex-rs/core/src/branding.rs",
    "codex-rs/cli/src/main.rs",
    "codex-rs/tui/src/update_action.rs",
    "codex-rs/tui_app_server/src/update_action.rs",
    "codex-cli/package.json",
    "codex-cli/bin/codex.js",
    ".github/workflows/rust-release.yml",
    "scripts/install/install.sh",
    "scripts/install/install.ps1",
    "scripts/godex-maintain.sh",
    "VERSION",
    "CHANGELOG.md",
}


@dataclass
class CommitEntry:
    sha: str
    commit_date: str
    author: str
    subject: str


def run_git(repo: Path, *args: str) -> str:
    completed = subprocess.run(
        ["git", "-C", str(repo), *args],
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout.strip()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=DEFAULT_REPO)
    parser.add_argument("--base-ref", default="main")
    parser.add_argument("--upstream-ref", default="upstream/main")
    parser.add_argument("--no-fetch", action="store_true")
    parser.add_argument("--output", type=Path)
    return parser.parse_args()


def parse_commit_log(text: str) -> list[CommitEntry]:
    commits: list[CommitEntry] = []
    for line in text.splitlines():
        if not line.strip():
            continue
        sha, commit_date, author, subject = line.split("\t", 3)
        commits.append(CommitEntry(sha=sha, commit_date=commit_date, author=author, subject=subject))
    return commits


def classify_path(path: str) -> str:
    if path.startswith("codex-rs/"):
        parts = path.split("/")
        return "/".join(parts[:2]) if len(parts) > 1 else "codex-rs"
    if path.startswith("codex-cli/"):
        return "codex-cli"
    if path.startswith(".github/"):
        return ".github"
    if path.startswith("scripts/"):
        return "scripts"
    if path.startswith("docs/"):
        return "docs"
    if "/" in path:
        return path.split("/", 1)[0]
    return path


def recommendation(behind: int, hot_file_hits: list[str], worktree_dirty: bool) -> tuple[str, str]:
    if worktree_dirty:
        return ("Hold", "The worktree is dirty, so sync should not start until local changes are settled.")
    if behind == 0:
        return ("Hold", "The fork is not behind upstream/main, so there is nothing new to absorb.")
    if hot_file_hits:
        return (
            "Sync after prep",
            "Upstream touched fork-sensitive files, so review and conflict planning should happen before merge.",
        )
    return ("Sync now", "The delta appears routine and does not overlap with the known fork-sensitive file list.")


def main() -> int:
    args = parse_args()
    repo = args.repo.resolve()
    if not repo.exists():
        raise SystemExit(f"Repository not found: {repo}")

    if not args.no_fetch:
        subprocess.run(["git", "-C", str(repo), "fetch", "upstream", "--tags"], check=True)

    branch = run_git(repo, "branch", "--show-current")
    worktree_dirty = bool(run_git(repo, "status", "--short"))
    fork_head = run_git(repo, "rev-parse", "--short", args.base_ref)
    upstream_head = run_git(repo, "rev-parse", "--short", args.upstream_ref)
    ahead_str, behind_str = run_git(
        repo, "rev-list", "--left-right", "--count", f"{args.base_ref}...{args.upstream_ref}"
    ).split()
    ahead = int(ahead_str)
    behind = int(behind_str)

    commit_log = run_git(
        repo,
        "log",
        "--reverse",
        "--date=short",
        "--format=%h\t%ad\t%an\t%s",
        f"{args.base_ref}..{args.upstream_ref}",
    )
    commits = parse_commit_log(commit_log)
    shortlog = run_git(repo, "shortlog", "-sn", f"{args.base_ref}..{args.upstream_ref}")
    diff_stat = run_git(repo, "diff", "--stat", f"{args.base_ref}...{args.upstream_ref}")
    changed_files = run_git(repo, "diff", "--name-only", f"{args.base_ref}...{args.upstream_ref}")

    buckets: collections.Counter[str] = collections.Counter()
    hot_file_hits: list[str] = []
    for path in changed_files.splitlines():
        if not path:
            continue
        buckets[classify_path(path)] += 1
        if path in HOT_FILES:
            hot_file_hits.append(path)

    top_buckets = buckets.most_common(8)
    decision, reason = recommendation(behind, hot_file_hits, worktree_dirty)
    risk = "high-risk" if worktree_dirty or hot_file_hits else ("routine" if behind <= 5 else "medium-risk")
    sync_branch = f"sync/{upstream_head}"

    output_path = args.output
    if output_path is None:
        reports_dir = repo / "docs" / "reports"
        reports_dir.mkdir(parents=True, exist_ok=True)
        output_path = reports_dir / f"upstream-review-{date.today().isoformat()}.md"
    else:
        output_path.parent.mkdir(parents=True, exist_ok=True)

    lines: list[str] = []
    lines.extend(
        [
            f"# Upstream Review - {date.today().isoformat()}",
            "",
            "## Snapshot",
            f"- repo_root: `{repo}`",
            f"- current_branch: `{branch}`",
            f"- fork_head: `{fork_head}`",
            f"- upstream_head: `{upstream_head}`",
            f"- ahead_of_upstream/main: `{ahead}`",
            f"- behind_upstream/main: `{behind}`",
            f"- worktree_dirty: `{str(worktree_dirty).lower()}`",
            "",
            "## Executive Summary",
        ]
    )

    if behind == 0:
        lines.append("The fork is not behind upstream/main, so there is no new upstream batch to review.")
    else:
        lines.append(
            f"The fork is behind upstream/main by `{behind}` commit(s). "
            f"This batch currently looks `{risk}` based on overlap with known fork-sensitive files."
        )

    lines.extend(["", "## Commit Themes"])
    if top_buckets:
        for bucket, count in top_buckets:
            lines.append(f"- `{bucket}`: {count} changed file(s)")
    else:
        lines.append("- No changed files detected in the compared range.")

    lines.extend(["", "## Representative Upstream Commits"])
    if commits:
        for entry in commits[:20]:
            lines.append(f"- `{entry.sha}` `{entry.commit_date}` {entry.author}: {entry.subject}")
        if len(commits) > 20:
            lines.append(f"- ... and {len(commits) - 20} more commit(s)")
    else:
        lines.append("- No upstream-only commits in range.")

    lines.extend(["", "## Fork Impact"])
    if hot_file_hits:
        lines.append("- Fork-sensitive overlap detected:")
        for path in hot_file_hits:
            lines.append(f"  - `{path}`")
    else:
        lines.append("- No exact overlap with the current hot-file list.")
    lines.append("- Review the fork manifest before merging any upstream batch that touches branding, install, release, or update-notice surfaces.")

    lines.extend(["", "## Author Summary"])
    if shortlog:
        for line in shortlog.splitlines():
            lines.append(f"- {line.strip()}")
    else:
        lines.append("- No upstream-only author activity in range.")

    lines.extend(["", "## Diff Stat", "```text"])
    lines.append(diff_stat or "(no diff stat)")
    lines.extend(["```", "", "## Recommendation", f"- Decision: `{decision}`", f"- Reason: {reason}"])

    lines.extend(
        [
            "",
            "## If Approved, Next Commands",
            "```bash",
            f"git checkout -b {sync_branch} main",
            "bash scripts/godex-maintain.sh sync --dry-run",
            "bash scripts/godex-maintain.sh sync",
            "bash scripts/godex-maintain.sh check",
            "bash scripts/godex-maintain.sh smoke",
            "```",
            "",
        ]
    )

    output_path.write_text("\n".join(lines), encoding="utf-8")
    print(output_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
