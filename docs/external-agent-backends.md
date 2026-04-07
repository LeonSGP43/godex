# External Agent Backends

External spawned-agent backends let `spawn_agent` talk to real provider runtimes that live outside the Codex binary.

## Best-practice layout

When you expect to maintain multiple backends, keep runtime code outside `config.toml` and organize it like this:

- `~/.codex/backends/_shared/`: shared HTTP/auth/client helpers
- `~/.codex/backends/<backend-id>/backend.py`: the backend entrypoint
- `~/.codex/backends/<backend-id>/README.md`: provider-specific setup notes
- `~/.codex/config.toml`: `[agent_backends.<backend-id>]` registrations only

That split keeps role config, runtime config, and provider implementation separate:

- `agent_type`: prompt/persona layer
- `backend`: runtime bridge selection
- backend code on disk: actual provider call logic

## Runtime contract

`protocol = "json_stdio_v1"` launches one external process per turn.

Codex sends one JSON request on stdin with fields such as:

- `backend_id`
- `thread_id`
- `cwd`
- `model`
- `reasoning_effort`
- `developer_instructions`
- `session_id`
- `history`
- `items`

The backend should return one JSON object on stdout.

Success shape:

```json
{
  "message": "final assistant text",
  "session_id": "backend-session-id",
  "usage": {
    "input_tokens": 10,
    "cached_input_tokens": 0,
    "output_tokens": 20,
    "reasoning_output_tokens": 5
  }
}
```

Structured error shape:

```json
{
  "error": {
    "message": "upstream rate limited",
    "code": "rate_limited",
    "retryable": true
  }
}
```

`retryable = true` is what allows Codex to use the configured retry budget for transient failures.

## Backend config knobs

These live under `[agent_backends.<name>]`:

- `command`: argv for the turn handler
- `working_dir`: optional process working directory; relative paths resolve from the spawned thread `cwd`
- `[agent_backends.<name>.env]`: backend-only environment overrides
- `healthcheck`: optional argv run before each turn
- `healthcheck_timeout_seconds`: timeout for the healthcheck command
- `turn_timeout_seconds`: timeout for the main turn process
- `max_retries`: retry budget for retryable turn failures and retryable healthcheck failures
- `supports_resume`: whether Codex should persist backend-owned `session_id`
- `supports_interrupt`: whether Codex may expose interrupt for that backend

## Reference backends

Copyable provider samples live under
`codex-rs/examples/external_agent_backends/`.

Current references:

- `python_json_stdio_v1/`
  - real Leonai Gemini `generateContent` HTTP call
  - intended runtime id: `backend = "gemini_worker"`
- `python_grok_responses_v1/`
  - real Grok-compatible `/v1/responses` HTTP call
  - intended runtime id: `backend = "grok_worker"`

They demonstrate:

- request parsing
- stable response JSON
- structured retryable errors
- `--healthcheck` handling
- a clean place to factor shared client code when you add more backends

The Gemini sample expects:

- `GEMINI_API_KEY`
- optional `GEMINI_BASE_URL` (defaults to `https://apileon.leonai.top/gemini`)
- optional `GEMINI_MODEL` (defaults to `gemini-2.5-pro`)
- optional `GEMINI_HTTP_TIMEOUT_SECONDS`

The Grok sample expects:

- `GROK_API_KEY`, `XAI_API_KEY`, or `OPENAI_API_KEY`
- optional `GROK_BASE_ORIGIN` (defaults to `https://apileon.leonai.top`)
- optional `GROK_PRESET` (defaults to `b42`)
- optional `GROK_PRESET_PATH`
- optional `GROK_MODEL`
- optional `GROK_HTTP_TIMEOUT_SECONDS`

Both samples use the Python standard library only, so they are easy to copy
into `~/.codex/backends/<backend-id>/backend.py` and run immediately.
