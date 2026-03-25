#!/bin/sh

set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"

mode="${1:-publish}"

case "$mode" in
  local)
    shift
    command_name="${1:-publish}"
    if [ "$#" -gt 0 ]; then
      shift
    fi
    exec "$SCRIPT_DIR/godex-release-local.sh" "$command_name" "$@"
    ;;
  remote)
    shift
    command_name="${1:-publish}"
    if [ "$#" -gt 0 ]; then
      shift
    fi
    exec "$SCRIPT_DIR/godex-release-remote.sh" "$command_name" "$@"
    ;;
  stage)
    shift
    exec "$SCRIPT_DIR/godex-release-local.sh" stage "$@"
    ;;
  publish)
    shift
    exec "$SCRIPT_DIR/godex-release-local.sh" publish "$@"
    ;;
  status|verify)
    shift
    exec "$SCRIPT_DIR/godex-release-local.sh" "$mode" "$@"
    ;;
  *)
    exec "$SCRIPT_DIR/godex-release-local.sh" publish "$@"
    ;;
esac
