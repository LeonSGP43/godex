#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd -P)"
WORKSPACE_ROOT="$REPO_ROOT/codex-rs"
INSTALL_DIR="${GODEX_INSTALL_DIR:-}"
BUILD_PROFILE="release"
LINK_MODE="copy"
DRY_RUN=0
UPDATE_PATH=1
TEMP_FILES=()

cleanup() {
  local path
  # Bash 3 + `set -u` treats empty arrays as unbound in "${arr[@]}".
  # Use the default expansion to keep cleanup no-op when TEMP_FILES is empty.
  for path in "${TEMP_FILES[@]:-}"; do
    if [[ -n "$path" && -e "$path" ]]; then
      rm -f "$path"
    fi
  done
}

trap cleanup EXIT

usage() {
  cat <<'EOF'
Usage: install-godex-from-source.sh [options]

Options:
  --repo PATH         Override the godex repository root.
  --install-dir PATH  Install into PATH instead of auto-selecting a user bin dir.
  --debug             Build/install the debug binary instead of release.
  --symlink           Install via symlink instead of copying the binary.
  --copy              Install via copy. This is the default.
  --no-path           Do not append the install dir to the shell profile.
  --dry-run           Show the commands that would run without executing them.
  -h, --help          Show this help text.
EOF
}

step() {
  printf '==> %s\n' "$1"
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

run() {
  printf '> '
  printf '%q ' "$@"
  printf '\n'
  if [[ "$DRY_RUN" -eq 0 ]]; then
    "$@"
  fi
}

choose_install_dir() {
  if [[ -n "$INSTALL_DIR" ]]; then
    printf '%s\n' "$INSTALL_DIR"
    return
  fi

  if [[ -d "$HOME/.local/bin" ]] || [[ ":$PATH:" == *":$HOME/.local/bin:"* ]]; then
    printf '%s\n' "$HOME/.local/bin"
    return
  fi

  if [[ -d "$HOME/bin" ]] || [[ ":$PATH:" == *":$HOME/bin:"* ]]; then
    printf '%s\n' "$HOME/bin"
    return
  fi

  printf '%s\n' "$HOME/.local/bin"
}

resolve_profile() {
  case "${SHELL:-}" in
    */zsh) printf '%s\n' "$HOME/.zshrc" ;;
    */bash) printf '%s\n' "$HOME/.bashrc" ;;
    *) printf '%s\n' "$HOME/.profile" ;;
  esac
}

ensure_repo() {
  [[ -d "$REPO_ROOT/.git" ]] || die "not a git repo: $REPO_ROOT"
  [[ -d "$WORKSPACE_ROOT" ]] || die "missing codex-rs workspace under: $REPO_ROOT"
  command -v cargo >/dev/null 2>&1 || die "cargo is required"
}

prepare_release_build_prefix() {
  RELEASE_BUILD_PREFIX=()

  if [[ "$BUILD_PROFILE" != "release" ]] || [[ "$(uname -s)" != "Darwin" ]]; then
    return
  fi

  local llvm_clang="/opt/homebrew/opt/llvm/bin/clang"
  if [[ ! -x "$llvm_clang" ]]; then
    return
  fi

  local sdk_root
  sdk_root="$(xcrun --show-sdk-path 2>/dev/null || true)"
  if [[ -z "$sdk_root" || ! -d "$sdk_root" ]]; then
    return
  fi

  local rust_sysroot
  rust_sysroot="$(rustc --print sysroot 2>/dev/null || true)"
  local rust_host
  rust_host="$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')"
  if [[ -z "$rust_sysroot" || -z "$rust_host" ]]; then
    return
  fi

  local lld_dir="$rust_sysroot/lib/rustlib/$rust_host/bin/gcc-ld"
  if [[ ! -x "$lld_dir/ld64.lld" ]]; then
    return
  fi

  local wrapper
  wrapper="$(mktemp -t godex-clang-lld)"
  TEMP_FILES+=("$wrapper")
  cat >"$wrapper" <<EOF
#!/bin/sh
export PATH="$lld_dir:\$PATH"
SDKROOT="$sdk_root"
export SDKROOT
exec "$llvm_clang" -isysroot "\$SDKROOT" -fuse-ld=lld "\$@"
EOF
  chmod 0755 "$wrapper"

  local cargo_target_env
  cargo_target_env="CARGO_TARGET_$(printf '%s' "$rust_host" | tr '[:lower:]-' '[:upper:]_')_LINKER"
  step "Using Homebrew clang + Rust ld64.lld for macOS release compatibility"
  RELEASE_BUILD_PREFIX=(env "$cargo_target_env=$wrapper")
}

add_to_path() {
  local profile="$1"
  local dir="$2"
  local line="export PATH=\"$dir:\$PATH\""

  if [[ "$UPDATE_PATH" -eq 0 ]]; then
    step "Skipping PATH update because --no-path was requested"
    return
  fi

  if [[ ":$PATH:" == *":$dir:"* ]]; then
    step "$dir is already on PATH"
    return
  fi

  if [[ -f "$profile" ]] && grep -F "$line" "$profile" >/dev/null 2>&1; then
    step "PATH is already configured in $profile"
    return
  fi

  step "Adding $dir to PATH in $profile"
  if [[ "$DRY_RUN" -eq 1 ]]; then
    printf '> append %q to %q\n' "$line" "$profile"
    return
  fi

  {
    printf '\n# Added by godex installer\n'
    printf '%s\n' "$line"
  } >>"$profile"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      [[ $# -ge 2 ]] || die "--repo requires a path"
      REPO_ROOT="$2"
      WORKSPACE_ROOT="$REPO_ROOT/codex-rs"
      shift 2
      ;;
    --install-dir)
      [[ $# -ge 2 ]] || die "--install-dir requires a path"
      INSTALL_DIR="$2"
      shift 2
      ;;
    --debug)
      BUILD_PROFILE="debug"
      shift
      ;;
    --symlink)
      LINK_MODE="symlink"
      shift
      ;;
    --copy)
      LINK_MODE="copy"
      shift
      ;;
    --no-path)
      UPDATE_PATH=0
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

REPO_ROOT="$(cd "$REPO_ROOT" && pwd -P)"
WORKSPACE_ROOT="$REPO_ROOT/codex-rs"
INSTALL_DIR="$(choose_install_dir)"
PROFILE_FILE="$(resolve_profile)"

ensure_repo
prepare_release_build_prefix

SOURCE_BIN="$WORKSPACE_ROOT/target/$BUILD_PROFILE/godex"
TARGET_BIN="$INSTALL_DIR/godex"

if [[ -e "$TARGET_BIN" ]]; then
  INSTALL_MODE="Updating"
else
  INSTALL_MODE="Installing"
fi

step "$INSTALL_MODE godex from source"
step "Repository: $REPO_ROOT"
step "Build profile: $BUILD_PROFILE"
step "Install dir: $INSTALL_DIR"
step "Install mode: $LINK_MODE"
step "Official codex stays untouched because only $TARGET_BIN is managed"

if [[ "$BUILD_PROFILE" == "release" ]]; then
  if [[ "${#RELEASE_BUILD_PREFIX[@]}" -gt 0 ]]; then
    run "${RELEASE_BUILD_PREFIX[@]}" cargo build -p codex-cli --bin godex --release --manifest-path "$WORKSPACE_ROOT/Cargo.toml"
  else
    run cargo build -p codex-cli --bin godex --release --manifest-path "$WORKSPACE_ROOT/Cargo.toml"
  fi
else
  run cargo build -p codex-cli --bin godex --manifest-path "$WORKSPACE_ROOT/Cargo.toml"
fi
run mkdir -p "$INSTALL_DIR"

if [[ "$LINK_MODE" == "symlink" ]]; then
  run ln -sfn "$SOURCE_BIN" "$TARGET_BIN"
else
  run cp "$SOURCE_BIN" "$TARGET_BIN"
  run chmod 0755 "$TARGET_BIN"
fi

add_to_path "$PROFILE_FILE" "$INSTALL_DIR"

if [[ "$DRY_RUN" -eq 0 ]]; then
  step "Installed version: $("$TARGET_BIN" --version)"
  step "Run: godex"
else
  step "Dry run complete"
fi
