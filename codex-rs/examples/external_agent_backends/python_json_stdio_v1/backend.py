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


DEFAULT_GEMINI_BASE_URL = "https://apileon.leonai.top/gemini"
DEFAULT_GEMINI_MODEL = "gemini-2.5-pro"
DEFAULT_HTTP_TIMEOUT_SECONDS = 90
RETRYABLE_HTTP_STATUS_CODES = {408, 409, 425, 429, 500, 502, 503, 504}


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
    except ValueError as exc:  # pragma: no cover - defensive config guard
        raise ValueError(f"{name} must be an integer") from exc
    if value <= 0:
        raise ValueError(f"{name} must be greater than 0")
    return value


def load_request() -> dict[str, Any]:
    raw = sys.stdin.read()
    if not raw.strip():
        raise ValueError("empty stdin")
    return json.loads(raw)


def resolve_generate_content_endpoint(base_url: str, model: str) -> str:
    normalized = base_url.rstrip("/")
    encoded_model = urllib.parse.quote(model, safe="")
    if ":generateContent" in normalized:
        return normalized
    if normalized.endswith("/gemini/v1beta/models"):
        return f"{normalized}/{encoded_model}:generateContent"
    if normalized.endswith("/gemini/v1beta"):
        return f"{normalized}/models/{encoded_model}:generateContent"
    if normalized.endswith("/gemini/v1"):
        prefix = normalized[: -len("/gemini/v1")]
        return f"{prefix}/gemini/v1beta/models/{encoded_model}:generateContent"
    if normalized.endswith("/gemini"):
        return f"{normalized}/v1beta/models/{encoded_model}:generateContent"
    if normalized.endswith("/v1beta/models"):
        return f"{normalized}/{encoded_model}:generateContent"
    if normalized.endswith("/v1beta"):
        return f"{normalized}/models/{encoded_model}:generateContent"
    if "/models/" in normalized and ":" not in normalized.rsplit("/", 1)[-1]:
        return f"{normalized}:generateContent"
    if normalized.endswith("/models"):
        return f"{normalized}/{encoded_model}:generateContent"
    return f"{normalized}/{encoded_model}:generateContent"


def should_attach_leonai_compat_headers(endpoint: str) -> bool:
    try:
        parsed = urllib.parse.urlparse(endpoint)
    except ValueError:
        return False
    host = (parsed.hostname or "").lower()
    return host.endswith("apileon.leonai.top") and "/gemini" in parsed.path


def history_role_to_gemini(role: str) -> str:
    return "model" if role.strip().lower() == "assistant" else "user"


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


def history_to_contents(history: Any) -> list[dict[str, Any]]:
    if not isinstance(history, list):
        return []
    contents: list[dict[str, Any]] = []
    for message in history:
        if not isinstance(message, dict):
            continue
        text = non_empty_trimmed(message.get("content"))
        if not text:
            continue
        role = history_role_to_gemini(str(message.get("role") or "user"))
        contents.append({"role": role, "parts": [{"text": text}]})
    return contents


def build_request_body(request: dict[str, Any], model: str) -> dict[str, Any]:
    contents = history_to_contents(request.get("history"))
    current_text = request_items_to_text(request.get("items"))
    if current_text:
        contents.append({"role": "user", "parts": [{"text": current_text}]})
    if not contents:
        contents.append(
            {
                "role": "user",
                "parts": [{"text": "Please answer the latest request."}],
            }
        )

    body: dict[str, Any] = {
        "contents": contents,
        "generationConfig": {
            "temperature": 0.2,
            "topP": 0.95,
            "responseMimeType": "text/plain",
        },
    }

    developer_instructions = non_empty_trimmed(request.get("developer_instructions"))
    if developer_instructions:
        body["systemInstruction"] = {"parts": [{"text": developer_instructions}]}

    max_output_tokens = env_string("GEMINI_MAX_OUTPUT_TOKENS")
    if max_output_tokens:
        try:
            body["generationConfig"]["maxOutputTokens"] = int(max_output_tokens)
        except ValueError as exc:
            raise ValueError("GEMINI_MAX_OUTPUT_TOKENS must be an integer") from exc

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
        code = non_empty_trimmed(error.get("status")) or non_empty_trimmed(error.get("code"))
        if message:
            return message, code
    message = non_empty_trimmed(payload.get("message")) or "upstream request failed"
    code = non_empty_trimmed(payload.get("status")) or non_empty_trimmed(payload.get("code"))
    return message, code


def extract_response_text(payload: dict[str, Any]) -> str:
    candidates = payload.get("candidates")
    if not isinstance(candidates, list) or not candidates:
        return ""
    first = candidates[0]
    if not isinstance(first, dict):
        return ""
    content = first.get("content")
    if not isinstance(content, dict):
        return ""
    parts = content.get("parts")
    if not isinstance(parts, list):
        return ""
    texts: list[str] = []
    for part in parts:
        if not isinstance(part, dict):
            continue
        text = non_empty_trimmed(part.get("text"))
        if text:
            texts.append(text)
    return "\n".join(texts)


def extract_usage(payload: dict[str, Any]) -> dict[str, int]:
    usage = payload.get("usageMetadata")
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

    return {
        "input_tokens": read_int("promptTokenCount"),
        "cached_input_tokens": read_int("cachedContentTokenCount"),
        "output_tokens": read_int("candidatesTokenCount"),
        "reasoning_output_tokens": read_int("thoughtsTokenCount"),
    }


def call_leonai_gemini(request: dict[str, Any]) -> dict[str, Any]:
    api_key = env_string("GEMINI_API_KEY")
    if not api_key:
        raise ValueError("missing GEMINI_API_KEY")

    base_url = env_string("GEMINI_BASE_URL", DEFAULT_GEMINI_BASE_URL) or DEFAULT_GEMINI_BASE_URL
    model = (
        non_empty_trimmed(request.get("model"))
        or env_string("GEMINI_MODEL", DEFAULT_GEMINI_MODEL)
        or DEFAULT_GEMINI_MODEL
    )
    endpoint = resolve_generate_content_endpoint(base_url, model)
    timeout_seconds = env_int("GEMINI_HTTP_TIMEOUT_SECONDS", DEFAULT_HTTP_TIMEOUT_SECONDS)
    request_body = build_request_body(request, model)

    headers = {
        "Authorization": f"Bearer {api_key}",
        "Content-Type": "application/json",
    }
    if should_attach_leonai_compat_headers(endpoint):
        headers["User-Agent"] = "curl/8.7.1"
        headers["Accept"] = "application/json"

    http_request = urllib.request.Request(
        endpoint,
        data=json.dumps(request_body).encode("utf-8"),
        headers=headers,
        method="POST",
    )

    try:
        with urllib.request.urlopen(http_request, timeout=timeout_seconds) as response:
            payload = parse_json_bytes(response.read(), label="Gemini response")
    except urllib.error.HTTPError as exc:
        raw = exc.read()
        try:
            payload = parse_json_bytes(raw, label="Gemini error response")
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
                    "message": f"network error contacting Gemini: {exc}",
                    "code": "network_error",
                    "retryable": True,
                }
            )
        ) from exc

    message = extract_response_text(payload)
    usage = extract_usage(payload)
    backend_id = str(request.get("backend_id") or "gemini_leonai")
    session_id = request.get("session_id") or f"{backend_id}-{uuid.uuid4().hex[:12]}"

    return {
        "message": message,
        "session_id": session_id,
        "usage": usage,
        "provider_response": {"model": model},
    }


def run_healthcheck() -> int:
    forced = os.environ.get("BACKEND_HEALTHCHECK_FAIL")
    if forced:
        return emit_error(
            f"forced healthcheck failure: {forced}",
            code="forced_healthcheck_failure",
            retryable=False,
        )

    api_key = env_string("GEMINI_API_KEY")
    if not api_key:
        return emit_error(
            "missing GEMINI_API_KEY",
            code="missing_api_key",
            retryable=False,
        )

    base_url = env_string("GEMINI_BASE_URL", DEFAULT_GEMINI_BASE_URL) or DEFAULT_GEMINI_BASE_URL
    model = env_string("GEMINI_MODEL", DEFAULT_GEMINI_MODEL) or DEFAULT_GEMINI_MODEL
    timeout_seconds = env_int("GEMINI_HTTP_TIMEOUT_SECONDS", DEFAULT_HTTP_TIMEOUT_SECONDS)
    endpoint = resolve_generate_content_endpoint(base_url, model)

    return emit(
        {
            "status": "ok",
            "python": sys.version.split()[0],
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

        response = call_leonai_gemini(request)
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
