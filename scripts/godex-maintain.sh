#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd -P)"
WORKSPACE_ROOT="$REPO_ROOT/codex-rs"
UPSTREAM_REMOTE="upstream"
UPSTREAM_BRANCH="main"
TRACKING_BRANCH="upstream-main"

CHECK_CMD=(cargo check -p codex-cli --bin godex --manifest-path "$WORKSPACE_ROOT/Cargo.toml")
BUILD_CMD=(cargo build -p codex-cli --bin godex --release --manifest-path "$WORKSPACE_ROOT/Cargo.toml")
SMOKE_RUN_CMD=(cargo run --quiet -p codex-cli --bin godex --manifest-path "$WORKSPACE_ROOT/Cargo.toml" -- --version)

usage() {
  cat <<'EOF'
Usage: godex-maintain.sh <command> [options]

Commands:
  status                Show fork maintenance status for this repo
  sync [options]        Fetch official Codex, refresh upstream-main, merge into current branch, and rebuild
  check                 Run cargo check for the godex CLI
  smoke                 Verify a runnable godex binary reports its version
  release-preflight     Validate VERSION/CHANGELOG alignment and main push readiness

Sync options:
  --dry-run             Print planned commands without executing them
  --ff-only             Require merge to fast-forward
  --no-build            Skip the rebuild step after merge
EOF
}

step() {
  printf '==> %s\n' "$1"
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

run_cmd() {
  printf '> '
  printf '%q ' "$@"
  printf '\n'
  "$@"
}

run_or_print() {
  local dry_run="$1"
  shift
  if [[ "$dry_run" -eq 1 ]]; then
    printf '> '
    printf '%q ' "$@"
    printf '\n'
    return
  fi
  run_cmd "$@"
}

ensure_repo() {
  [[ -d "$REPO_ROOT/.git" ]] || die "not a git repo: $REPO_ROOT"
  [[ -d "$WORKSPACE_ROOT" ]] || die "missing codex-rs workspace under: $REPO_ROOT"
  command -v git >/dev/null 2>&1 || die "git is required"
}

ensure_clean_worktree() {
  local status_output
  status_output="$(git -C "$REPO_ROOT" status --short)"
  if [[ -n "$status_output" ]]; then
    die "worktree is dirty; commit or stash changes first"
  fi
}

ensure_tracking_branch() {
  local dry_run="$1"
  if git -C "$REPO_ROOT" show-ref --verify --quiet "refs/heads/$TRACKING_BRANCH"; then
    run_or_print "$dry_run" git -C "$REPO_ROOT" branch -f "$TRACKING_BRANCH" "$UPSTREAM_REMOTE/$UPSTREAM_BRANCH"
  else
    run_or_print "$dry_run" git -C "$REPO_ROOT" branch "$TRACKING_BRANCH" "$UPSTREAM_REMOTE/$UPSTREAM_BRANCH"
  fi
}

show_repo_status() {
  ensure_repo

  step "Refreshing upstream refs"
  run_cmd git -C "$REPO_ROOT" fetch "$UPSTREAM_REMOTE" --tags

  local current_branch
  current_branch="$(git -C "$REPO_ROOT" branch --show-current)"

  local left_right
  left_right="$(git -C "$REPO_ROOT" rev-list --left-right --count HEAD..."$UPSTREAM_REMOTE/$UPSTREAM_BRANCH")"

  local ahead behind
  ahead="${left_right%%$'\t'*}"
  behind="${left_right##*$'\t'}"

  local worktree_state="clean"
  if [[ -n "$(git -C "$REPO_ROOT" status --short)" ]]; then
    worktree_state="dirty"
  fi

  local version
  version="$(tr -d '\n' < "$REPO_ROOT/VERSION")"

  step "Repository"
  printf 'repo_root: %s\n' "$REPO_ROOT"
  printf 'current_branch: %s\n' "$current_branch"
  printf 'worktree: %s\n' "$worktree_state"
  printf 'version: %s\n' "$version"

  step "Remotes"
  printf 'origin: %s\n' "$(git -C "$REPO_ROOT" remote get-url origin)"
  printf 'upstream: %s\n' "$(git -C "$REPO_ROOT" remote get-url "$UPSTREAM_REMOTE")"

  step "Upstream drift"
  printf 'ahead_of_%s/%s: %s\n' "$UPSTREAM_REMOTE" "$UPSTREAM_BRANCH" "$ahead"
  printf 'behind_%s/%s: %s\n' "$UPSTREAM_REMOTE" "$UPSTREAM_BRANCH" "$behind"

  if git -C "$REPO_ROOT" show-ref --verify --quiet "refs/heads/$TRACKING_BRANCH"; then
    printf 'tracking_branch: %s present\n' "$TRACKING_BRANCH"
  else
    printf 'tracking_branch: %s missing\n' "$TRACKING_BRANCH"
  fi

  if [[ -f "$REPO_ROOT/.codex/config.toml" ]]; then
    printf 'project_config: .codex/config.toml present\n'
  else
    printf 'project_config: .codex/config.toml missing\n'
  fi

  if command -v godex >/dev/null 2>&1; then
    printf 'godex_on_path: %s\n' "$(command -v godex)"
    printf 'godex_version: %s\n' "$(godex --version)"
  else
    printf 'godex_on_path: missing\n'
  fi
}

sync_upstream() {
  ensure_repo

  local dry_run=0
  local ff_only=0
  local no_build=0

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --dry-run)
        dry_run=1
        ;;
      --ff-only)
        ff_only=1
        ;;
      --no-build)
        no_build=1
        ;;
      *)
        die "unknown sync option: $1"
        ;;
    esac
    shift
  done

  if [[ "$dry_run" -eq 0 ]]; then
    ensure_clean_worktree
  fi

  step "Syncing upstream"
  run_or_print "$dry_run" git -C "$REPO_ROOT" fetch "$UPSTREAM_REMOTE" --tags
  ensure_tracking_branch "$dry_run"

  local merge_cmd=(git -C "$REPO_ROOT" merge --no-edit)
  if [[ "$ff_only" -eq 1 ]]; then
    merge_cmd+=(--ff-only)
  fi
  merge_cmd+=("$UPSTREAM_REMOTE/$UPSTREAM_BRANCH")
  run_or_print "$dry_run" "${merge_cmd[@]}"

  if [[ "$no_build" -eq 0 ]]; then
    run_or_print "$dry_run" "${BUILD_CMD[@]}"
  fi

  if [[ "$dry_run" -eq 0 ]]; then
    step "Post-sync drift"
    run_cmd git -C "$REPO_ROOT" rev-list --left-right --count HEAD..."$UPSTREAM_REMOTE/$UPSTREAM_BRANCH"
  fi
}

run_check() {
  ensure_repo
  step "Running cargo check"
  run_cmd "${CHECK_CMD[@]}"
}

run_smoke() {
  ensure_repo
  step "Running smoke check"
  if [[ -x "$WORKSPACE_ROOT/target/release/godex" ]]; then
    run_cmd "$WORKSPACE_ROOT/target/release/godex" --version
    return
  fi
  run_cmd "${SMOKE_RUN_CMD[@]}"
}

run_release_preflight() {
  ensure_repo

  step "Checking release metadata"
  [[ -f "$REPO_ROOT/CHANGELOG.md" ]] || die "missing CHANGELOG.md"
  [[ -f "$REPO_ROOT/VERSION" ]] || die "missing VERSION"

  local version
  version="$(tr -d '\n' < "$REPO_ROOT/VERSION")"
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "VERSION is not SemVer: $version"

  rg -q '^## \[Unreleased\]' "$REPO_ROOT/CHANGELOG.md" || die "CHANGELOG.md missing [Unreleased]"
  rg -q '^## \[[0-9]+\.[0-9]+\.[0-9]+\]' "$REPO_ROOT/CHANGELOG.md" || die "CHANGELOG.md missing released version heading"

  local workspace_version
  workspace_version="$(
    awk '
      /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
      /^\[/ { in_workspace_package = 0 }
      in_workspace_package && /^version = "/ {
        gsub(/^version = "/, "", $0)
        gsub(/"$/, "", $0)
        print
        exit
      }
    ' "$WORKSPACE_ROOT/Cargo.toml"
  )"
  [[ -n "$workspace_version" ]] || die "failed to read workspace.package version"
  [[ "$workspace_version" == "$version" ]] || die "VERSION ($version) does not match codex-rs/Cargo.toml ($workspace_version)"
  rg -q "^## \[$version\]" "$REPO_ROOT/CHANGELOG.md" || die "CHANGELOG.md missing release heading for VERSION [$version]"

  local current_branch
  current_branch="$(git -C "$REPO_ROOT" branch --show-current)"
  if [[ "$current_branch" == "main" ]] && git -C "$REPO_ROOT" show-ref --verify --quiet "refs/remotes/origin/main"; then
    local left_right
    left_right="$(git -C "$REPO_ROOT" rev-list --left-right --count origin/main...HEAD)"
    local ahead
    ahead="${left_right##*$'\t'}"

    if [[ "$ahead" != "0" ]]; then
      local remote_version
      remote_version="$(git -C "$REPO_ROOT" show origin/main:VERSION 2>/dev/null || true)"
      remote_version="${remote_version//$'\n'/}"

      if [[ -n "$remote_version" && "$remote_version" == "$version" ]]; then
        die "main is ahead of origin/main but VERSION is still $version; bump VERSION before push"
      fi

      local unreleased_block
      unreleased_block="$(
        awk '
          /^## \[Unreleased\]$/ { in_unreleased = 1; next }
          /^## \[/ {
            if (in_unreleased) {
              exit
            }
          }
          in_unreleased { print }
        ' "$REPO_ROOT/CHANGELOG.md"
      )"
      if printf '%s\n' "$unreleased_block" | rg -q '^- '; then
        die "CHANGELOG.md still has entries under [Unreleased]; move them into [$version] before pushing main"
      fi
    fi
  fi

  step "Release preflight passed"
  printf 'version: %s\n' "$version"
}

main() {
  local cmd="${1:-status}"
  if [[ $# -gt 0 ]]; then
    shift
  fi

  case "$cmd" in
    status)
      show_repo_status
      ;;
    sync)
      sync_upstream "$@"
      ;;
    check)
      run_check
      ;;
    smoke)
      run_smoke
      ;;
    release-preflight)
      run_release_preflight
      ;;
    -h|--help|help)
      usage
      ;;
    *)
      usage
      die "unknown command: $cmd"
      ;;
  esac
}

main "$@"
