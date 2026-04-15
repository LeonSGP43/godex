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
FAST_RELEASE=0
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
  --fast-release      Keep release mode but use faster local build overrides.
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

  if [[ "$FAST_RELEASE" -eq 1 ]]; then
    step "Using native macOS linker for fast local release builds"
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

prepare_release_build_overrides() {
  RELEASE_BUILD_OVERRIDES=()

  if [[ "$BUILD_PROFILE" != "release" ]] || [[ "$FAST_RELEASE" -eq 0 ]]; then
    return
  fi

  step "Using fast local release overrides (lto=off, codegen-units=16)"
  RELEASE_BUILD_OVERRIDES=(
    --config 'profile.release.lto="off"'
    --config 'profile.release.codegen-units=16'
  )
}

run_logged_command() {
  local log_file="$1"
  shift

  if [[ "$DRY_RUN" -eq 1 ]]; then
    run "$@"
    return 0
  fi

  local status=0
  set +e
  "$@" 2>&1 | tee "$log_file"
  status=${PIPESTATUS[0]}
  set -e
  return "$status"
}

is_lld_duplicate_symbol_conflict() {
  local log_file="$1"
  grep -q "duplicate symbol" "$log_file" &&
    grep -q "libwebrtc_sys" "$log_file" &&
    grep -q "libv8" "$log_file"
}

build_release_binary() {
  local cargo_cmd=(
    cargo build -p codex-cli --bin godex --release
    "${RELEASE_BUILD_OVERRIDES[@]}"
    --manifest-path "$WORKSPACE_ROOT/Cargo.toml"
  )

  if [[ "${#RELEASE_BUILD_PREFIX[@]}" -eq 0 ]]; then
    run "${cargo_cmd[@]}"
    return
  fi

  local log_file
  log_file="$(mktemp -t godex-release-build)"
  TEMP_FILES+=("$log_file")

  if run_logged_command "$log_file" "${RELEASE_BUILD_PREFIX[@]}" "${cargo_cmd[@]}"; then
    return
  fi

  if ! is_lld_duplicate_symbol_conflict "$log_file"; then
    return 1
  fi

  step "Rust ld64.lld hit duplicate libwebrtc/v8 symbols; retrying with the native macOS linker"
  run "${cargo_cmd[@]}"
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
    --fast-release)
      FAST_RELEASE=1
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
prepare_release_build_overrides

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
  build_release_binary
else
  run cargo build -p codex-cli --bin godex --manifest-path "$WORKSPACE_ROOT/Cargo.toml"
fi
run mkdir -p "$INSTALL_DIR"

if [[ -L "$TARGET_BIN" ]]; then
  step "Removing existing symlink at $TARGET_BIN before install"
  run rm -f "$TARGET_BIN"
fi

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
