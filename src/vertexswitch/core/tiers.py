"""服务档位（Standard / Flex / Priority）：请求头规则与 trafficType 核对，全部是纯函数。

依据：
- https://docs.cloud.google.com/vertex-ai/generative-ai/docs/flex-paygo
- https://docs.cloud.google.com/vertex-ai/generative-ai/docs/priority-paygo

要点：
- ``X-Vertex-AI-LLM-Shared-Request-Type: flex|priority`` 选档位；
  只带它时先用预配吞吐量，再溢出到该档位
- 再加 ``X-Vertex-AI-LLM-Request-Type: shared`` 则只走 PayGo（绕过预配吞吐量）
- Flex 只支持 global 端点，且不支持部分新模型（如 gemini-4-argon）
- Priority 支持 global、us、eu 端点；没有空余容量时会降级为 Standard（trafficType=ON_DEMAND）
- Flex 的排队超时用 ``X-Server-Timeout``（秒），最长 30 分钟
- 只有 Gemini 原生 generateContent / streamGenerateContent 支持这些头
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from enum import StrEnum

HEADER_REQUEST_TYPE = "X-Vertex-AI-LLM-Request-Type"
HEADER_SHARED_TYPE = "X-Vertex-AI-LLM-Shared-Request-Type"
HEADER_SERVER_TIMEOUT = "X-Server-Timeout"

FLEX_MAX_TIMEOUT = 30 * 60
FLEX_REGIONS = frozenset({"global"})
PRIORITY_REGIONS = frozenset({"global", "us", "eu"})


class Tier(StrEnum):
    STANDARD = "standard"
    FLEX = "flex"
    PRIORITY = "priority"


@dataclass(frozen=True)
class TierPlan:
    requested: Tier
    # 实际按哪个档位发出请求头（降级后可能是 STANDARD）
    effective: Tier
    headers: dict[str, str] = field(default_factory=dict)
    shared_only: bool = False
    warnings: tuple[str, ...] = ()
    # 发了 Flex 头时，本地读超时不能小于这个值
    server_timeout: float | None = None

    @property
    def sent_tier_header(self) -> bool:
        return HEADER_SHARED_TYPE in self.headers


def parse_tier(value: str | None) -> Tier | None:
    if not value:
        return None
    try:
        return Tier(value.strip().lower())
    except ValueError:
        return None


def plan_tier(
    tier: Tier,
    *,
    model: str,
    region: str,
    shared_only: bool = False,
    flex_timeout: float = FLEX_MAX_TIMEOUT,
    flex_unsupported_models: frozenset[str] | set[str] = frozenset(),
) -> TierPlan:
    """根据档位、模型、区域算出要发的请求头。不支持时降级为 Standard 并给出告警。"""
    warnings: list[str] = []
    effective = tier
    if tier is Tier.FLEX:
        if region not in FLEX_REGIONS:
            warnings.append(f"Flex 只支持 global 端点，当前区域 {region}，已改用 Standard")
            effective = Tier.STANDARD
        elif model in flex_unsupported_models:
            warnings.append(f"模型 {model} 不支持 Flex，已改用 Standard")
            effective = Tier.STANDARD
    elif tier is Tier.PRIORITY and region not in PRIORITY_REGIONS:
        warnings.append(
            f"Priority 只支持 global / us / eu 端点，当前区域 {region}，已改用 Standard"
        )
        effective = Tier.STANDARD

    headers: dict[str, str] = {}
    server_timeout = None
    downgraded = effective is not tier
    # 降级时什么头都不发，避免把不支持的组合发给上游
    if shared_only and not downgraded:
        headers[HEADER_REQUEST_TYPE] = "shared"
    if effective is not Tier.STANDARD:
        headers[HEADER_SHARED_TYPE] = effective.value
    if effective is Tier.FLEX:
        server_timeout = float(min(max(int(flex_timeout), 1), FLEX_MAX_TIMEOUT))
        headers[HEADER_SERVER_TIMEOUT] = str(int(server_timeout))
    return TierPlan(
        requested=tier,
        effective=effective,
        headers=headers,
        shared_only=shared_only and not downgraded,
        warnings=tuple(warnings),
        server_timeout=server_timeout,
    )


_EXPECTED = {Tier.FLEX: "ON_DEMAND_FLEX", Tier.PRIORITY: "ON_DEMAND_PRIORITY"}


def check_traffic_type(plan: TierPlan, traffic_type: str | None) -> str | None:
    """核对响应里的 usageMetadata.trafficType，不符时返回告警文字。

    只在真的发了档位头时核对；没发档位头（Standard 或已降级）一律不告警。
    """
    if not plan.sent_tier_header or not traffic_type:
        return None
    expected = _EXPECTED[plan.effective]
    if traffic_type == expected:
        return None
    if traffic_type == "PROVISIONED_THROUGHPUT" and not plan.shared_only:
        # 未加 shared 头时先用预配吞吐量是文档描述的正常行为
        return None
    return (
        f"请求了 {plan.effective.value} 档位，但上游实际按 {traffic_type} 处理（期望 {expected}）"
    )


_TRAFFIC_RE = re.compile(rb'"trafficType"\s*:\s*"([A-Z_]+)"')


class TrafficTypeWatcher:
    """从（可能被任意切分的）响应字节流里找出第一个 trafficType。

    流式响应每个 chunk 都带 usageMetadata，同一请求只需要报一次。
    """

    _KEEP = 64

    def __init__(self) -> None:
        self.traffic_type: str | None = None
        self._tail = b""

    def feed(self, chunk: bytes) -> str | None:
        if self.traffic_type is not None:
            return None
        data = self._tail + chunk
        m = _TRAFFIC_RE.search(data)
        if m:
            self.traffic_type = m.group(1).decode()
            self._tail = b""
            return self.traffic_type
        self._tail = data[-self._KEEP :]
        return None
