# godex 0.2.0 Release Notes

## Summary

`godex 0.2.0` introduces the first managed distribution lane for the fork.
This release makes the fork installable and upgradeable through its own npm
package family instead of sending users back to upstream `@openai/codex`.

## What Changed

- published package identity is now `@leonsgp43/godex`
- release tarballs for the fork now use the `godex-npm-*` prefix
- installer scripts now download from `LeonSGP43/godex` releases and install `godex`
- in-product npm and bun update hints now point at `@leonsgp43/godex@latest`

## Install And Update

```bash
npm install -g @leonsgp43/godex
npm install -g @leonsgp43/godex@latest
```

Or use the release installer:

```bash
curl -fsSL https://github.com/LeonSGP43/godex/releases/latest/download/install.sh | sh
```

## Why This Is 0.2.0

The fork policy reserves `0.2.0` for staged user-facing changes to install,
configuration, or update mechanisms. This release crosses that threshold by
adding a fork-owned managed install and upgrade path.

## Verification

- `bash scripts/godex-maintain.sh release-preflight`
- `python3 scripts/stage_npm_packages.py --release-version 0.2.0 --package codex --output-dir /tmp/godex-npm-stage`
- `cargo test -p codex-tui update_prompt`
