"""Gemini 原生入口：``POST /v1beta/models/{model}:{action}``。

action 为 generateContent / streamGenerateContent / countTokens。查询参数（如 ``alt=sse``）
透传，代理自己的 ``key`` 参数除外。生成类请求按配置附加服务档位头，并用响应里的
``usageMetadata.trafficType`` 核对实际档位。
"""

from __future__ import annotations

import json
import logging
from urllib.parse import urlencode

from fastapi import APIRouter, Request, Response
from fastapi.responses import StreamingResponse

from ..backends.gemini_native import ACTIONS
from ..core.context import AppContext
from ..core.errors import Protocol, ProxyError, from_upstream
from ..core.registry import GEMINI
from ..core.tiers import TierPlan, TrafficTypeWatcher, check_traffic_type
from .common import read_json_object, relay

logger = logging.getLogger(__name__)

PREFIXES = ("", "/gemini")
TIER_OVERRIDE_HEADER = "x-vertexswitch-tier"
# Flex 排队到 X-Server-Timeout 才返回时，本地多等这么久再判超时
_TIMEOUT_MARGIN = 60.0


def _split_target(target: str) -> tuple[str, str]:
    model, sep, action = target.rpartition(":")
    if not sep or not model or action not in ACTIONS:
        raise ProxyError(404, f"不支持的 Gemini 方法：{target!r}")
    return model, action


class _TrafficCheck:
    """收到第一个 trafficType 时核对一次档位。"""

    def __init__(self, plan: TierPlan | None, model: str):
        self.plan = plan
        self.model = model
        self.watcher = TrafficTypeWatcher()

    def feed(self, chunk: bytes) -> None:
        if self.plan is None:
            return
        traffic_type = self.watcher.feed(chunk)
        if traffic_type is None:
            return
        warning = check_traffic_type(self.plan, traffic_type)
        if warning:
            logger.warning("%s：%s", self.model, warning)


async def model_action(request: Request, target: str) -> Response:
    ctx: AppContext = request.app.state.ctx
    model, action = _split_target(target)
    route = ctx.registry.resolve(model)
    if route is None or route.backend != GEMINI:
        raise ProxyError(404, f"未知的 Gemini 模型 {model!r}")
    body = ctx.gemini.prepare_body(await read_json_object(request))

    headers = {
        "Authorization": f"Bearer {await ctx.google_token()}",
        "Content-Type": "application/json",
    }
    plan = ctx.gemini.tier_plan(
        route.upstream_model, action, request.headers.get(TIER_OVERRIDE_HEADER)
    )
    if plan is not None:
        for warning in plan.warnings:
            logger.warning("%s：%s", route.upstream_model, warning)
        headers.update(plan.headers)

    url = ctx.gemini.url(ctx.project_id(), route.upstream_model, action)
    query = [(k, v) for k, v in request.query_params.multi_items() if k != "key"]
    if query:
        url = f"{url}?{urlencode(query)}"
    stream = action == "streamGenerateContent"
    min_read = plan.server_timeout + _TIMEOUT_MARGIN if plan and plan.server_timeout else None

    response = await ctx.transport.open(
        "POST",
        url,
        headers=headers,
        content=json.dumps(body, ensure_ascii=False).encode(),
        stream=True,
        min_read_timeout=min_read,
    )
    if response.status_code >= 400:
        raise from_upstream(response.status_code, await ctx.transport.read_body(response))

    out_headers = {}
    if plan is not None:
        out_headers["X-VertexSwitch-Tier"] = plan.effective.value
    check = _TrafficCheck(plan, route.upstream_model)
    media_type = response.headers.get("content-type", "application/json")
    if not stream:
        data = await ctx.transport.read_body(response)
        check.feed(data)
        return Response(
            data, status_code=response.status_code, media_type=media_type, headers=out_headers
        )
    return StreamingResponse(
        relay(response, Protocol.GEMINI, check.feed),
        status_code=response.status_code,
        media_type=media_type,
        headers={"Cache-Control": "no-cache", **out_headers},
    )


def build_router() -> APIRouter:
    router = APIRouter()
    for prefix in PREFIXES:
        router.add_api_route(
            f"{prefix}/v1beta/models/{{target:path}}", model_action, methods=["POST"]
        )
    # 部分客户端走 v1 路径
    router.add_api_route("/v1/models/{target:path}", model_action, methods=["POST"])
    return router
