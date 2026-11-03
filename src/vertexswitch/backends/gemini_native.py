"""Gemini 原生 generateContent 后端（Vertex AI publishers/google）。

这是唯一支持服务档位头（Flex / Priority）的通道。
URL：``{host}/v1/projects/{p}/locations/{r}/publishers/google/models/{model}:{action}``
"""

from __future__ import annotations

from typing import Any

from ..core.settings import Settings
from ..core.tiers import Tier, TierPlan, parse_tier, plan_tier
from .vertex import publisher_models_url

GENERATE_ACTIONS = frozenset({"generateContent", "streamGenerateContent"})
ACTIONS = GENERATE_ACTIONS | {"countTokens"}


class GeminiNativeBackend:
    name = "gemini"
    needs_google_token = True
    supports_tier = True

    def __init__(self, settings: Settings):
        self.settings = settings

    @property
    def region(self) -> str:
        return self.settings.gemini_region()

    def url(self, project_id: str, model: str, action: str) -> str:
        return f"{publisher_models_url(project_id, self.region, 'google')}/{model}:{action}"

    def tier_plan(self, model: str, action: str, override: str | None = None) -> TierPlan | None:
        """档位头只用于生成类请求；countTokens 等不发。"""
        if action not in GENERATE_ACTIONS:
            return None
        g = self.settings.gemini
        tier = parse_tier(override) or parse_tier(g.tier) or Tier.STANDARD
        return plan_tier(
            tier,
            model=model,
            region=self.region,
            shared_only=g.shared_only,
            flex_timeout=g.flex_server_timeout,
            flex_unsupported_models=frozenset(g.flex_unsupported_models),
        )

    @staticmethod
    def prepare_body(body: dict[str, Any]) -> dict[str, Any]:
        """Gemini API 允许 contents 省略 role，Vertex 要求写明；缺省补 "user"。"""
        contents = body.get("contents")
        if not isinstance(contents, list) or all(
            not isinstance(c, dict) or c.get("role") for c in contents
        ):
            return body
        out = dict(body)
        out["contents"] = [
            {**c, "role": "user"} if isinstance(c, dict) and not c.get("role") else c
            for c in contents
        ]
        return out
