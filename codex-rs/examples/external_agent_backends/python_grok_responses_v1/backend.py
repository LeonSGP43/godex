#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import socket
import sys
import urllib.error
import urllib.parse
import urllib.request
import uuid
from typing import Any


DEFAULT_GROK_BASE_ORIGIN = "https://apileon.leonai.top"
DEFAULT_GROK_PRESET = "b42"
DEFAULT_GROK_MODEL = "grok-4.20-beta"
DEFAULT_HTTP_TIMEOUT_SECONDS = 90
RETRYABLE_HTTP_STATUS_CODES = {408, 409, 425, 429, 500, 502, 503, 504}
PRESET_PATHS = {
    "default": "/grokcodex",
    "b42": "/grokcodexb42",
    "thinking41": "/grokcodex41thinking",
    "expert41": "/grokcodex41expert",
}
PRESET_MODELS = {
    "default": DEFAULT_GROK_MODEL,
    "b42": "grok-4.20-beta",
    "thinking41": "grok-4.1-thinking",
    "expert41": "grok-4.1-expert",
}


def emit(payload: dict[str, Any], *, exit_code: int = 0) -> int:
    sys.stdout.write(json.dumps(payload, ensure_ascii=True))
    sys.stdout.flush()
    return exit_code


def emit_error(message: str, *, code: str, retryable: bool, exit_code: int = 1) -> int:
    return emit(
        {
            "error": {
                "message": message,
                "code": code,
                "retryable": retryable,
            }
        },
        exit_code=exit_code,
    )


def non_empty_trimmed(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    trimmed = value.strip()
    return trimmed or None


def env_string(name: str, default: str | None = None) -> str | None:
    return non_empty_trimmed(os.environ.get(name)) or default


def env_int(name: str, default: int) -> int:
    raw = non_empty_trimmed(os.environ.get(name))
    if raw is None:
        return default
    try:
        value = int(raw)
    except ValueError as exc:
        raise ValueError(f"{name} must be an integer") from exc
    if value <= 0:
        raise ValueError(f"{name} must be greater than 0")
    return value


def load_request() -> dict[str, Any]:
    raw = sys.stdin.read()
    if not raw.strip():
        raise ValueError("empty stdin")
    return json.loads(raw)


def resolve_api_key() -> str | None:
    for env_name in ("GROK_API_KEY", "XAI_API_KEY", "OPENAI_API_KEY"):
        value = env_string(env_name)
        if value:
            return value
    return None


def resolve_preset() -> str:
    raw = (env_string("GROK_PRESET", DEFAULT_GROK_PRESET) or DEFAULT_GROK_PRESET).lower()
    if raw not in PRESET_PATHS:
        raise ValueError(
            f"GROK_PRESET must be one of: {', '.join(sorted(PRESET_PATHS))}"
        )
    return raw


def resolve_responses_endpoint(base_origin: str, preset_path: str) -> str:
    normalized = base_origin.rstrip("/")
    if normalized.endswith("/v1/responses"):
        return normalized
    trimmed_preset = preset_path.strip("/")
    if not trimmed_preset:
        return f"{normalized}/v1/responses"
    if normalized.endswith(f"/{trimmed_preset}"):
        return f"{normalized}/v1/responses"
    return f"{normalized}/{trimmed_preset}/v1/responses"


def render_input_item(item: dict[str, Any]) -> str:
    item_type = str(item.get("type") or "").strip().lower()
    text = non_empty_trimmed(item.get("text"))
    if text:
        return text
    if item_type in {"localimage", "local_image"}:
        path = non_empty_trimmed(item.get("path")) or "unknown"
        return f"[local_image:{path}]"
    if item_type == "image":
        url = non_empty_trimmed(item.get("image_url")) or "unknown"
        return f"[image:{url}]"
    if item_type == "mention":
        name = non_empty_trimmed(item.get("name")) or "mention"
        path = non_empty_trimmed(item.get("path")) or ""
        return f"[mention:${name}]({path})" if path else f"[mention:${name}]"
    if item_type == "skill":
        name = non_empty_trimmed(item.get("name")) or "skill"
        path = non_empty_trimmed(item.get("path")) or ""
        return f"[skill:${name}]({path})" if path else f"[skill:${name}]"
    if item_type:
        return f"[{item_type}]"
    return "[input]"


def request_items_to_text(items: Any) -> str:
    if not isinstance(items, list):
        return ""
    rendered = [
        render_input_item(item)
        for item in items
        if isinstance(item, dict)
    ]
    return "\n".join(part for part in rendered if part.strip())


def history_to_input(history: Any) -> list[dict[str, str]]:
    if not isinstance(history, list):
        return []
    rendered: list[dict[str, str]] = []
    for message in history:
        if not isinstance(message, dict):
            continue
        role = non_empty_trimmed(message.get("role")) or "user"
        content = non_empty_trimmed(message.get("content"))
        if not content:
            continue
        rendered.append({"role": role, "content": content})
    return rendered


def build_request_body(request: dict[str, Any], model: str) -> dict[str, Any]:
    input_messages = history_to_input(request.get("history"))
    current_text = request_items_to_text(request.get("items"))
    if current_text:
        input_messages.append({"role": "user", "content": current_text})
    if not input_messages:
        input_messages.append({"role": "user", "content": "Please answer the latest request."})

    body: dict[str, Any] = {
        "model": model,
        "input": input_messages,
        "stream": False,
    }

    developer_instructions = non_empty_trimmed(request.get("developer_instructions"))
    if developer_instructions:
        body["instructions"] = developer_instructions

    return body


def parse_json_bytes(raw: bytes, *, label: str) -> dict[str, Any]:
    try:
        decoded = raw.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise ValueError(f"{label} was not valid UTF-8") from exc
    try:
        value = json.loads(decoded)
    except json.JSONDecodeError as exc:
        raise ValueError(f"{label} was not valid JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be a JSON object")
    return value


def extract_upstream_error(payload: dict[str, Any]) -> tuple[str, str | None]:
    error = payload.get("error")
    if isinstance(error, dict):
        message = non_empty_trimmed(error.get("message"))
        code = non_empty_trimmed(error.get("code")) or non_empty_trimmed(error.get("type"))
        if message:
            return message, code
    message = non_empty_trimmed(payload.get("message")) or "upstream request failed"
    code = non_empty_trimmed(payload.get("code")) or non_empty_trimmed(payload.get("status"))
    return message, code


def extract_response_text(payload: dict[str, Any]) -> str:
    output_text = non_empty_trimmed(payload.get("output_text"))
    if output_text:
        return output_text

    output = payload.get("output")
    if not isinstance(output, list):
        return ""

    texts: list[str] = []
    for item in output:
        if not isinstance(item, dict):
            continue
        content = item.get("content")
        if not isinstance(content, list):
            continue
        for part in content:
            if not isinstance(part, dict):
                continue
            text = non_empty_trimmed(part.get("text"))
            if text:
                texts.append(text)
    return "\n".join(texts)


def extract_usage(payload: dict[str, Any]) -> dict[str, int]:
    usage = payload.get("usage")
    if not isinstance(usage, dict):
        return {
            "input_tokens": 0,
            "cached_input_tokens": 0,
            "output_tokens": 0,
            "reasoning_output_tokens": 0,
        }

    def read_int(key: str) -> int:
        value = usage.get(key)
        return value if isinstance(value, int) and value >= 0 else 0

    output_details = usage.get("output_tokens_details")
    reasoning_tokens = 0
    if isinstance(output_details, dict):
        value = output_details.get("reasoning_tokens")
        if isinstance(value, int) and value >= 0:
            reasoning_tokens = value

    return {
        "input_tokens": read_int("input_tokens"),
        "cached_input_tokens": read_int("cached_input_tokens"),
        "output_tokens": read_int("output_tokens"),
        "reasoning_output_tokens": reasoning_tokens,
    }


def call_grok_responses(request: dict[str, Any]) -> dict[str, Any]:
    api_key = resolve_api_key()
    if not api_key:
        raise ValueError("missing GROK_API_KEY, XAI_API_KEY, or OPENAI_API_KEY")

    preset = resolve_preset()
    preset_path = env_string("GROK_PRESET_PATH", PRESET_PATHS[preset]) or PRESET_PATHS[preset]
    base_origin = env_string("GROK_BASE_ORIGIN", DEFAULT_GROK_BASE_ORIGIN) or DEFAULT_GROK_BASE_ORIGIN
    model = (
        non_empty_trimmed(request.get("model"))
        or env_string("GROK_MODEL", PRESET_MODELS.get(preset, DEFAULT_GROK_MODEL))
        or PRESET_MODELS.get(preset, DEFAULT_GROK_MODEL)
    )
    timeout_seconds = env_int("GROK_HTTP_TIMEOUT_SECONDS", DEFAULT_HTTP_TIMEOUT_SECONDS)
    endpoint = resolve_responses_endpoint(base_origin, preset_path)
    request_body = build_request_body(request, model)

    http_request = urllib.request.Request(
        endpoint,
        data=json.dumps(request_body).encode("utf-8"),
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
            "Accept": "application/json",
            "originator": "codex-grok-worker",
        },
        method="POST",
    )

    try:
        with urllib.request.urlopen(http_request, timeout=timeout_seconds) as response:
            payload = parse_json_bytes(response.read(), label="Grok response")
    except urllib.error.HTTPError as exc:
        raw = exc.read()
        try:
            payload = parse_json_bytes(raw, label="Grok error response")
            message, status_code = extract_upstream_error(payload)
        except ValueError:
            message = f"upstream HTTP {exc.code}: {exc.reason}"
            status_code = None
        raise RuntimeError(
            json.dumps(
                {
                    "message": message,
                    "code": status_code or f"http_{exc.code}",
                    "retryable": exc.code in RETRYABLE_HTTP_STATUS_CODES,
                }
            )
        ) from exc
    except (urllib.error.URLError, TimeoutError, socket.timeout) as exc:
        raise RuntimeError(
            json.dumps(
                {
                    "message": f"network error contacting Grok: {exc}",
                    "code": "network_error",
                    "retryable": True,
                }
            )
        ) from exc

    message = extract_response_text(payload)
    usage = extract_usage(payload)
    backend_id = str(request.get("backend_id") or "grok_worker")
    session_id = request.get("session_id") or f"{backend_id}-{uuid.uuid4().hex[:12]}"

    return {
        "message": message,
        "session_id": session_id,
        "usage": usage,
        "provider_response": {
            "model": model,
            "preset": preset,
            "endpoint": endpoint,
        },
    }


def run_healthcheck() -> int:
    forced = os.environ.get("BACKEND_HEALTHCHECK_FAIL")
    if forced:
        return emit_error(
            f"forced healthcheck failure: {forced}",
            code="forced_healthcheck_failure",
            retryable=False,
        )

    api_key = resolve_api_key()
    if not api_key:
        return emit_error(
            "missing GROK_API_KEY, XAI_API_KEY, or OPENAI_API_KEY",
            code="missing_api_key",
            retryable=False,
        )

    preset = resolve_preset()
    preset_path = env_string("GROK_PRESET_PATH", PRESET_PATHS[preset]) or PRESET_PATHS[preset]
    base_origin = env_string("GROK_BASE_ORIGIN", DEFAULT_GROK_BASE_ORIGIN) or DEFAULT_GROK_BASE_ORIGIN
    model = env_string("GROK_MODEL", PRESET_MODELS.get(preset, DEFAULT_GROK_MODEL)) or PRESET_MODELS.get(
        preset, DEFAULT_GROK_MODEL
    )
    timeout_seconds = env_int("GROK_HTTP_TIMEOUT_SECONDS", DEFAULT_HTTP_TIMEOUT_SECONDS)
    endpoint = resolve_responses_endpoint(base_origin, preset_path)

    return emit(
        {
            "status": "ok",
            "python": sys.version.split()[0],
            "preset": preset,
            "model": model,
            "endpoint": endpoint,
            "timeout_seconds": timeout_seconds,
        }
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--healthcheck", action="store_true")
    args = parser.parse_args()

    try:
        if args.healthcheck:
            return run_healthcheck()

        request = load_request()
        force_retry = os.environ.get("BACKEND_FORCE_RETRY")
        if force_retry:
            return emit_error(force_retry, code="forced_retry", retryable=True)

        response = call_grok_responses(request)
        return emit(response)
    except json.JSONDecodeError as exc:
        return emit_error(f"invalid json: {exc}", code="invalid_json", retryable=False)
    except ValueError as exc:
        return emit_error(str(exc), code="invalid_request", retryable=False)
    except RuntimeError as exc:
        try:
            payload = json.loads(str(exc))
        except json.JSONDecodeError:
            return emit_error(str(exc), code="runtime_error", retryable=False)
        return emit_error(
            payload.get("message") or "runtime error",
            code=payload.get("code") or "runtime_error",
            retryable=bool(payload.get("retryable")),
        )


if __name__ == "__main__":
    raise SystemExit(main())
