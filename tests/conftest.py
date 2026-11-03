from __future__ import annotations

from collections.abc import Callable

import httpx
import pytest
from fastapi.testclient import TestClient

from vertexswitch.app import create_app
from vertexswitch.core.auth import StaticTokenProvider
from vertexswitch.core.registry import ModelRegistry
from vertexswitch.core.settings import Settings


@pytest.fixture
def anyio_backend():
    return "asyncio"


@pytest.fixture
def settings() -> Settings:
    s = Settings()
    s.retry.backoff_seconds = 0
    return s


@pytest.fixture
def make_client(settings: Settings) -> Callable[..., TestClient]:
    """用 MockTransport 搭一个完整的应用；handler 接收发往 Vertex 的请求。"""

    def factory(handler, **kwargs) -> TestClient:
        http = httpx.AsyncClient(transport=httpx.MockTransport(handler))
        app = create_app(
            settings,
            registry=ModelRegistry.load(),
            tokens=StaticTokenProvider("tok", "proj"),
            http_client=http,
        )
        return TestClient(app, **kwargs)

    return factory
