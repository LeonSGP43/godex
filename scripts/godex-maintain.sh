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
  review-scope [range]  Show which fork patch groups and hot files a diff range touches
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

declare -a PATCH_GROUPS=(
  "fork/provider-backends"
  "fork/config-namespace-home"
  "fork/native-grok-legacy"
  "fork/memory-system"
  "fork/bootstrap-residue"
  "fork/identity-governance"
  "fork/distribution-release"
  "fork/maintenance-automation"
)

patch_group_patterns() {
  case "$1" in
    fork/provider-backends)
      cat <<'EOF'
codex-rs/core/src/agent/
codex-rs/core/src/tools/handlers/multi_agents
codex-rs/core/src/tools/spec.rs
docs/agent-roles.md
docs/external-agent-backends.md
codex-rs/examples/external_agent_backends/
EOF
      ;;
    fork/config-namespace-home)
      cat <<'EOF'
codex-rs/cli/src/main.rs
codex-rs/cli/src/root_cli_policy.rs
codex-rs/cli/tests/godex_home.rs
codex-rs/core/src/config/
codex-rs/core/src/config_loader/
codex-rs/core/config.schema.json
codex-rs/utils/home-dir/src/lib.rs
docs/config.md
EOF
      ;;
    fork/native-grok-legacy)
      cat <<'EOF'
codex-rs/core/src/agent/builtins/grok.toml
codex-rs/core/src/agent/role.rs
codex-rs/core/src/tools/handlers/grok_research.rs
codex-rs/core/src/tools/spec.rs
docs/agent-roles.md
docs/config.md
EOF
      ;;
    fork/memory-system)
      cat <<'EOF'
codex-rs/core/src/memories/
codex-rs/core/src/fork_patch/memory.rs
codex-rs/core/src/fork_patch/mod.rs
codex-rs/core/templates/memories/
codex-rs/state/src/fork_patch/
codex-rs/state/src/runtime/memories.rs
codex-rs/state/src/runtime/threads.rs
codex-rs/state/src/model/thread_metadata.rs
codex-rs/rollout/src/
codex-rs/state/migrations/0023_threads_memory_scope.sql
docs/godex-memory-system.md
docs/godex-memory-patch-layer-plan.md
docs/godex-memory-mvp-closure.md
EOF
      ;;
    fork/bootstrap-residue)
      cat <<'EOF'
codex-rs/cli/src/login.rs
codex-rs/cli/src/login_copy.rs
codex-rs/cli/src/mcp_cmd.rs
codex-rs/cli/src/mcp_copy.rs
codex-rs/login/src/
codex-rs/core/src/network_proxy_loader.rs
codex-rs/core/src/mcp_connection_manager.rs
codex-rs/core/src/mcp_connection_copy.rs
codex-rs/tui/src/app.rs
codex-rs/tui/src/app/runtime_ui.rs
codex-rs/tui/src/history_cell.rs
codex-rs/tui/src/runtime_ui_copy.rs
codex-rs/tui/src/status/
codex-rs/tui/src/slash_command.rs
EOF
      ;;
    fork/identity-governance)
      cat <<'EOF'
README.md
CHANGELOG.md
VERSION
announcement_tip.toml
docs/godex-
docs/reports/upstream-review-
codex-rs/core/src/branding.rs
codex-rs/tui/src/tooltips.rs
codex-rs/tui/src/updates.rs
codex-rs/tui/src/update_action.rs
codex-rs/tui/src/update_prompt.rs
EOF
      ;;
    fork/distribution-release)
      cat <<'EOF'
codex-cli/
.github/workflows/rust-release
scripts/install/
scripts/godex-release
scripts/stage_npm_packages.py
codex-rs/Cargo.toml
codex-rs/Cargo.lock
codex-rs/cli/Cargo.toml
codex-rs/README.md
docs/install.md
EOF
      ;;
    fork/maintenance-automation)
      cat <<'EOF'
.codex/config.toml
.codex/skills/godex-
scripts/godex-maintain.sh
docs/godex-maintenance.md
docs/godex-fork-guidelines.md
EOF
      ;;
    *)
      return 1
      ;;
  esac
}

patch_group_hot_files() {
  case "$1" in
    fork/provider-backends)
      cat <<'EOF'
codex-rs/core/src/agent/backend.rs
codex-rs/core/src/agent/mod.rs
codex-rs/core/src/agent/control.rs
codex-rs/core/src/tools/spec.rs
codex-rs/core/src/tools/handlers/multi_agents/spawn.rs
codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs
EOF
      ;;
    fork/config-namespace-home)
      cat <<'EOF'
codex-rs/cli/src/main.rs
codex-rs/core/src/config/mod.rs
codex-rs/core/src/config_loader/mod.rs
EOF
      ;;
    fork/native-grok-legacy)
      cat <<'EOF'
codex-rs/core/src/agent/role.rs
codex-rs/core/src/tools/spec.rs
EOF
      ;;
    fork/memory-system)
      cat <<'EOF'
codex-rs/cli/src/main.rs
codex-rs/core/src/config/mod.rs
codex-rs/core/src/memories/prompts.rs
codex-rs/core/src/memories/storage.rs
codex-rs/state/src/runtime/memories.rs
codex-rs/state/src/runtime/threads.rs
EOF
      ;;
    fork/bootstrap-residue)
      cat <<'EOF'
codex-rs/cli/src/login.rs
codex-rs/cli/src/mcp_cmd.rs
codex-rs/core/src/network_proxy_loader.rs
codex-rs/core/src/mcp_connection_manager.rs
codex-rs/tui/src/app.rs
codex-rs/tui/src/history_cell.rs
EOF
      ;;
    fork/identity-governance)
      cat <<'EOF'
codex-rs/core/src/branding.rs
codex-rs/tui/src/tooltips.rs
codex-rs/tui/src/updates.rs
EOF
      ;;
    fork/distribution-release)
      cat <<'EOF'
codex-rs/Cargo.toml
codex-rs/Cargo.lock
codex-rs/cli/Cargo.toml
EOF
      ;;
    fork/maintenance-automation)
      cat <<'EOF'
.codex/config.toml
scripts/godex-maintain.sh
EOF
      ;;
    *)
      return 1
      ;;
  esac
}

path_matches_pattern() {
  local file="$1"
  local pattern="$2"
  if [[ "$pattern" == */ ]]; then
    [[ "$file" == "$pattern"* ]]
  else
    [[ "$file" == "$pattern" || "$file" == "$pattern"* ]]
  fi
}

collect_matching_files() {
  local changed_file="$1"
  shift

  local pattern
  for pattern in "$@"; do
    if path_matches_pattern "$changed_file" "$pattern"; then
      return 0
    fi
  done
  return 1
}

count_nonempty_lines() {
  local text="$1"
  if [[ -z "$text" ]]; then
    printf '0\n'
    return
  fi

  printf '%s\n' "$text" | awk 'NF { count += 1 } END { print count + 0 }'
}

print_bullet_block() {
  local label="$1"
  local block="$2"
  local limit="${3:-0}"

  [[ -n "$block" ]] || return

  printf '%s\n' "$label"
  if [[ "$limit" -gt 0 ]]; then
    block="$(printf '%s\n' "$block" | sed -n "1,${limit}p")"
  fi

  while IFS= read -r item; do
    [[ -n "$item" ]] || continue
    printf '  - %s\n' "$item"
  done <<< "$block"
}

path_matches_any_block() {
  local file="$1"
  local patterns="$2"
  local pattern

  while IFS= read -r pattern; do
    [[ -n "$pattern" ]] || continue
    if path_matches_pattern "$file" "$pattern"; then
      return 0
    fi
  done <<< "$patterns"

  return 1
}

show_patch_review_scope() {
  ensure_repo

  local revspec="${1:-$UPSTREAM_REMOTE/$UPSTREAM_BRANCH...HEAD}"
  local changed_output
  if ! changed_output="$(git -C "$REPO_ROOT" diff --name-only "$revspec")"; then
    die "invalid review range: $revspec"
  fi

  step "Patch review scope"
  printf 'range: %s\n' "$revspec"

  if [[ -z "$changed_output" ]]; then
    printf 'changed_paths: 0\n'
    printf 'patch_groups: none\n'
    return
  fi

  printf 'changed_paths: %s\n' "$(count_nonempty_lines "$changed_output")"

  local group
  local touched_groups=0
  for group in "${PATCH_GROUPS[@]}"; do
    local patterns_output hot_output
    patterns_output="$(patch_group_patterns "$group")"
    hot_output="$(patch_group_hot_files "$group")"

    local matched_files=""
    local file
    while IFS= read -r file; do
      [[ -n "$file" ]] || continue
      if path_matches_any_block "$file" "$patterns_output"; then
        matched_files+="${file}"$'\n'
      fi
    done <<< "$changed_output"

    matched_files="${matched_files%$'\n'}"
    if [[ -z "$matched_files" ]]; then
      continue
    fi

    local matched_hot=""
    local hot_file
    while IFS= read -r hot_file; do
      [[ -n "$hot_file" ]] || continue
      if printf '%s\n' "$matched_files" | rg -Fxq "$hot_file"; then
        matched_hot+="${hot_file}"$'\n'
      fi
    done <<< "$hot_output"
    matched_hot="${matched_hot%$'\n'}"

    touched_groups=$((touched_groups + 1))
    printf 'patch_group: %s\n' "$group"
    printf '  touched_paths: %s\n' "$(count_nonempty_lines "$matched_files")"
    print_bullet_block '  sample_paths:' "$matched_files" 6
    print_bullet_block '  hot_overlap:' "$matched_hot" 0
  done

  if [[ "$touched_groups" -eq 0 ]]; then
    printf 'patch_groups: none matched the built-in fork inventory map\n'
  fi
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

  show_patch_review_scope "$UPSTREAM_REMOTE/$UPSTREAM_BRANCH...HEAD"
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
    review-scope)
      show_patch_review_scope "$@"
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
