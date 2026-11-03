"""组装 FastAPI 应用。"""

from __future__ import annotations

import asyncio
import contextlib
import logging
import secrets
from collections.abc import AsyncIterator

import httpx
from fastapi import Depends, FastAPI, Request
from fastapi.exceptions import RequestValidationError
from fastapi.responses import JSONResponse
from starlette.exceptions import HTTPException as StarletteHTTPException

from . import __version__
from .backends.claude_vertex import ClaudeVertexBackend
from .backends.gemini_native import GeminiNativeBackend
from .backends.openai_compat import OllamaBackend, VertexOpenAIBackend
from .core.auth import AuthError, ServiceAccountTokenProvider, TokenProvider
from .core.context import AppContext
from .core.errors import ProxyError, error_body, protocol_for_path
from .core.registry import ModelRegistry
from .core.settings import Settings
from .core.transport import Transport
from .frontends import anthropic_api, gemini_api, models_api, openai_api

logger = logging.getLogger(__name__)


def _render(request: Request, err: ProxyError) -> JSONResponse:
    protocol = protocol_for_path(request.url.path)
    return JSONResponse(error_body(protocol, err), status_code=err.status)


def _extract_key(request: Request) -> str:
    """Anthropic 客户端用 x-api-key，OpenAI 用 Bearer，Gemini 用 x-goog-api-key 或 ?key=。"""
    auth = request.headers.get("authorization", "")
    if auth[:7].lower() == "bearer ":
        return auth[7:].strip()
    return (
        request.headers.get("x-api-key")
        or request.headers.get("x-goog-api-key")
        or request.query_params.get("key")
        or ""
    )


async def require_api_key(request: Request) -> None:
    expected = request.app.state.ctx.settings.server.api_key
    if not expected:
        return
    if not secrets.compare_digest(_extract_key(request).encode(), expected.encode()):
        raise ProxyError(401, "API key 无效或缺失")


async def refresh_ollama_models(ctx: AppContext) -> set[str]:
    """从 Ollama ``/api/tags`` 发现本地模型；Ollama 没运行时只记日志。"""
    try:
        resp = await ctx.transport.client.get(ctx.ollama.tags_url, timeout=5)
        resp.raise_for_status()
        models = ctx.ollama.parse_tags(resp.json())
    except (httpx.HTTPError, ValueError) as e:
        logger.warning("Ollama 模型发现失败：%r", e)
        return ctx.registry.ollama_models
    ctx.registry.ollama_models = models
    logger.info("发现 %d 个 Ollama 模型", len(models))
    return models


def create_app(
    settings: Settings,
    *,
    registry: ModelRegistry | None = None,
    tokens: TokenProvider | None = None,
    http_client: httpx.AsyncClient | None = None,
) -> FastAPI:
    registry = registry or ModelRegistry.load()
    tokens = tokens or ServiceAccountTokenProvider(
        settings.resolved_credentials_file(), settings.google.project_id
    )
    owns_client = http_client is None
    client = http_client or httpx.AsyncClient(
        limits=httpx.Limits(max_connections=100, max_keepalive_connections=20)
    )
    ctx = AppContext(
        settings=settings,
        registry=registry,
        tokens=tokens,
        transport=Transport(client, settings.timeouts, settings.retry),
        claude=ClaudeVertexBackend(settings.google, settings.claude),
        gemini=GeminiNativeBackend(settings),
        gemini_openai=VertexOpenAIBackend(
            "gemini", settings.gemini_region(), settings.gemini.openai_api_version, "google/"
        ),
        maas=VertexOpenAIBackend("maas", settings.maas_region(), settings.maas.api_version),
        ollama=OllamaBackend(settings.ollama.base_url),
    )

    @contextlib.asynccontextmanager
    async def lifespan(app: FastAPI) -> AsyncIterator[None]:
        refresher = None
        if isinstance(tokens, ServiceAccountTokenProvider):
            refresher = asyncio.create_task(
                tokens.run_background_refresh(settings.google.token_refresh_minutes)
            )
        if settings.ollama.enabled:
            await refresh_ollama_models(ctx)
        try:
            yield
        finally:
            if refresher:
                refresher.cancel()
                with contextlib.suppress(asyncio.CancelledError):
                    await refresher
            if owns_client:
                await client.aclose()

    app = FastAPI(title="VertexSwitch", version=__version__, lifespan=lifespan)
    app.state.ctx = ctx

    @app.exception_handler(ProxyError)
    async def _proxy_error(request: Request, exc: ProxyError) -> JSONResponse:
        return _render(request, exc)

    @app.exception_handler(StarletteHTTPException)
    async def _http_error(request: Request, exc: StarletteHTTPException) -> JSONResponse:
        return _render(request, ProxyError(exc.status_code, str(exc.detail)))

    @app.exception_handler(RequestValidationError)
    async def _validation_error(request: Request, exc: RequestValidationError) -> JSONResponse:
        return _render(request, ProxyError(400, str(exc.errors())))

    @app.exception_handler(Exception)
    async def _unexpected(request: Request, exc: Exception) -> JSONResponse:
        logger.exception("未处理的异常")
        return _render(request, ProxyError(500, f"代理内部错误：{type(exc).__name__}: {exc}"))

    @app.get("/health")
    async def health() -> JSONResponse:
        info: dict = {
            "status": "ok",
            "version": __version__,
            "region": settings.google.region,
            "gemini_region": settings.gemini_region(),
            "gemini_tier": settings.gemini.tier,
        }
        try:
            await tokens.get_token()
            info["google_token"] = "ok"
            info["project_id"] = settings.google.project_id or tokens.project_id
        except AuthError as e:
            info["status"] = "degraded"
            info["google_token"] = str(e)
        return JSONResponse(info, status_code=200 if info["status"] == "ok" else 503)

    protected = [Depends(require_api_key)]
    app.include_router(anthropic_api.build_router(), dependencies=protected)
    app.include_router(gemini_api.build_router(), dependencies=protected)
    app.include_router(openai_api.build_router(), dependencies=protected)
    app.include_router(models_api.build_router(), dependencies=protected)
    return app
