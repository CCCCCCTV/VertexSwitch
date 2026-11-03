"""模型列表：客户端探测用。

``/v1/models`` 同时被 Anthropic 和 OpenAI 客户端使用：带 ``anthropic-version`` 头时
按 Anthropic 格式返回，否则按 OpenAI 格式返回。``/v1beta/models`` 按 Gemini 格式返回。
"""

from __future__ import annotations

from fastapi import APIRouter, Request

from ..core.context import AppContext
from ..core.errors import ProxyError
from ..core.registry import CLAUDE, GEMINI, MAAS

_EPOCH = "1970-01-01T00:00:00Z"


def _anthropic_model(model_id: str) -> dict:
    return {"type": "model", "id": model_id, "display_name": model_id, "created_at": _EPOCH}


def _openai_model(model_id: str) -> dict:
    return {"id": model_id, "object": "model", "created": 0, "owned_by": "vertexswitch"}


def _gemini_model(model_id: str) -> dict:
    return {
        "name": f"models/{model_id}",
        "displayName": model_id,
        "supportedGenerationMethods": ["generateContent", "streamGenerateContent", "countTokens"],
    }


def _ids(ctx: AppContext) -> list[str]:
    reg = ctx.registry
    ollama = (
        [f"ollama/{m}" for m in sorted(reg.ollama_models)] if ctx.settings.ollama.enabled else []
    )
    return reg.aliases(CLAUDE) + reg.aliases(GEMINI) + reg.aliases(MAAS) + ollama


def _wants_anthropic(request: Request) -> bool:
    return request.url.path.startswith("/anthropic/") or "anthropic-version" in request.headers


async def list_models(request: Request) -> dict:
    ctx: AppContext = request.app.state.ctx
    if _wants_anthropic(request):
        ids = ctx.registry.aliases(CLAUDE)
        return {
            "data": [_anthropic_model(i) for i in ids],
            "has_more": False,
            "first_id": ids[0] if ids else None,
            "last_id": ids[-1] if ids else None,
        }
    return {"object": "list", "data": [_openai_model(i) for i in _ids(ctx)]}


async def get_model(request: Request, model_id: str) -> dict:
    if model_id not in _ids(request.app.state.ctx):
        raise ProxyError(404, f"未知模型 {model_id!r}")
    return _anthropic_model(model_id) if _wants_anthropic(request) else _openai_model(model_id)


async def list_gemini_models(request: Request) -> dict:
    return {"models": [_gemini_model(i) for i in request.app.state.ctx.registry.aliases(GEMINI)]}


async def get_gemini_model(request: Request, model_id: str) -> dict:
    model_id = model_id.removeprefix("models/")
    if model_id not in request.app.state.ctx.registry.aliases(GEMINI):
        raise ProxyError(404, f"未知模型 {model_id!r}")
    return _gemini_model(model_id)


def build_router() -> APIRouter:
    router = APIRouter()
    for prefix in ("", "/gemini"):
        router.add_api_route(f"{prefix}/v1beta/models", list_gemini_models, methods=["GET"])
        router.add_api_route(
            f"{prefix}/v1beta/models/{{model_id:path}}", get_gemini_model, methods=["GET"]
        )
    for prefix in ("", "/anthropic"):
        router.add_api_route(f"{prefix}/v1/models", list_models, methods=["GET"])
        router.add_api_route(f"{prefix}/v1/models/{{model_id:path}}", get_model, methods=["GET"])
    return router
