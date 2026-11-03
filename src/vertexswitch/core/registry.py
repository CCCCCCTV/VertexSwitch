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
GEMINI = "gemini"
MAAS = "maas"
OLLAMA = "ollama"

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
    # 运行时从 Ollama /api/tags 发现的模型名
    ollama_models: set[str] = field(default_factory=set)

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
        """解析顺序：显式前缀 → Gemini → Claude → MaaS → 已发现的 Ollama 模型。"""
        model = model.strip().removeprefix("models/")
        if model.startswith("ollama/"):
            return Route(OLLAMA, model.removeprefix("ollama/"))
        # OpenAI 兼容写法 google/gemini-x、anthropic/claude-x
        model = model.removeprefix("google/").removeprefix("anthropic/")
        gemini = self.tables.get(GEMINI, {})
        if model in gemini:
            return Route(GEMINI, gemini[model])
        if model.startswith("gemini-"):
            return Route(GEMINI, model)
        claude = self.tables.get(CLAUDE, {})
        if model in claude:
            return Route(CLAUDE, claude[model])
        if model.startswith("claude-"):
            m = _DATED_CLAUDE.match(model)
            if m:
                return Route(CLAUDE, "{}@{}".format(*m.groups()))
            return Route(CLAUDE, model)
        maas = self.tables.get(MAAS, {})
        if model in maas:
            return Route(MAAS, maas[model])
        if "/" in model:
            # publisher/model 形式（如 deepseek-ai/deepseek-v3.1-maas）原样交给 MaaS
            return Route(MAAS, model)
        if model in self.ollama_models:
            return Route(OLLAMA, model)
        return None

    def aliases(self, backend: str) -> list[str]:
        return list(self.tables.get(backend, {}))
