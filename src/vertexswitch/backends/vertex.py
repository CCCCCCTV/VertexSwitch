"""Vertex AI 端点的公共部分。"""

from __future__ import annotations


def vertex_base_url(region: str) -> str:
    """按区域选择 Vertex AI 主机（global / us、eu 多区域 / 单区域）。"""
    if region == "global":
        return "https://aiplatform.googleapis.com/v1"
    if region in ("us", "eu"):
        # 多区域端点：主机名和路径里都带 us / eu
        return f"https://aiplatform.{region}.rep.googleapis.com/v1"
    return f"https://{region}-aiplatform.googleapis.com/v1"


def publisher_models_url(project_id: str, region: str, publisher: str) -> str:
    return (
        f"{vertex_base_url(region)}/projects/{project_id}"
        f"/locations/{region}/publishers/{publisher}/models"
    )
