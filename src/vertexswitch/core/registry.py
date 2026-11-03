"""模型名 → (后端, 上游模型 ID)。

模型表来自 models.yaml（包内默认，用户目录下的同名文件优先），不写死在代码里。
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from importlib import resources
from pathlib import Path

import yaml

from .settings import app_dir

CLAUDE = "claude"

# claude-haiku-4-5-20251001 → claude-haiku-4-5@20251001
_DATED_CLAUDE = re.compile(r"^(claude-[a-z0-9-]+?)-(\d{8})$")


@dataclass(frozen=True)
class Route:
    backend: str
    upstream_model: str


@dataclass
class ModelRegistry:
    # 后端名 → {别名: 上游 ID}
    tables: dict[str, dict[str, str]] = field(default_factory=dict)

    @classmethod
    def from_mapping(cls, data: dict) -> ModelRegistry:
        tables: dict[str, dict[str, str]] = {}
        for backend, table in (data or {}).items():
            if not isinstance(table, dict):
                raise ValueError(f"models.yaml 中 {backend!r} 应为 别名: 模型ID 的映射")
            tables[str(backend)] = {str(k): str(v) for k, v in table.items()}
        return cls(tables)

    @classmethod
    def load(cls, path: Path | None = None) -> ModelRegistry:
        if path is None:
            user_file = app_dir() / "models.yaml"
            if user_file.exists():
                path = user_file
        if path is not None:
            text = path.read_text(encoding="utf-8")
        else:
            text = resources.files("vertexswitch.data").joinpath("models.yaml").read_text("utf-8")
        return cls.from_mapping(yaml.safe_load(text) or {})

    def resolve(self, model: str) -> Route | None:
        model = model.strip()
        claude = self.tables.get(CLAUDE, {})
        if model in claude:
            return Route(CLAUDE, claude[model])
        if model.startswith("claude-"):
            m = _DATED_CLAUDE.match(model)
            if m:
                return Route(CLAUDE, "{}@{}".format(*m.groups()))
            return Route(CLAUDE, model)
        return None

    def aliases(self, backend: str) -> list[str]:
        return list(self.tables.get(backend, {}))
