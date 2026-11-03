"""Claude on Vertex AI 后端。

参考：
- https://platform.claude.com/docs/en/build-with-claude/claude-models-on-google-cloud/google-managed
- Anthropic 官方 Python SDK ``anthropic.lib.vertex``（count_tokens 路径、anthropic-beta 透传）

请求体与 Messages API 基本相同，区别：``model`` 写在 URL 里而不在请求体，
``anthropic_version`` 放在请求体里。
"""

from __future__ import annotations

from typing import Any

from ..core.settings import ClaudeSettings, GoogleSettings


def vertex_base_url(region: str) -> str:
    """按区域选择 Vertex AI 主机。"""
    if region == "global":
        return "https://aiplatform.googleapis.com/v1"
    if region in ("us", "eu"):
        # 多区域端点：主机名和路径里都带 us / eu
        return f"https://aiplatform.{region}.rep.googleapis.com/v1"
    return f"https://{region}-aiplatform.googleapis.com/v1"


class ClaudeVertexBackend:
    name = "claude"
    needs_google_token = True
    supports_tier = False

    def __init__(self, google: GoogleSettings, claude: ClaudeSettings):
        self.google = google
        self.claude = claude

    def _publisher_url(self, project_id: str) -> str:
        region = self.google.region
        return (
            f"{vertex_base_url(region)}/projects/{project_id}"
            f"/locations/{region}/publishers/anthropic/models"
        )

    def messages_url(self, project_id: str, model: str, stream: bool) -> str:
        specifier = "streamRawPredict" if stream else "rawPredict"
        return f"{self._publisher_url(project_id)}/{model}:{specifier}"

    def count_tokens_url(self, project_id: str) -> str:
        return f"{self._publisher_url(project_id)}/count-tokens:rawPredict"

    def messages_body(self, body: dict[str, Any]) -> dict[str, Any]:
        out = {k: v for k, v in body.items() if k != "model"}
        for name in self.claude.drop_body_fields:
            out.pop(name, None)
        out.setdefault("anthropic_version", self.claude.anthropic_version)
        return out

    def count_tokens_body(self, body: dict[str, Any], upstream_model: str) -> dict[str, Any]:
        # count-tokens 端点的模型写在请求体里
        out = self.messages_body(body)
        out["model"] = upstream_model
        return out
