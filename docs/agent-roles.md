# Agent Roles

Codex supports agent roles for `spawn_agent`. A role can add a description for
the model-facing tool spec and can optionally layer extra configuration onto the
spawned sub-agent.

## Built-in roles

Current built-in roles include:

- `default`: the default spawned-agent behavior.
- `explorer`: optimized for narrow code-reading and investigation tasks.
- `worker`: optimized for execution-oriented subtasks.
- `claude-style`: a native Codex spawned-agent preset for architecture review,
  acceptance review, and tradeoff analysis.
- `grok`: a native Codex spawned-agent preset for Grok-backed research,
  planning, and synthesis.

## `claude-style`

`claude-style` is intended to feel like a concise, review-heavy architecture
partner while still running inside Codex's native spawned-agent runtime.

What it changes:

- injects review-oriented `developer_instructions`
- locks `model_reasoning_effort = "high"`

What it does not change:

- it does not switch the spawned agent to an external Claude Code runtime
- it does not force a specific model
- it does not override the current model provider

That means `spawn_agent(agent_type = "claude-style")` still creates a normal
Codex sub-agent thread, but with a stronger analysis/review posture.

## `grok`

This built-in role is for teams that want Grok to behave like a dedicated
research employee inside Codex's native spawned-agent runtime rather than as a
top-level model provider swap.

What they change:

- inject research-oriented `developer_instructions`
- push the sub-agent toward Codex's built-in `grok` tool instead of manually
  rebuilding the workflow every time
- lock reasoning effort at `high`
- inherit the native Grok preset catalog and default preset selection from the
  top-level `[grok]` config in `config.toml`

What they do not change:

- they do not switch the spawned agent to an external runtime
- they do not force a specific model provider
- they do not bundle credentials into the child; the Grok tools still require
  a reachable Grok API plus credentials in the environment

This role is designed to pair with Codex's built-in `grok` tool. The built-in
instructions tell the child to:

- use `preset = "b42"` for current facts, news, and source-heavy search
- use `preset = "thinking41"` for research planning and decomposition
- use `preset = "expert41"` for judgments, polished summaries, and reports

That means `spawn_agent(agent_type = "grok")` still creates a normal native
Codex child thread, but with a much stronger research-worker posture.

If you need to repoint the Grok gateway or remap which model each preset uses,
change the top-level `[grok]` and `[grok.presets.*]` entries in
`config.toml`; the built-in Grok roles automatically pick up those settings.

## Runtime layering

Agent roles and spawned-agent runtimes are separate concerns:

- a role controls prompt/config layering for the spawned agent
- a runtime backend controls how that spawned agent is hosted and how lifecycle
  operations such as `send_input`, `wait`, `resume`, and `close` are routed

Today, all built-in roles still run on the native Codex spawned-agent runtime.
User-defined roles selected through `agent_type` also stay on the native Codex
runtime unless the caller explicitly selects a different `backend`.

That means `[agents.<role>]` is never the place to register a new backend
runtime. A role can make a child feel like "Gemini", "Claude-style", or
"researcher", but it does not provide the actual hosting/runtime bridge for
`spawn_agent`.

Internally, Codex routes spawned-agent lifecycle operations through a
`SpawnedAgentHandle` abstraction. The default backend is still the native
`Codex` runtime, and `claude_code` remains an explicit opt-in backend.

Custom external backends are configured separately under
`[agent_backends.<name>]` in `config.toml`. Those entries can point to your own
launcher code, so you do not need to ship a built-in MCP server or a dedicated
long-running backend daemon just to make a spawned backend usable.

An external Claude Code path is available through the explicit `backend`
selector on `spawn_agent`:

```json
{
  "message": "Review this architecture and identify blockers.",
  "backend": "claude_code"
}
```

Current behavior of `backend = "claude_code"`:

- it is a separate external runtime from native Codex spawned agents
- `spawn_agent`, `send_input`, `resume_agent`, `wait_agent`, and `close_agent`
  are supported
- `fork_context` is supported by replaying parent thread history into the
  initial Claude prompt
- `model` overrides are supported
- if `model` is omitted, Codex defers to the local Claude Code CLI's default
  model selection

The same `backend` selector also accepts any configured
`[agent_backends.<name>]` id, for example:

```json
{
  "message": "Investigate this failure and summarize the root cause.",
  "agent_type": "worker",
  "backend": "gemini_worker"
}
```

In that example:

- `agent_type = "worker"` still controls the child role/persona
- `backend = "gemini_worker"` controls the external runtime bridge

A matching backend config can live in `config.toml`, for example:

```toml
[agent_backends.gemini_worker]
type = "command"
protocol = "json_stdio_v1"
command = ["python3", "backend.py"]
healthcheck = ["python3", "backend.py", "--healthcheck"]
working_dir = "/abs/path/to/.codex/backends/gemini_leonai"
default_model = "gemini-2.5-pro"
turn_timeout_seconds = 90
healthcheck_timeout_seconds = 10
max_retries = 2

[agent_backends.gemini_worker.env]
GEMINI_API_KEY = "replace-me"
GEMINI_BASE_URL = "https://apileon.leonai.top/gemini"
```

For multi-backend layout guidance and a runnable Leonai Gemini reference backend, see
[External agent backends](./external-agent-backends.md).

## Example

You can call it from `spawn_agent` like this:

```json
{
  "message": "Review this architecture and identify blockers.",
  "agent_type": "claude-style"
}
```

Or use the Grok-backed research role like this:

```json
{
  "message": "Investigate the current open-source agent frameworks and summarize the strongest options.",
  "agent_type": "grok"
}
```

## Custom roles

User-defined roles are loaded from `config.toml` and optional role files. The
minimal config shape is:

```toml
[agents.reviewer]
description = "Review role"
config_file = "./agents/reviewer.toml"
nickname_candidates = ["Atlas"]
```

And the referenced role file can provide the layered configuration:

```toml
developer_instructions = "Review carefully"
model_reasoning_effort = "high"
```

Explicitly referenced role layer files do not have to define
`developer_instructions`; they can override only the settings you need. In
practice, most useful roles will still set it.

Auto-discovered standalone role files under `agents/*.toml` are stricter: they
must define the metadata needed to stand on their own, including
`developer_instructions`.

A role may also lock other settings, such as `model`, `model_provider`, or
`profile`, if that is part of the role design.
