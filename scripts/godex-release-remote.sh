#!/bin/sh

set -eu

REPO_ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
RUNNER="$REPO_ROOT/.codex/skills/godex-release-distributor/scripts/run.sh"

command_name="${1:-publish}"

case "$command_name" in
  publish|status|verify)
    shift
    exec bash "$RUNNER" "$command_name" "$@"
    ;;
  *)
    exec bash "$RUNNER" publish "$@"
    ;;
esac
