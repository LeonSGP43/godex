# Configuration

For basic configuration instructions, see [this documentation](https://developers.openai.com/codex/config-basic).

For advanced configuration instructions, see [this documentation](https://developers.openai.com/codex/config-advanced).

For a full configuration reference, see [this documentation](https://developers.openai.com/codex/config-reference).

## Connecting to MCP servers

Codex can connect to MCP servers configured in `~/.codex/config.toml`. See the configuration reference for the latest MCP server options:

- https://developers.openai.com/codex/config-reference

## Agent roles

Codex supports built-in and user-defined agent roles for `spawn_agent`.

- For local role behavior and examples, see [Agent roles](./agent-roles.md).
- Role configuration lives under `[agents.<role>]` in `config.toml`.
- Built-in roles can also layer embedded config files, such as
  `claude-style.toml`.
- Built-in roles currently still run on the native Codex spawned-agent runtime.

## External Claude backend command prefix

When using `spawn_agent(backend = "claude_code")`, you can configure the
launcher argv prefix in `config.toml`:

```toml
[agents.claude_code]
command = ["cps", "claude", "--dangerously-skip-permissions"]
```

This `command` value is only the prefix. Codex always appends the required
non-interactive JSON output flags automatically:

- `-p`
- `--output-format json`
- `--no-session-persistence`
- `--tools ""`

## Grok research config

The native Grok research tools and Grok-focused built-in agent roles now read
their routing defaults from `config.toml` instead of relying only on ad-hoc
environment variables.

The top-level section is `[grok]`. It controls:

- `base_origin`: Grok gateway origin, for example `https://apileon.leonai.top`
- `api_key_env`: which environment variable contains the Grok API key
- `default_preset`: default preset for the native `grok` tool
- `default_dynamic_model`: fallback model when the selected preset is dynamic

You can also override preset-specific endpoint paths and fixed models under
`[grok.presets.*]`, which makes it possible to retarget or remap the original
`grokMcp` preset catalog without code changes.

Example:

```toml
[grok]
base_origin = "https://apileon.leonai.top"
api_key_env = "GROK_API_KEY"
default_preset = "b42"
default_dynamic_model = "grok-4.20-beta"

[grok.presets.default]
path = "/grokcodex"

[grok.presets.b42]
path = "/grokcodexb42"
fixed_model = "grok-4.20-beta"

[grok.presets.expert41]
path = "/grokcodex41expert"
fixed_model = "grok-4.1-expert"

[grok.presets.thinking41]
path = "/grokcodex41thinking"
fixed_model = "grok-4.1-thinking"
```

## Memories semantic helpers

The `[memories]` section supports semantic helper controls for the generated
memory indexes, project scoping, summary truncation, and hybrid QMD recall:

For full architecture, runtime flow, and complete parameter annotations, see:

- `docs/godex-memory-system.md`

- `scope`: memory partition to use for this session (default `global`)
  - `global`: legacy shared memory root under `~/.codex/memories`
  - `project`: partition memory under `~/.codex/memories/scopes/project/<project-hash>`
- `summary_token_limit`: max tokens from `memory_summary.md` injected into
  developer instructions (default `5000`, clamped to `256..20000`)
- `semantic_index_enabled`: enable or disable generation/usage of
  `memory_index.qmd` and `vector_index.json` (default `true`)
- `semantic_recall_limit`: max number of semantic recall hints injected into
  memory developer instructions per turn (default `5`, clamped to `1..20`)
- `qmd_hybrid_enabled`: enable BM25 + vector + RRF + rerank fusion in semantic
  recall (default `true`)
- `qmd_query_expansion_enabled`: enable lightweight query expansion for hybrid
  recall (default `true`)
- `qmd_rerank_limit`: max candidates reranked in hybrid recall (default `30`,
  clamped to `1..100`)

Example:

```toml
[memories]
scope = "project"
summary_token_limit = 3000
semantic_index_enabled = true
semantic_recall_limit = 8
qmd_hybrid_enabled = true
qmd_query_expansion_enabled = true
qmd_rerank_limit = 24
```

## MCP tool approvals

Codex stores per-tool approval overrides for custom MCP servers under
`mcp_servers` in `~/.codex/config.toml`:

```toml
[mcp_servers.docs.tools.search]
approval_mode = "approve"
```

## Apps (Connectors)

Use `$` in the composer to insert a ChatGPT connector; the popover lists accessible
apps. The `/apps` command lists available and installed apps. Connected apps appear first
and are labeled as connected; others are marked as can be installed.

## Notify

Codex can run a notification hook when the agent finishes a turn. See the configuration reference for the latest notification settings:

- https://developers.openai.com/codex/config-reference

When Codex knows which client started the turn, the legacy notify JSON payload also includes a top-level `client` field. The TUI reports `codex-tui`, and the app server reports the `clientInfo.name` value from `initialize`.

## godex upstream updates

`godex` runs in two config modes:

- default `godex`: reuse Codex config locations
  - global: `~/.codex`
  - project: `.codex`
- `godex -g`: use isolated godex config locations
  - global: `~/.godex`
  - project: `.godex`

That keeps `godex` parallel to official Codex by default, while still giving
you an explicit isolated mode when needed. On first use, `godex -g` creates the
global `~/.godex` directory automatically before loading config.

Use `[godex_updates]` for the fork's own release tracking:

- `enabled`: turn godex release monitoring on or off
- `release_repo`: GitHub repo to compare against for new godex releases

Example:

```toml
[godex_updates]
enabled = true
release_repo = "LeonSGP43/godex"
```

With that in place:

- startup can show `godex update available!`
- the comparison is against the current `godex --version`, not the tracked
  upstream Codex base version
- if no automatic update action is known, the UI falls back to release notes

The top-level `[upstream_updates]` section controls two different things:

- startup notices that show how many releases behind official Codex the tracked
  upstream base is
- the `godex sync-upstream` command, which fetches, merges, and rebuilds your
  local checkout

Supported fields:

- `enabled`: turn source-based upstream monitoring on or off
- `repo_root`: absolute path to the local godex git checkout
- `remote`: tracked git remote name, usually `upstream`
- `branch`: tracked upstream branch/ref, usually `main`
- `release_repo`: GitHub repo used for release checks, usually `openai/codex`
- `build_command`: argv array used after a successful sync

Example:

```toml
[upstream_updates]
enabled = true
repo_root = "/Users/leongong/Desktop/LeonProjects/codex"
remote = "upstream"
branch = "main"
release_repo = "openai/codex"
build_command = ["cargo", "build", "-p", "codex-cli", "--bins"]
```

Recommended remote layout:

- `origin`: your godex fork
- `upstream`: the official Codex repository

With that in place:

- startup can show that `Official Codex` is several releases ahead
- `godex sync-upstream` runs `git fetch`, `git merge`, and then your configured
  build command
- dirty worktrees are rejected before merge to avoid clobbering local changes

Typical workflow:

- run `godex` when you want full compatibility with your existing Codex config
- run `godex -g` when you want separate godex-only config, trust state, and MCP
  settings

## JSON Schema

The generated JSON Schema for `config.toml` lives at `codex-rs/core/config.schema.json`.

## SQLite State DB

Codex stores the SQLite-backed state DB under `sqlite_home` (config key) or the
`CODEX_SQLITE_HOME` environment variable. When unset, WorkspaceWrite sandbox
sessions default to a temp directory; other modes default to `CODEX_HOME`.

## Custom CA Certificates

Codex can trust a custom root CA bundle for outbound HTTPS and secure websocket
connections when enterprise proxies or gateways intercept TLS. This applies to
login flows and to Codex's other external connections, including Codex
components that build reqwest clients or secure websocket clients through the
shared `codex-client` CA-loading path and remote MCP connections that use it.

Set `CODEX_CA_CERTIFICATE` to the path of a PEM file containing one or more
certificate blocks to use a Codex-specific CA bundle. If
`CODEX_CA_CERTIFICATE` is unset, Codex falls back to `SSL_CERT_FILE`. If
neither variable is set, Codex uses the system root certificates.

`CODEX_CA_CERTIFICATE` takes precedence over `SSL_CERT_FILE`. Empty values are
treated as unset.

The PEM file may contain multiple certificates. Codex also tolerates OpenSSL
`TRUSTED CERTIFICATE` labels and ignores well-formed `X509 CRL` sections in the
same bundle. If the file is empty, unreadable, or malformed, the affected Codex
HTTP or secure websocket connection reports a user-facing error that points
back to these environment variables.

## Notices

Codex stores "do not show again" flags for some UI prompts under the `[notice]` table.

## Plan mode defaults

`plan_mode_reasoning_effort` lets you set a Plan-mode-specific default reasoning
effort override. When unset, Plan mode uses the built-in Plan preset default
(currently `medium`). When explicitly set (including `none`), it overrides the
Plan preset. The string value `none` means "no reasoning" (an explicit Plan
override), not "inherit the global default". There is currently no separate
config value for "follow the global default in Plan mode".

## Realtime start instructions

`experimental_realtime_start_instructions` lets you replace the built-in
developer message Codex inserts when realtime becomes active. It only affects
the realtime start message in prompt history and does not change websocket
backend prompt settings or the realtime end/inactive message.

Ctrl+C/Ctrl+D quitting uses a ~1 second double-press hint (`ctrl + c again to quit`).
