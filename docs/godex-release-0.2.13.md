# godex 0.2.13 Release Notes

Release date: 2026-04-03

## Summary

`godex 0.2.13` fixes the local release-packaging path so staging `godex` npm
tarballs works on this maintainer machine's Python 3.10 runtime.

## Release Headlines

- make `godex` local release staging compatible with Python 3.10 tar handling
- keep ripgrep staging restricted to regular-file archive members
- add a focused Python unittest for the ripgrep tar extraction path

## What Changed

- `codex-cli/scripts/install_native_deps.py` now copies the requested `tar.gz`
  member directly into place instead of relying on a newer-stdlib-only
  `TarFile.extract(..., filter="data")` API
- `codex-cli/scripts/test_install_native_deps.py` adds focused coverage for
  successful ripgrep extraction and non-regular-member rejection

## Verification

- `python3 codex-cli/scripts/test_install_native_deps.py`
- `HTTP_PROXY=http://127.0.0.1:10808 HTTPS_PROXY=http://127.0.0.1:10808 ALL_PROXY=http://127.0.0.1:10808 bash .codex/skills/godex-release-distributor/scripts/run.sh local-stage`

## Distribution Status

- GitHub release flow: should be publishable after this release metadata commit
  is pushed and tagged
- npm distribution: still blocked until this machine regains valid npm auth for
  `@leonsgp43/godex`
