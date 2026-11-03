"""按入口协议渲染错误。

每种入口协议有自己的错误体格式，任何异常都要以对应格式的 JSON 返回，
流式响应开始后出错则发一个对应格式的错误事件。
"""

from __future__ import annotations

import json
from enum import StrEnum
from typing import Any


class Protocol(StrEnum):
    ANTHROPIC = "anthropic"
    OPENAI = "openai"
    GEMINI = "gemini"


class ProxyError(Exception):
    """代理内部统一的错误，由 app 层按入口协议渲染。"""

    def __init__(self, status: int, message: str, *, upstream_body: Any = None):
        super().__init__(message)
        self.status = status
        self.message = message
        # 上游原样的错误体（已解析），渲染时若已是目标协议格式则直接透传
        self.upstream_body = upstream_body


# https://platform.claude.com/docs/en/api/errors
_ANTHROPIC_TYPES = {
    400: "invalid_request_error",
    401: "authentication_error",
    402: "billing_error",
    403: "permission_error",
    404: "not_found_error",
    413: "request_too_large",
    429: "rate_limit_error",
    500: "api_error",
    504: "timeout_error",
    529: "overloaded_error",
}

# google.rpc.Code 名称
_GOOGLE_STATUS = {
    400: "INVALID_ARGUMENT",
    401: "UNAUTHENTICATED",
    403: "PERMISSION_DENIED",
    404: "NOT_FOUND",
    409: "ABORTED",
    413: "INVALID_ARGUMENT",
    429: "RESOURCE_EXHAUSTED",
    499: "CANCELLED",
    500: "INTERNAL",
    501: "UNIMPLEMENTED",
    502: "UNAVAILABLE",
    503: "UNAVAILABLE",
    504: "DEADLINE_EXCEEDED",
}

_OPENAI_TYPES = {
    400: "invalid_request_error",
    401: "authentication_error",
    403: "permission_error",
    404: "not_found_error",
    429: "rate_limit_error",
}


def anthropic_error_type(status: int) -> str:
    if status in _ANTHROPIC_TYPES:
        return _ANTHROPIC_TYPES[status]
    return "invalid_request_error" if 400 <= status < 500 else "api_error"


def protocol_for_path(path: str) -> Protocol:
    """根据请求路径判断入口协议（用于 app 层兜底的错误渲染）。"""
    if path.startswith("/anthropic/") or path.startswith("/v1/messages"):
        return Protocol.ANTHROPIC
    if path.startswith("/gemini/") or path.startswith("/v1beta/"):
        return Protocol.GEMINI
    return Protocol.OPENAI


def extract_message(body: Any) -> str | None:
    """从各种上游错误体里取出可读的错误信息。"""
    if isinstance(body, list) and body:
        body = body[0]
    if not isinstance(body, dict):
        return None
    err = body.get("error")
    if isinstance(err, dict) and isinstance(err.get("message"), str):
        return err["message"]
    if isinstance(err, str):
        return err
    if isinstance(body.get("message"), str):
        return body["message"]
    return None


def _is_anthropic_error(body: Any) -> bool:
    return (
        isinstance(body, dict)
        and body.get("type") == "error"
        and isinstance(body.get("error"), dict)
        and "type" in body["error"]
    )


def error_body(protocol: Protocol, err: ProxyError) -> dict[str, Any]:
    if protocol is Protocol.ANTHROPIC:
        if _is_anthropic_error(err.upstream_body):
            return err.upstream_body
        return {
            "type": "error",
            "error": {"type": anthropic_error_type(err.status), "message": err.message},
        }
    if protocol is Protocol.GEMINI:
        return {
            "error": {
                "code": err.status,
                "message": err.message,
                "status": _GOOGLE_STATUS.get(err.status, "UNKNOWN"),
            }
        }
    return {
        "error": {
            "message": err.message,
            "type": _OPENAI_TYPES.get(err.status, "api_error"),
            "param": None,
            "code": err.status,
        }
    }


def stream_error_event(protocol: Protocol, err: ProxyError) -> bytes:
    """流式响应开始后出错时发给客户端的最后一个事件。

    开头的空行用来结束可能被截断的上一个事件。
    """
    payload = json.dumps(error_body(protocol, err), ensure_ascii=False)
    if protocol is Protocol.ANTHROPIC:
        return f"\n\nevent: error\ndata: {payload}\n\n".encode()
    return f"\n\ndata: {payload}\n\n".encode()


def from_upstream(status: int, raw: bytes) -> ProxyError:
    """把上游的非 2xx 响应包装成 ProxyError，保留原始错误体。"""
    try:
        body: Any = json.loads(raw) if raw else None
    except ValueError:
        body = None
    message = extract_message(body)
    if message is None:
        text = raw.decode("utf-8", errors="replace").strip()
        message = text[:2000] or f"上游返回 HTTP {status}"
    return ProxyError(status, message, upstream_body=body)
