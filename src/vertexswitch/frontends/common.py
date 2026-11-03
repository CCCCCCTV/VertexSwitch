"""各入口协议共用的小工具。"""

from __future__ import annotations

import logging
from collections.abc import AsyncIterator, Callable
from typing import Any

import httpx
from fastapi import Request

from ..core.errors import Protocol, ProxyError, stream_error_event

logger = logging.getLogger(__name__)


async def read_json_object(request: Request) -> dict[str, Any]:
    try:
        body = await request.json()
    except (ValueError, UnicodeDecodeError) as e:
        raise ProxyError(400, f"请求体不是合法的 JSON：{e}") from e
    if not isinstance(body, dict):
        raise ProxyError(400, "请求体必须是 JSON 对象")
    return body


async def relay(
    response: httpx.Response,
    protocol: Protocol,
    on_chunk: Callable[[bytes], None] | None = None,
) -> AsyncIterator[bytes]:
    """原样转发上游流；中途出错时按入口协议补发一个错误事件后结束。"""
    try:
        async for chunk in response.aiter_bytes():
            if on_chunk is not None:
                on_chunk(chunk)
            yield chunk
    except httpx.ReadTimeout:
        logger.warning("上游流式响应空闲超时")
        yield stream_error_event(protocol, ProxyError(504, "Vertex AI 流式响应空闲超时"))
    except httpx.HTTPError as e:
        logger.warning("上游流式响应中断：%r", e)
        yield stream_error_event(
            protocol, ProxyError(502, f"Vertex AI 流式响应中断：{type(e).__name__}")
        )
    finally:
        await response.aclose()
