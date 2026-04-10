# godex Fork Acceptance Matrix

This is the patch-group verification matrix for the fork.

Run the rows that match the patch groups touched by a change or sync.

## Runtime and behavior lanes

| Patch group | Required verification | Success signal |
| --- | --- | --- |
| `fork/provider-backends` | `cargo check -p codex-core --lib --manifest-path codex-rs/Cargo.toml`; `cargo test -p codex-core spawn_agent_with_command_backend --manifest-path codex-rs/Cargo.toml -- --nocapture`; `cargo test -p codex-core command_backend_spawn_wait_and_close_round_trip --manifest-path codex-rs/Cargo.toml -- --nocapture` | external backend contract compiles and spawned-agent lifecycle still works |
| `fork/config-namespace-home` | `cargo test -p codex-cli --test godex_home --manifest-path codex-rs/Cargo.toml -- --nocapture`; `cargo test -p codex-core home_policy --manifest-path codex-rs/Cargo.toml -- --nocapture`; `godex --memory-scope project --version` | `godex` and `godex -g` policy still resolve correctly |
| `fork/native-grok-legacy` | inspect `docs/agent-roles.md` and `docs/config.md` for migration language toward `backend = "grok_worker"`; inspect Grok tool/role registration diff | native Grok remains compatibility-only and does not become the default provider lane again |
| `fork/memory-system` | `cargo test -p codex-core memories:: --manifest-path codex-rs/Cargo.toml -- --nocapture`; `cargo test -p codex-state --lib --manifest-path codex-rs/Cargo.toml`; `cargo test -p codex-app-server --tests --no-run --manifest-path codex-rs/Cargo.toml` | memory, state, and app-server validation remain green |
| `fork/bootstrap-residue` | `cargo check -p codex-cli --manifest-path codex-rs/Cargo.toml`; `cargo check -p codex-core --manifest-path codex-rs/Cargo.toml`; targeted login/TUI/MCP tests listed in `docs/godex-fork-inventory-ledger.md` | residue lanes still behave, without reopening broad refactors |

## Governance and release lanes

| Patch group | Required verification | Success signal |
| --- | --- | --- |
| `fork/identity-governance` | `godex --version`; `bash scripts/godex-maintain.sh status` | fork identity, announcement wiring, and upstream drift reporting still make sense |
| `fork/distribution-release` | `bash scripts/godex-maintain.sh release-preflight`; install dry-run if release assets changed | version, changelog, and release metadata are aligned |
| `fork/maintenance-automation` | `bash scripts/godex-maintain.sh status`; `bash scripts/godex-maintain.sh review-scope`; `bash scripts/godex-maintain.sh sync --dry-run` | maintainer workflow still reports the fork correctly and previews sync safely |

## Review rule

When a change touches more than one patch group:

1. run every touched row
2. include one sentence per row in the change summary
3. update manifest and ledger if ownership or verification changed
