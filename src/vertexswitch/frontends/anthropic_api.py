"""Anthropic Messages 入口：``/v1/messages``、``/v1/messages/count_tokens``。

请求原样转给 Vertex 上的 Claude（只改写 model / anthropic_version），
响应（含 SSE 流）原样回传。流开始后出错时补发一个 ``event: error``。
"""

from __future__ import annotations

import json
import logging
from typing import Any

from fastapi import APIRouter, Request, Response
from fastapi.responses import StreamingResponse

from ..core.context import AppContext
from ..core.errors import Protocol, ProxyError, from_upstream
from ..core.registry import CLAUDE
from .common import read_json_object, relay

logger = logging.getLogger(__name__)

PREFIXES = ("", "/anthropic")
# 透传给 Vertex 的客户端请求头
_FORWARD_HEADERS = ("anthropic-beta",)


def _require_model(ctx: AppContext, body: dict[str, Any]) -> str:
    model = body.get("model")
    if not isinstance(model, str) or not model.strip():
        raise ProxyError(400, "model: 缺少模型名")
    route = ctx.registry.resolve(model)
    if route is None or route.backend != CLAUDE:
        raise ProxyError(404, f"model: 未知的 Claude 模型 {model!r}")
    return route.upstream_model


async def _upstream_headers(ctx: AppContext, request: Request) -> dict[str, str]:
    headers = {
        "Authorization": f"Bearer {await ctx.google_token()}",
        "Content-Type": "application/json",
    }
    for name in _FORWARD_HEADERS:
        value = request.headers.get(name)
        if value:
            headers[name] = value
    return headers


async def messages(request: Request) -> Response:
    ctx: AppContext = request.app.state.ctx
    body = await read_json_object(request)
    upstream_model = _require_model(ctx, body)
    stream = body.get("stream") is True
    # 先取 token：项目 ID 可能要从密钥文件里读出来
    headers = await _upstream_headers(ctx, request)
    backend = ctx.claude
    url = backend.messages_url(ctx.project_id(), upstream_model, stream)
    content = json.dumps(backend.messages_body(body), ensure_ascii=False).encode()

    response = await ctx.transport.open("POST", url, headers=headers, content=content, stream=True)
    if response.status_code >= 400:
        raise from_upstream(response.status_code, await ctx.transport.read_body(response))
    if not stream:
        data = await ctx.transport.read_body(response)
        return Response(
            data,
            status_code=response.status_code,
            media_type=response.headers.get("content-type", "application/json"),
        )
    return StreamingResponse(
        relay(response, Protocol.ANTHROPIC),
        status_code=response.status_code,
        media_type="text/event-stream",
        headers={"Cache-Control": "no-cache"},
    )


async def count_tokens(request: Request) -> Response:
    ctx: AppContext = request.app.state.ctx
    body = await read_json_object(request)
    upstream_model = _require_model(ctx, body)
    headers = await _upstream_headers(ctx, request)
    backend = ctx.claude
    content = json.dumps(
        backend.count_tokens_body(body, upstream_model), ensure_ascii=False
    ).encode()
    response = await ctx.transport.open(
        "POST",
        backend.count_tokens_url(ctx.project_id()),
        headers=headers,
        content=content,
        stream=False,
    )
    data = await ctx.transport.read_body(response)
    if response.status_code >= 400:
        raise from_upstream(response.status_code, data)
    return Response(data, status_code=response.status_code, media_type="application/json")


def build_router() -> APIRouter:
    router = APIRouter()
    for prefix in PREFIXES:
        router.add_api_route(f"{prefix}/v1/messages", messages, methods=["POST"])
        router.add_api_route(f"{prefix}/v1/messages/count_tokens", count_tokens, methods=["POST"])
    return router
