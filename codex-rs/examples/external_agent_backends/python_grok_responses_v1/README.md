# Python JSON-STDIO v1 Grok backend

This folder is a runnable reference backend for Codex external spawned-agent
backends that call a Grok-compatible Responses endpoint.

What it demonstrates:
- one process per turn
- `json_stdio_v1` request/response handling
- real HTTP calls to a Grok `/v1/responses` endpoint
- preset-to-endpoint routing through backend-local environment
- optional `--healthcheck` command
- structured retryable errors via `{"error":{"message","code","retryable"}}`
- stable `session_id` handoff for resume-capable backends

Suggested local layout when you manage many backends:
- `~/.codex/backends/_shared/` for common client/auth helpers
- `~/.codex/backends/<backend-id>/backend.py` for the runtime entrypoint
- `~/.codex/backends/<backend-id>/README.md` for provider-specific notes
- `~/.codex/config.toml` for `[agent_backends.<backend-id>]` registration

The script uses only the Python standard library. No extra package install is required.

Required environment:
- `GROK_API_KEY`, or
- `XAI_API_KEY`, or
- `OPENAI_API_KEY`

Optional environment:
- `GROK_BASE_ORIGIN` default: `https://apileon.leonai.top`
- `GROK_PRESET` default: `b42`
- `GROK_PRESET_PATH` overrides the preset path directly
- `GROK_MODEL` default: preset-dependent (`grok-4.20-beta`, `grok-4.1-thinking`, `grok-4.1-expert`)
- `GROK_HTTP_TIMEOUT_SECONDS` default: `90`

Example config:

```toml
[agent_backends.grok_worker]
type = "command"
protocol = "json_stdio_v1"
command = ["python3", "backend.py"]
healthcheck = ["python3", "backend.py", "--healthcheck"]
working_dir = "examples/external_agent_backends/python_grok_responses_v1"
turn_timeout_seconds = 90
healthcheck_timeout_seconds = 10
max_retries = 2
supports_resume = true
supports_interrupt = false
default_model = "grok-4.20-beta"

[agent_backends.grok_worker.env]
GROK_API_KEY = "replace-me"
GROK_BASE_ORIGIN = "https://apileon.leonai.top"
GROK_PRESET = "b42"
```

Quick smoke test:

```bash
GROK_API_KEY=replace-me python3 backend.py --healthcheck
printf '%s' '{"backend_id":"grok_worker","model":"grok-4.20-beta","history":[],"items":[{"type":"text","text":"say hello in one sentence"}]}' | GROK_API_KEY=replace-me python3 backend.py
```

Notes:
1. The sample maps rollout history plus current `items` into a Responses-style `input` array.
2. Backend-local env decides which Grok preset path is used, so multiple backend ids can target different presets without changing Codex core.
3. Retryable network and upstream HTTP failures return structured `error` JSON so Codex can apply `max_retries`.
4. Keep healthchecks cheap and deterministic; this sample validates env/config without making a network call.
