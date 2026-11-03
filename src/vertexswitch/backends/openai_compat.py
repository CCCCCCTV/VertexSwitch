"""OpenAI Chat Completions 兼容的上游：Vertex 的 Gemini 兼容接口、MaaS 开放模型、本地 Ollama。

- Gemini：``{host}/v1beta1/projects/{p}/locations/{r}/endpoints/openapi/chat/completions``，
  ``model`` 写成 ``google/{model}``；**不支持服务档位头**（Vertex 会返回 400）
- MaaS：同一路径（官方文档为 v1），``model`` 为 ``publisher/model``
- Ollama：``{base_url}/v1/chat/completions``，不需要 Google token

参考：
- https://docs.cloud.google.com/vertex-ai/generative-ai/docs/multimodal/call-gemini-using-openai-library
- https://docs.cloud.google.com/vertex-ai/generative-ai/docs/maas/call-open-model-apis
"""

from __future__ import annotations

from typing import Any

from .vertex import vertex_base_url


def openapi_chat_url(project_id: str, region: str, api_version: str) -> str:
    base = vertex_base_url(region).removesuffix("/v1")
    return (
        f"{base}/{api_version}/projects/{project_id}"
        f"/locations/{region}/endpoints/openapi/chat/completions"
    )


class VertexOpenAIBackend:
    """Gemini 兼容接口与 MaaS 共用：只有区域、API 版本、模型名写法不同。"""

    needs_google_token = True
    supports_tier = False

    def __init__(self, name: str, region: str, api_version: str, model_prefix: str = ""):
        self.name = name
        self.region = region
        self.api_version = api_version
        self.model_prefix = model_prefix

    def url(self, project_id: str) -> str:
        return openapi_chat_url(project_id, self.region, self.api_version)

    def body(self, body: dict[str, Any], upstream_model: str) -> dict[str, Any]:
        return {**body, "model": f"{self.model_prefix}{upstream_model}"}


class OllamaBackend:
    name = "ollama"
    needs_google_token = False
    supports_tier = False

    def __init__(self, base_url: str):
        self.base_url = base_url.rstrip("/")

    def url(self, project_id: str = "") -> str:
        return f"{self.base_url}/v1/chat/completions"

    @property
    def tags_url(self) -> str:
        return f"{self.base_url}/api/tags"

    def body(self, body: dict[str, Any], upstream_model: str) -> dict[str, Any]:
        return {**body, "model": upstream_model}

    @staticmethod
    def parse_tags(data: Any) -> set[str]:
        """``GET /api/tags`` → 模型名集合。"""
        models = data.get("models") if isinstance(data, dict) else None
        if not isinstance(models, list):
            return set()
        return {m["name"] for m in models if isinstance(m, dict) and isinstance(m.get("name"), str)}
