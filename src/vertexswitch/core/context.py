"""挂在 ``app.state.ctx`` 上的运行时状态（不使用模块级全局变量）。"""

from __future__ import annotations

from dataclasses import dataclass

from ..backends.claude_vertex import ClaudeVertexBackend
from ..backends.gemini_native import GeminiNativeBackend
from .auth import AuthError, TokenProvider
from .errors import ProxyError
from .registry import ModelRegistry
from .settings import Settings
from .transport import Transport


@dataclass
class AppContext:
    settings: Settings
    registry: ModelRegistry
    tokens: TokenProvider
    transport: Transport
    claude: ClaudeVertexBackend
    gemini: GeminiNativeBackend

    async def google_token(self) -> str:
        try:
            return await self.tokens.get_token()
        except AuthError as e:
            raise ProxyError(500, str(e)) from e

    def project_id(self) -> str:
        project = self.settings.google.project_id or self.tokens.project_id
        if not project:
            raise ProxyError(500, "未配置 Google Cloud 项目 ID（google.project_id）")
        return project
