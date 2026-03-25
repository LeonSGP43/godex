## Installing & building

### System requirements

| Requirement                 | Details                                                         |
| --------------------------- | --------------------------------------------------------------- |
| Operating systems           | macOS 12+, Ubuntu 20.04+/Debian 10+, or Windows 11 **via WSL2** |
| Git (optional, recommended) | 2.23+ for built-in PR helpers                                   |
| RAM                         | 4-GB minimum (8-GB recommended)                                 |

### DotSlash

The GitHub Release also contains a [DotSlash](https://dotslash-cli.com/) file for the Codex CLI named `codex`. Using a DotSlash file makes it possible to make a lightweight commit to source control to ensure all contributors use the same version of an executable, regardless of what platform they use for development.

### Install `godex` via npm

This fork publishes a managed npm package for the `godex` command:

```bash
npm install -g @leonsgp43/godex
```

Upgrade later with:

```bash
npm install -g @leonsgp43/godex@latest
```

### Install `godex` from the latest GitHub release

Use the release installer if you want a managed binary without cloning the repo:

```bash
curl -fsSL https://github.com/LeonSGP43/godex/releases/latest/download/install.sh | sh
```

On Windows PowerShell:

```powershell
irm https://github.com/LeonSGP43/godex/releases/latest/download/install.ps1 | iex
```

### Build from source

```bash
# Clone the repository and navigate to the root of the Cargo workspace.
git clone https://github.com/openai/codex.git
cd codex/codex-rs

# Install the Rust toolchain, if necessary.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup component add rustfmt
rustup component add clippy
# Install helper tools used by the workspace justfile:
cargo install just
# Optional: install nextest for the `just test` helper
cargo install --locked cargo-nextest

# Build Codex.
cargo build

# Launch the TUI with a sample prompt.
cargo run --bin codex -- "explain this codebase to me"

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
- installs only `godex` into a user bin directory such as `~/.local/bin`
- leaves the official `codex` command untouched
- appends the install dir to your shell profile if it is not already on `PATH`

Useful options:

```bash
# Preview without changing anything
bash scripts/install/install-godex-from-source.sh --dry-run

# Install a debug build for local development
bash scripts/install/install-godex-from-source.sh --debug

# Install into a specific directory
bash scripts/install/install-godex-from-source.sh --install-dir ~/bin

# Use a symlink instead of copying the binary
bash scripts/install/install-godex-from-source.sh --symlink
```

For the maintenance workflow after upstream merges, rebuild and reinstall with the same script.

## Tracing / verbose logging

Codex is written in Rust, so it honors the `RUST_LOG` environment variable to configure its logging behavior.

The TUI defaults to `RUST_LOG=codex_core=info,codex_tui=info,codex_rmcp_client=info` and log messages are written to `~/.codex/log/codex-tui.log` by default. For a single run, you can override the log directory with `-c log_dir=...` (for example, `-c log_dir=./.codex-log`).

```bash
tail -F ~/.codex/log/codex-tui.log
```

By comparison, the non-interactive mode (`codex exec`) defaults to `RUST_LOG=error`, but messages are printed inline, so there is no need to monitor a separate file.

See the Rust documentation on [`RUST_LOG`](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) for more information on the configuration options.
