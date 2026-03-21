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

## Runtime layering

Agent roles and spawned-agent runtimes are separate concerns:

- a role controls prompt/config layering for the spawned agent
- a runtime backend controls how that spawned agent is hosted and how lifecycle
  operations such as `send_input`, `wait`, `resume`, and `close` are routed

Today, all built-in roles still run on the native Codex spawned-agent runtime.
User-defined roles selected through `agent_type` also stay on the native Codex
runtime unless the caller explicitly selects a different `backend`.

Internally, Codex routes spawned-agent lifecycle operations through a
`SpawnedAgentHandle` abstraction. The default backend is still the native
`Codex` runtime, and `claude_code` remains an explicit opt-in backend.

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

## Example

You can call it from `spawn_agent` like this:

```json
{
  "message": "Review this architecture and identify blockers.",
  "agent_type": "claude-style"
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
