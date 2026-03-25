#!/bin/sh

set -eu

REPO_ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
RUNNER="$REPO_ROOT/.codex/skills/godex-release-distributor/scripts/run.sh"

command_name="${1:-publish}"

case "$command_name" in
  publish)
    shift
    exec bash "$RUNNER" local-publish "$@"
    ;;
  stage)
    shift
    exec bash "$RUNNER" local-stage "$@"
    ;;
  status|verify)
    shift
    exec bash "$RUNNER" "$command_name" "$@"
    ;;
  local-stage|local-publish)
    shift
    exec bash "$RUNNER" "$command_name" "$@"
    ;;
  *)
    exec bash "$RUNNER" local-publish "$@"
    ;;
esac
