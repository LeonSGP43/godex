## Installing & building

### System requirements

| Requirement                 | Details                                                         |
| --------------------------- | --------------------------------------------------------------- |
| Operating systems           | macOS 12+, Ubuntu 20.04+/Debian 10+, or Windows 11 **via WSL2** |
| Git (optional, recommended) | 2.23+ for built-in PR helpers                                   |
| RAM                         | 4-GB minimum (8-GB recommended)                                 |

### Current channel status

Treat release channels by observed availability, not by assumption:

- Reliable today: build and install `godex` from your local source checkout.
- GitHub Releases: use them as release history and public release signaling.
- npm and managed installers: only use them when the specific release or package is visibly published for that version.

If you are maintaining the fork, verify release readiness first:

```bash
bash scripts/godex-maintain.sh release-preflight
```

### Build from source

```bash
# Clone the fork and navigate to the root of the Cargo workspace.
git clone https://github.com/LeonSGP43/godex.git
cd godex/codex-rs

# Install the Rust toolchain, if necessary.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup component add rustfmt
rustup component add clippy
# Install helper tools used by the workspace justfile:
cargo install just
# Optional: install nextest for the `just test` helper
cargo install --locked cargo-nextest

# Build godex.
cargo build

# Launch the TUI with a sample prompt.
cargo run --bin godex -- "explain this codebase to me"

# After making changes, use the root justfile helpers (they default to codex-rs):
just fmt
just fix -p <crate-you-touched>

# Run the relevant tests (project-specific is fastest), for example:
cargo test -p codex-tui
# If you have cargo-nextest installed, `just test` runs the test suite via nextest:
just test
# Avoid `--all-features` for routine local runs because it increases build
# time and `target/` disk usage by compiling additional feature combinations.
# If you specifically want full feature coverage, use:
cargo test --all-features
```

### Install `godex` from your source checkout

This fork keeps `godex` parallel to official `codex` instead of replacing it.

```bash
cd /path/to/your/godex/repo
bash scripts/install/install-godex-from-source.sh
```

What the installer does:

- builds `codex-rs` with `cargo build -p codex-cli --bin godex --release`
- on macOS, first tries a local Homebrew-LLVM plus Rust-LLD wrapper when available, but automatically falls back to the native macOS linker if that path hits the known `libwebrtc`/`v8` duplicate-symbol conflict during release linking
- installs only `godex` into a user bin directory such as `~/.local/bin`
- leaves the official `codex` command untouched
- appends the install dir to your shell profile if it is not already on `PATH`

Useful options:

```bash
# Preview without changing anything
bash scripts/install/install-godex-from-source.sh --dry-run

# Install a debug build for local development
bash scripts/install/install-godex-from-source.sh --debug

# Install a faster local release build when the default fat-LTO link is too slow
bash scripts/install/install-godex-from-source.sh --fast-release

# Install into a specific directory
bash scripts/install/install-godex-from-source.sh --install-dir ~/bin

# Use a symlink instead of copying the binary
bash scripts/install/install-godex-from-source.sh --symlink
```

`--fast-release` keeps the install on the `release` path but applies local-only
Cargo overrides (`lto=off`, `codegen-units=16`) and uses the native macOS
linker so developer installs finish faster. Use the default release path when
you need artifact parity with the repository's official release profile.

For the maintenance workflow after upstream merges, rebuild and reinstall with the same script.

### Optional managed channels

The fork keeps room for GitHub-release installers and npm distribution, but these are release-state dependent.

Use them only when the current release explicitly ships them:

- GitHub release page: `https://github.com/LeonSGP43/godex/releases/latest`
- npm package page: verify that `@leonsgp43/godex` exists before documenting or relying on `npm install -g`

That keeps the public install docs honest while the fork's release pipeline continues to mature.

## Tracing / verbose logging

Codex is written in Rust, so it honors the `RUST_LOG` environment variable to configure its logging behavior.

The TUI defaults to `RUST_LOG=codex_core=info,codex_tui=info,codex_rmcp_client=info` and log messages are written to `~/.codex/log/codex-tui.log` by default. For a single run, you can override the log directory with `-c log_dir=...` (for example, `-c log_dir=./.codex-log`).

```bash
tail -F ~/.codex/log/codex-tui.log
```

By comparison, the non-interactive mode (`codex exec`) defaults to `RUST_LOG=error`, but messages are printed inline, so there is no need to monitor a separate file.

See the Rust documentation on [`RUST_LOG`](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) for more information on the configuration options.
