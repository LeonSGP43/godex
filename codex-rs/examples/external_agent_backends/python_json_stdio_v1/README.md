# Python JSON-STDIO v1 Leonai Gemini backend

This folder is a runnable reference backend for Codex external spawned-agent backends.

What it demonstrates:
- one process per turn
- `json_stdio_v1` request/response handling
- real HTTP calls to a Gemini `generateContent` endpoint
- Leonai-compatible request headers for `https://apileon.leonai.top/gemini`
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
- `GEMINI_API_KEY`

Optional environment:
- `GEMINI_BASE_URL` default: `https://apileon.leonai.top/gemini`
- `GEMINI_MODEL` default: `gemini-2.5-pro`
- `GEMINI_HTTP_TIMEOUT_SECONDS` default: `90`
- `GEMINI_MAX_OUTPUT_TOKENS`

Example config:

```toml
[agent_backends.gemini_leonai]
type = "command"
protocol = "json_stdio_v1"
command = ["python3", "backend.py"]
healthcheck = ["python3", "backend.py", "--healthcheck"]
working_dir = "examples/external_agent_backends/python_json_stdio_v1"
turn_timeout_seconds = 90
healthcheck_timeout_seconds = 10
max_retries = 2
supports_resume = true
supports_interrupt = false
default_model = "gemini-2.5-pro"

[agent_backends.gemini_leonai.env]
GEMINI_API_KEY = "replace-me"
GEMINI_BASE_URL = "https://apileon.leonai.top/gemini"
GEMINI_MODEL = "gemini-2.5-pro"
```

Quick smoke test:

```bash
GEMINI_API_KEY=replace-me python3 backend.py --healthcheck
printf '%s' '{"backend_id":"gemini_leonai","model":"gemini-2.5-pro","history":[],"items":[{"type":"text","text":"say hello in one sentence"}]}' | GEMINI_API_KEY=replace-me python3 backend.py
```

Notes:
1. The sample maps text history and current `items` into Gemini `contents`.
2. Non-text input items are preserved as text placeholders like `[local_image:/path]`.
3. Retryable network and upstream HTTP failures return structured `error` JSON so Codex can apply `max_retries`.
4. Keep healthchecks cheap and deterministic; this sample validates env/config without making a network call.
5. Move shared HTTP/auth helpers into `_shared/` when you add more backends.
