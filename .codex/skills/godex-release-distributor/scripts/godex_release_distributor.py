#!/usr/bin/env python3
"""Run release status checks and optional push/tag steps for godex."""

from __future__ import annotations

import argparse
import json
import shlex
import subprocess
from pathlib import Path


DEFAULT_REPO = Path("/Users/leongong/Desktop/LeonProjects/codex")
DEFAULT_NPM_PACKAGE = "@leonsgp43/godex"
DEFAULT_RELEASE_REPO = "LeonSGP43/godex"


def run(cmd: list[str], *, cwd: Path, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, cwd=cwd, check=check, capture_output=True, text=True)


def run_text(cmd: list[str], *, cwd: Path, check: bool = True) -> str:
    return run(cmd, cwd=cwd, check=check).stdout.strip()


def git_status(repo: Path) -> dict[str, str | bool]:
    branch = run_text(["git", "branch", "--show-current"], cwd=repo)
    dirty = bool(run_text(["git", "status", "--short"], cwd=repo))
    return {"branch": branch, "dirty": dirty}


def version(repo: Path) -> str:
    return (repo / "VERSION").read_text(encoding="utf-8").strip()


def release_tag_name(version_value: str) -> str:
    return f"rust-v{version_value}"


def npm_version(repo: Path, package_name: str) -> tuple[bool, str]:
    completed = run(["npm", "view", package_name, "version"], cwd=repo, check=False)
    if completed.returncode == 0:
        return True, completed.stdout.strip()
    detail = completed.stderr.strip() or completed.stdout.strip()
    return False, detail


def gh_release_state(repo: Path, release_repo: str, tag_name: str) -> tuple[bool, str]:
    completed = run(
        ["gh", "release", "view", tag_name, "--repo", release_repo, "--json", "tagName,url,isPrerelease"],
        cwd=repo,
        check=False,
    )
    if completed.returncode == 0:
        return True, completed.stdout.strip()
    detail = completed.stderr.strip() or completed.stdout.strip()
    return False, detail


def preflight(repo: Path) -> None:
    subprocess.run(["bash", "scripts/godex-maintain.sh", "release-preflight"], cwd=repo, check=True)


def ensure_clean_main(repo: Path) -> None:
    status = git_status(repo)
    if status["dirty"]:
        raise SystemExit("Refusing release push with a dirty worktree.")
    if status["branch"] != "main":
        raise SystemExit(f"Refusing release push outside main; current branch is {status['branch']}.")


def cmd_status(repo: Path, package_name: str, release_repo: str) -> int:
    repo_status = git_status(repo)
    current_version = version(repo)
    tag_name = release_tag_name(current_version)
    npm_ok, npm_detail = npm_version(repo, package_name)
    gh_ok, gh_detail = gh_release_state(repo, release_repo, tag_name)
    payload = {
        "repo_root": str(repo),
        "branch": repo_status["branch"],
        "dirty": repo_status["dirty"],
        "version": current_version,
        "release_tag": tag_name,
        "npm_package": package_name,
        "npm_present": npm_ok,
        "npm_detail": npm_detail,
        "github_release_present": gh_ok,
        "github_release_detail": json.loads(gh_detail) if gh_ok else gh_detail,
    }
    print(json.dumps(payload, indent=2, ensure_ascii=False))
    return 0


def cmd_publish(repo: Path, package_name: str, release_repo: str, skip_main_push: bool, skip_tag_push: bool) -> int:
    current_version = version(repo)
    tag_name = release_tag_name(current_version)
    ensure_clean_main(repo)
    preflight(repo)

    if not skip_main_push:
        subprocess.run(["git", "push", "origin", "main"], cwd=repo, check=True)

    local_tag = run(["git", "rev-parse", "-q", "--verify", tag_name], cwd=repo, check=False)
    if local_tag.returncode != 0:
        subprocess.run(["git", "tag", "-a", tag_name, "-m", f"Release {current_version}"], cwd=repo, check=True)

    if not skip_tag_push:
        subprocess.run(["git", "push", "origin", tag_name], cwd=repo, check=True)

    print(f"Published branch/tag intent for version {current_version}.")
    print(f"Next: {shlex.join(['python3', '.codex/skills/godex-release-distributor/scripts/godex_release_distributor.py', 'verify'])}")
    return 0


def cmd_verify(repo: Path, package_name: str, release_repo: str) -> int:
    current_version = version(repo)
    tag_name = release_tag_name(current_version)
    npm_ok, npm_detail = npm_version(repo, package_name)
    gh_ok, gh_detail = gh_release_state(repo, release_repo, tag_name)

    print(f"version: {current_version}")
    print(f"release_tag: {tag_name}")
    print(f"github_release_present: {str(gh_ok).lower()}")
    print(f"npm_present: {str(npm_ok).lower()}")
    if gh_ok:
        print(f"github_release: {gh_detail}")
    else:
        print(f"github_release_error: {gh_detail}")
    if npm_ok:
        print(f"npm_version: {npm_detail}")
    else:
        print(f"npm_error: {npm_detail}")

    if gh_ok and npm_ok and npm_detail == current_version:
        print("distribution_status: ready")
        return 0
    if gh_ok and not npm_ok:
        print("distribution_status: github-release-live-npm-missing")
        return 2
    if not gh_ok and npm_ok:
        print("distribution_status: npm-live-release-missing")
        return 3
    print("distribution_status: blocked")
    return 4


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=DEFAULT_REPO)
    parser.add_argument("--npm-package", default=DEFAULT_NPM_PACKAGE)
    parser.add_argument("--release-repo", default=DEFAULT_RELEASE_REPO)
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("status")

    publish = subparsers.add_parser("publish")
    publish.add_argument("--skip-main-push", action="store_true")
    publish.add_argument("--skip-tag-push", action="store_true")

    subparsers.add_parser("verify")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo = args.repo.resolve()

    if args.command == "status":
        return cmd_status(repo, args.npm_package, args.release_repo)
    if args.command == "publish":
        return cmd_publish(repo, args.npm_package, args.release_repo, args.skip_main_push, args.skip_tag_push)
    if args.command == "verify":
        return cmd_verify(repo, args.npm_package, args.release_repo)
    raise SystemExit(f"Unknown command: {args.command}")


if __name__ == "__main__":
    raise SystemExit(main())
