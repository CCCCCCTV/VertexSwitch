"""配置模型与读写。

配置文件固定在 ``%APPDATA%\\VertexSwitch\\config.toml``；开发或测试时可用环境变量
``VERTEXSWITCH_HOME`` 指向别的目录。不从当前目录读 ``.env``。
"""

from __future__ import annotations

import os
import tomllib
from pathlib import Path

import tomli_w
from pydantic import BaseModel, Field

APP_NAME = "VertexSwitch"


def app_dir() -> Path:
    """配置、日志所在目录。"""
    override = os.environ.get("VERTEXSWITCH_HOME")
    if override:
        return Path(override)
    appdata = os.environ.get("APPDATA")
    base = Path(appdata) if appdata else Path.home() / "AppData" / "Roaming"
    return base / APP_NAME


def config_path() -> Path:
    return app_dir() / "config.toml"


class ServerSettings(BaseModel):
    host: str = "127.0.0.1"
    port: int = 8788
    # 非空时，除 /health 外的请求都要带 ``Authorization: Bearer <key>`` 或 ``x-api-key``
    api_key: str = ""
    keep_alive_seconds: int = 75


class GoogleSettings(BaseModel):
    # 服务账号 JSON 路径；为空时退回 GOOGLE_APPLICATION_CREDENTIALS
    credentials_file: str = ""
    # 为空时从密钥文件读取
    project_id: str = ""
    # global / us / eu / 具体区域（如 us-east5）
    region: str = "global"
    token_refresh_minutes: int = 50


class TimeoutSettings(BaseModel):
    connect: float = 10.0
    # 非流式：等上游完整响应的读超时
    read: float = 600.0
    # 流式：两段数据之间允许的最长空闲时间，防止永久挂起
    stream_idle: float = 900.0
    write: float = 60.0
    pool: float = 10.0


class RetrySettings(BaseModel):
    # 只在收到任何响应字节之前断开时重试
    attempts: int = 2
    backoff_seconds: float = 0.4
    # 上游挂住超过这个时长后无响应断开，视为配额耗尽，不再重试
    quota_hang_seconds: float = 20.0


class ClaudeSettings(BaseModel):
    anthropic_version: str = "vertex-2023-10-16"
    # 转发前从请求体删除的顶层字段（Vertex 不接受的字段）
    drop_body_fields: list[str] = Field(default_factory=lambda: ["context_management"])


class GeminiSettings(BaseModel):
    # 为空时用 google.region；Flex 只支持 global，所以 Gemini 常需要单独设区域
    region: str = ""
    # standard / flex / priority；请求头 X-VertexSwitch-Tier 可按请求覆盖
    tier: str = "standard"
    # 加 X-Vertex-AI-LLM-Request-Type: shared，只走 PayGo（绕过预配吞吐量）
    shared_only: bool = False
    # Flex 排队超时（秒，X-Server-Timeout），上限 1800
    flex_server_timeout: int = 1800
    # 官方文档注明不支持 Flex 的模型，请求 Flex 时降级为 Standard
    flex_unsupported_models: list[str] = Field(default_factory=lambda: ["gemini-4-argon"])
    # OpenAI 兼容接口（/endpoints/openapi/chat/completions）的 API 版本，官方示例为 v1beta1。
    # 该接口不支持服务档位头。
    openai_api_version: str = "v1beta1"


class MaaSSettings(BaseModel):
    # 开放模型（DeepSeek、Qwen、GLM 等）所在区域；为空时用 google.region
    region: str = ""
    api_version: str = "v1"


class OllamaSettings(BaseModel):
    enabled: bool = False
    base_url: str = "http://127.0.0.1:11434"


class OpenAISettings(BaseModel):
    # OpenAI 请求转 Claude 时，客户端没给 max_tokens 用这个值（Anthropic 要求必填）
    default_max_tokens: int = 8192


class Settings(BaseModel):
    server: ServerSettings = Field(default_factory=ServerSettings)
    google: GoogleSettings = Field(default_factory=GoogleSettings)
    timeouts: TimeoutSettings = Field(default_factory=TimeoutSettings)
    retry: RetrySettings = Field(default_factory=RetrySettings)
    claude: ClaudeSettings = Field(default_factory=ClaudeSettings)
    gemini: GeminiSettings = Field(default_factory=GeminiSettings)
    maas: MaaSSettings = Field(default_factory=MaaSSettings)
    ollama: OllamaSettings = Field(default_factory=OllamaSettings)
    openai: OpenAISettings = Field(default_factory=OpenAISettings)

    def gemini_region(self) -> str:
        return self.gemini.region or self.google.region

    def maas_region(self) -> str:
        return self.maas.region or self.google.region

    def resolved_credentials_file(self) -> str:
        return self.google.credentials_file or os.environ.get("GOOGLE_APPLICATION_CREDENTIALS", "")


def load_settings(path: Path | None = None) -> Settings:
    """读取配置；文件不存在时返回默认值（不写盘）。"""
    path = path or config_path()
    if not path.exists():
        return Settings()
    with path.open("rb") as f:
        return Settings.model_validate(tomllib.load(f))


def save_settings(settings: Settings, path: Path | None = None) -> Path:
    path = path or config_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".toml.tmp")
    tmp.write_bytes(tomli_w.dumps(settings.model_dump()).encode("utf-8"))
    tmp.replace(path)
    return path
