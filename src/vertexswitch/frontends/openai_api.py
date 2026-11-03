"""OpenAI Chat Completions 入口：``/v1/chat/completions`` 及若干前缀别名。

按 ``model`` 分流：
- Gemini → Vertex 的 OpenAI 兼容接口（原样转发，不带档位头）
- MaaS 开放模型 → 同一接口，``publisher/model``
- Ollama → 本地 ``/v1/chat/completions``（不需要 Google token）
- Claude → 转成 Anthropic Messages 调 Vertex，再把响应（含流式）转回 OpenAI 格式
"""

from __future__ import annotations

import json
import logging
from collections.abc import AsyncIterator
from typing import Any

import httpx
from fastapi import APIRouter, Request, Response
from fastapi.responses import JSONResponse, StreamingResponse

from ..core.context import AppContext
from ..core.errors import Protocol, ProxyError, error_body, from_upstream, stream_error_event
from ..core.registry import CLAUDE, Route
from ..translate.openai_to_anthropic import (
    SSEParser,
    StreamTranslator,
    request_to_anthropic,
    response_to_openai,
    sse_data,
)
from .common import read_json_object, relay

logger = logging.getLogger(__name__)

PATHS = (
    "/v1/chat/completions",
    "/chat/completions",
    "/openai/v1/chat/completions",
    "/api/v1/chat/completions",
)


async def chat_completions(request: Request) -> Response:
    ctx: AppContext = request.app.state.ctx
    body = await read_json_object(request)
    model = body.get("model")
    if not isinstance(model, str) or not model.strip():
        raise ProxyError(400, "model: 缺少模型名")
    route = ctx.registry.resolve(model)
    if route is None:
        raise ProxyError(404, f"未知模型 {model!r}")
    if route.backend == CLAUDE:
        return await _via_claude(ctx, body, route, model)
    return await _passthrough(ctx, body, route)


async def _passthrough(ctx: AppContext, body: dict[str, Any], route: Route) -> Response:
    backend = ctx.openai_backend(route.backend)
    headers = {"Content-Type": "application/json"}
    if backend.needs_google_token:
        headers["Authorization"] = f"Bearer {await ctx.google_token()}"
        url = backend.url(ctx.project_id())
        upstream, detect_quota = "Vertex AI", True
    else:
        url = backend.url()
        upstream, detect_quota = "Ollama", False
    stream = body.get("stream") is True
    response = await ctx.transport.open(
        "POST",
        url,
        headers=headers,
        content=json.dumps(backend.body(body, route.upstream_model), ensure_ascii=False).encode(),
        stream=True,
        upstream=upstream,
        detect_quota=detect_quota,
    )
    if response.status_code >= 400:
        raise from_upstream(response.status_code, await ctx.transport.read_body(response))
    media_type = response.headers.get("content-type", "application/json")
    if not stream:
        data = await ctx.transport.read_body(response)
        return Response(data, status_code=response.status_code, media_type=media_type)
    return StreamingResponse(
        relay(response, Protocol.OPENAI),
        status_code=response.status_code,
        media_type="text/event-stream",
        headers={"Cache-Control": "no-cache"},
    )


async def _via_claude(
    ctx: AppContext, body: dict[str, Any], route: Route, client_model: str
) -> Response:
    anthropic_body = request_to_anthropic(body, ctx.settings.openai.default_max_tokens)
    stream = anthropic_body.get("stream") is True
    headers = {
        "Authorization": f"Bearer {await ctx.google_token()}",
        "Content-Type": "application/json",
    }
    url = ctx.claude.messages_url(ctx.project_id(), route.upstream_model, stream)
    content = json.dumps(ctx.claude.messages_body(anthropic_body), ensure_ascii=False).encode()
    response = await ctx.transport.open("POST", url, headers=headers, content=content, stream=True)
    if response.status_code >= 400:
        raise from_upstream(response.status_code, await ctx.transport.read_body(response))
    if not stream:
        data = await ctx.transport.read_body(response)
        try:
            message = json.loads(data)
        except ValueError as e:
            raise ProxyError(502, "Vertex AI 返回了无法解析的响应") from e
        return JSONResponse(response_to_openai(message, client_model))
    include_usage = bool((body.get("stream_options") or {}).get("include_usage"))
    return StreamingResponse(
        _translate_stream(response, StreamTranslator(client_model, include_usage)),
        media_type="text/event-stream",
        headers={"Cache-Control": "no-cache"},
    )


async def _translate_stream(
    response: httpx.Response, translator: StreamTranslator
) -> AsyncIterator[bytes]:
    parser = SSEParser()
    try:
        async for chunk in response.aiter_bytes():
            for event, raw in parser.feed(chunk):
                try:
                    data = json.loads(raw)
                except ValueError:
                    logger.warning("无法解析的上游 SSE 数据：%.200s", raw)
                    continue
                if event == "error" or data.get("type") == "error":
                    err = data.get("error") or {}
                    yield sse_data(
                        error_body(
                            Protocol.OPENAI,
                            ProxyError(500, err.get("message") or "上游返回错误事件"),
                        )
                    )
                    return
                for out in translator.handle(event, data):
                    yield sse_data(out)
        if not translator.finished:
            yield stream_error_event(Protocol.OPENAI, ProxyError(502, "Vertex AI 流式响应提前结束"))
            return
        yield sse_data("[DONE]")
    except httpx.ReadTimeout:
        yield stream_error_event(Protocol.OPENAI, ProxyError(504, "Vertex AI 流式响应空闲超时"))
    except httpx.HTTPError as e:
        logger.warning("上游流式响应中断：%r", e)
        yield stream_error_event(
            Protocol.OPENAI, ProxyError(502, f"Vertex AI 流式响应中断：{type(e).__name__}")
        )
    finally:
        await response.aclose()


def build_router() -> APIRouter:
    router = APIRouter()
    for path in PATHS:
        router.add_api_route(path, chat_completions, methods=["POST"])
    return router
