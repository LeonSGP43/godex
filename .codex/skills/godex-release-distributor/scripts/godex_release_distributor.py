#!/usr/bin/env python3
"""Run release status checks and optional push/tag steps for godex."""

from __future__ import annotations

import argparse
import importlib.util
import json
import shutil
import shlex
import subprocess
import tempfile
from pathlib import Path


DEFAULT_REPO = Path("/Users/leongong/Desktop/LeonProjects/codex")
DEFAULT_NPM_PACKAGE = "@leonsgp43/godex"
DEFAULT_RELEASE_REPO = "LeonSGP43/godex"
BUILD_NPM_SCRIPT = DEFAULT_REPO / "codex-cli" / "scripts" / "build_npm_package.py"
INSTALL_NATIVE_DEPS_SCRIPT = DEFAULT_REPO / "codex-cli" / "scripts" / "install_native_deps.py"


def load_module(module_name: str, script_path: Path):
    spec = importlib.util.spec_from_file_location(module_name, script_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Unable to load module from {script_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


BUILD_NPM_MODULE = load_module("godex_build_npm_package", BUILD_NPM_SCRIPT)
NATIVE_DEPS_MODULE = load_module("godex_install_native_deps", INSTALL_NATIVE_DEPS_SCRIPT)

CODEX_PLATFORM_PACKAGES = getattr(BUILD_NPM_MODULE, "CODEX_PLATFORM_PACKAGES")
RG_MANIFEST = getattr(NATIVE_DEPS_MODULE, "RG_MANIFEST")
fetch_rg = getattr(NATIVE_DEPS_MODULE, "fetch_rg")


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


def npm_auth_state(repo: Path) -> tuple[bool, str]:
    completed = run(["npm", "whoami"], cwd=repo, check=False)
    if completed.returncode == 0:
        return True, completed.stdout.strip()
    detail = completed.stderr.strip() or completed.stdout.strip()
    return False, detail


def preflight(repo: Path) -> None:
    subprocess.run(["bash", "scripts/godex-maintain.sh", "release-preflight"], cwd=repo, check=True)


def refresh_upstream_metadata(repo: Path) -> None:
    subprocess.run(["bash", "scripts/godex-maintain.sh", "refresh-upstream-metadata"], cwd=repo, check=True)


def preflight_result(repo: Path) -> tuple[bool, str]:
    completed = subprocess.run(
        ["bash", "scripts/godex-maintain.sh", "release-preflight"],
        cwd=repo,
        check=False,
        capture_output=True,
        text=True,
    )
    detail = (completed.stdout + completed.stderr).strip()
    return completed.returncode == 0, detail


def ensure_clean_main(repo: Path) -> None:
    status = git_status(repo)
    if status["dirty"]:
        raise SystemExit("Refusing release push with a dirty worktree.")
    if status["branch"] != "main":
        raise SystemExit(f"Refusing release push outside main; current branch is {status['branch']}.")


def local_platform(repo: Path) -> dict[str, str]:
    os_name = run_text(["uname", "-s"], cwd=repo)
    arch = run_text(["uname", "-m"], cwd=repo)

    if os_name == "Darwin":
        if arch == "x86_64":
            translated = run(
                ["sysctl", "-n", "sysctl.proc_translated"],
                cwd=repo,
                check=False,
            )
            if translated.returncode == 0 and translated.stdout.strip() == "1":
                arch = "arm64"
        if arch in ("arm64", "aarch64"):
            return {
                "target": "aarch64-apple-darwin",
                "package": "codex-darwin-arm64",
                "npm_tag": "darwin-arm64",
                "binary_name": "godex",
            }
        if arch == "x86_64":
            return {
                "target": "x86_64-apple-darwin",
                "package": "codex-darwin-x64",
                "npm_tag": "darwin-x64",
                "binary_name": "godex",
            }
    if os_name == "Linux":
        if arch in ("arm64", "aarch64"):
            return {
                "target": "aarch64-unknown-linux-musl",
                "package": "codex-linux-arm64",
                "npm_tag": "linux-arm64",
                "binary_name": "godex",
            }
        if arch == "x86_64":
            return {
                "target": "x86_64-unknown-linux-musl",
                "package": "codex-linux-x64",
                "npm_tag": "linux-x64",
                "binary_name": "godex",
            }
    raise SystemExit(f"Unsupported local release platform: {os_name} {arch}")


def tarball_name(package: str, release_version: str) -> str:
    if package in CODEX_PLATFORM_PACKAGES:
        platform_name = package.removeprefix("codex-")
        return f"godex-npm-{platform_name}-{release_version}.tgz"
    if package == "codex":
        return f"godex-npm-{release_version}.tgz"
    return f"{package}-npm-{release_version}.tgz"


def run_cmd_live(cmd: list[str], *, cwd: Path) -> None:
    subprocess.run(cmd, cwd=cwd, check=True)


def build_local_vendor(repo: Path, target: str, binary_name: str) -> Path:
    vendor_root = Path(tempfile.mkdtemp(prefix="godex-local-vendor-"))
    target_root = vendor_root / target
    codex_dir = target_root / "codex"
    codex_dir.mkdir(parents=True, exist_ok=True)

    binary_src = repo / "codex-rs" / "target" / target / "release" / binary_name
    if not binary_src.exists():
        raise SystemExit(f"Missing built binary for local release: {binary_src}")

    binary_dest = codex_dir / binary_name
    shutil.copy2(binary_src, binary_dest)
    binary_dest.chmod(0o755)

    fetch_rg(vendor_root, [target], manifest_path=RG_MANIFEST)
    return vendor_root


def stage_local_release(repo: Path, output_dir: Path | None = None) -> dict[str, str]:
    current_version = version(repo)
    platform = local_platform(repo)
    target = platform["target"]
    binary_name = platform["binary_name"]

    run_cmd_live(
        [
            "cargo",
            "build",
            "-p",
            "codex-cli",
            "--bin",
            "godex",
            "--release",
            "--target",
            target,
            "--manifest-path",
            "codex-rs/Cargo.toml",
        ],
        cwd=repo,
    )

    dist_dir = (output_dir or (repo / "dist" / "local-release" / current_version)).resolve()
    dist_dir.mkdir(parents=True, exist_ok=True)

    vendor_root = build_local_vendor(repo, target, binary_name)
    try:
        meta_tarball = dist_dir / tarball_name("codex", current_version)
        run_cmd_live(
            [
                "python3",
                str(BUILD_NPM_SCRIPT),
                "--package",
                "codex",
                "--release-version",
                current_version,
                "--pack-output",
                str(meta_tarball),
            ],
            cwd=repo,
        )

        platform_package = platform["package"]
        platform_tarball = dist_dir / tarball_name(platform_package, current_version)
        run_cmd_live(
            [
                "python3",
                str(BUILD_NPM_SCRIPT),
                "--package",
                platform_package,
                "--release-version",
                current_version,
                "--vendor-src",
                str(vendor_root),
                "--pack-output",
                str(platform_tarball),
            ],
            cwd=repo,
        )
    finally:
        shutil.rmtree(vendor_root, ignore_errors=True)

    return {
        "version": current_version,
        "target": target,
        "platform_package": platform["package"],
        "npm_tag": platform["npm_tag"],
        "meta_tarball": str(meta_tarball),
        "platform_tarball": str(platform_tarball),
        "dist_dir": str(dist_dir),
    }


def ensure_github_release(repo: Path, release_repo: str, tag_name: str, release_version: str) -> None:
    gh_ok, _ = gh_release_state(repo, release_repo, tag_name)
    if gh_ok:
        return
    subprocess.run(
        [
            "gh",
            "release",
            "create",
            tag_name,
            "--repo",
            release_repo,
            "--title",
            release_version,
            "--notes",
            f"godex {release_version}",
        ],
        cwd=repo,
        check=True,
    )


def publish_local_npm(repo: Path, staged: dict[str, str]) -> tuple[bool, str]:
    npm_ok, npm_detail = npm_auth_state(repo)
    if not npm_ok:
        return False, npm_detail

    subprocess.run(["npm", "publish", staged["meta_tarball"]], cwd=repo, check=True)
    subprocess.run(
        ["npm", "publish", staged["platform_tarball"], "--tag", staged["npm_tag"]],
        cwd=repo,
        check=True,
    )
    return True, "published"


def cmd_status(repo: Path, package_name: str, release_repo: str) -> int:
    repo_status = git_status(repo)
    current_version = version(repo)
    tag_name = release_tag_name(current_version)
    preflight_ok, preflight_detail = preflight_result(repo)
    npm_auth_ok, npm_auth_detail = npm_auth_state(repo)
    npm_ok, npm_detail = npm_version(repo, package_name)
    gh_ok, gh_detail = gh_release_state(repo, release_repo, tag_name)
    payload = {
        "repo_root": str(repo),
        "branch": repo_status["branch"],
        "dirty": repo_status["dirty"],
        "version": current_version,
        "release_tag": tag_name,
        "release_preflight_ok": preflight_ok,
        "release_preflight_detail": preflight_detail,
        "npm_auth_ok": npm_auth_ok,
        "npm_auth_detail": npm_auth_detail,
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
    refresh_upstream_metadata(repo)
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


def cmd_local_stage(repo: Path, output_dir: Path | None) -> int:
    staged = stage_local_release(repo, output_dir)
    print(json.dumps(staged, indent=2, ensure_ascii=False))
    return 0


def cmd_local_publish(
    repo: Path,
    package_name: str,
    release_repo: str,
    output_dir: Path | None,
    skip_main_push: bool,
    skip_tag_push: bool,
    skip_release_upload: bool,
    skip_npm_publish: bool,
) -> int:
    current_version = version(repo)
    tag_name = release_tag_name(current_version)
    ensure_clean_main(repo)
    refresh_upstream_metadata(repo)
    ensure_clean_main(repo)
    preflight(repo)

    if not skip_main_push:
        subprocess.run(["git", "push", "origin", "main"], cwd=repo, check=True)

    local_tag = run(["git", "rev-parse", "-q", "--verify", tag_name], cwd=repo, check=False)
    if local_tag.returncode != 0:
        subprocess.run(["git", "tag", "-a", tag_name, "-m", f"Release {current_version}"], cwd=repo, check=True)

    if not skip_tag_push:
        subprocess.run(["git", "push", "origin", tag_name], cwd=repo, check=True)

    staged = stage_local_release(repo, output_dir)

    if not skip_release_upload:
        ensure_github_release(repo, release_repo, tag_name, current_version)
        subprocess.run(
            [
                "gh",
                "release",
                "upload",
                tag_name,
                staged["meta_tarball"],
                staged["platform_tarball"],
                "--repo",
                release_repo,
                "--clobber",
            ],
            cwd=repo,
            check=True,
        )

    npm_result = {"npm_publish_attempted": False, "npm_publish_ok": False, "npm_publish_detail": "skipped"}
    if not skip_npm_publish:
        npm_result["npm_publish_attempted"] = True
        npm_ok, npm_detail = publish_local_npm(repo, staged)
        npm_result["npm_publish_ok"] = npm_ok
        npm_result["npm_publish_detail"] = npm_detail

    payload = {
        "version": current_version,
        "release_tag": tag_name,
        "release_repo": release_repo,
        "meta_tarball": staged["meta_tarball"],
        "platform_tarball": staged["platform_tarball"],
        "platform_target": staged["target"],
        "platform_npm_tag": staged["npm_tag"],
        **npm_result,
    }
    print(json.dumps(payload, indent=2, ensure_ascii=False))
    return 0


def cmd_verify(repo: Path, package_name: str, release_repo: str) -> int:
    current_version = version(repo)
    tag_name = release_tag_name(current_version)
    npm_auth_ok, npm_auth_detail = npm_auth_state(repo)
    npm_ok, npm_detail = npm_version(repo, package_name)
    gh_ok, gh_detail = gh_release_state(repo, release_repo, tag_name)

    print(f"version: {current_version}")
    print(f"release_tag: {tag_name}")
    print(f"github_release_present: {str(gh_ok).lower()}")
    print(f"npm_auth_ok: {str(npm_auth_ok).lower()}")
    print(f"npm_present: {str(npm_ok).lower()}")
    if gh_ok:
        print(f"github_release: {gh_detail}")
    else:
        print(f"github_release_error: {gh_detail}")
    if npm_auth_ok:
        print(f"npm_auth: {npm_auth_detail}")
    else:
        print(f"npm_auth_error: {npm_auth_detail}")
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

    local_stage = subparsers.add_parser("local-stage")
    local_stage.add_argument("--output-dir", type=Path)

    local_publish = subparsers.add_parser("local-publish")
    local_publish.add_argument("--output-dir", type=Path)
    local_publish.add_argument("--skip-main-push", action="store_true")
    local_publish.add_argument("--skip-tag-push", action="store_true")
    local_publish.add_argument("--skip-release-upload", action="store_true")
    local_publish.add_argument("--skip-npm-publish", action="store_true")

    subparsers.add_parser("verify")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo = args.repo.resolve()

    if args.command == "status":
        return cmd_status(repo, args.npm_package, args.release_repo)
    if args.command == "publish":
        return cmd_publish(repo, args.npm_package, args.release_repo, args.skip_main_push, args.skip_tag_push)
    if args.command == "local-stage":
        return cmd_local_stage(repo, args.output_dir)
    if args.command == "local-publish":
        return cmd_local_publish(
            repo,
            args.npm_package,
            args.release_repo,
            args.output_dir,
            args.skip_main_push,
            args.skip_tag_push,
            args.skip_release_upload,
            args.skip_npm_publish,
        )
    if args.command == "verify":
        return cmd_verify(repo, args.npm_package, args.release_repo)
    raise SystemExit(f"Unknown command: {args.command}")


if __name__ == "__main__":
    raise SystemExit(main())
