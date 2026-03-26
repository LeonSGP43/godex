# godex 0.2.6 Release Notes

Release date: 2026-03-26

## Summary

`godex 0.2.6` hardens the local release path after the `0.2.5` cut by fixing
workspace version metadata drift and keeping macOS source installs on the
standard `release` profile instead of falling back to a test-profile build.

## Release Headlines

- synchronized the checked-in Rust workspace lockfile with the active `0.2.5`
  workspace package version metadata before the new release cut
- restored the default macOS source installer path to `cargo build --release`
  by routing release linking through a local compatibility wrapper when needed
- verified that local `godex` source installs can land on a real `release`
  binary instead of depending on `snapshot-test` as an operational workaround

## What Changed

- `codex-rs/Cargo.lock` now matches the workspace package versioning expected by
  the repository release metadata
- `scripts/install/install-godex-from-source.sh` now keeps `release` as the
  default build profile on macOS and auto-detects the compatible linker path
- `docs/install.md` now documents that the source installer still targets the
  standard release profile on compatible macOS machines

## Verification

- `bash scripts/godex-maintain.sh check`
- `bash scripts/godex-maintain.sh release-preflight`
- `bash scripts/godex-release.sh status`
- `bash scripts/install/install-godex-from-source.sh --dry-run --symlink --no-path`
- local `godex --version` => `godex 0.2.5` before the version bump, followed by
  release metadata updates for `0.2.6`

## Distribution Status

- GitHub release flow: ready after version-governance checks pass and the
  maintainer pushes the release commit/tag
- npm distribution: still blocked until this machine is authenticated with npm
  and `@leonsgp43/godex` is actually published
