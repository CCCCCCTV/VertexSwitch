from vertexswitch.backends.claude_vertex import ClaudeVertexBackend
from vertexswitch.backends.vertex import vertex_base_url
from vertexswitch.core.registry import CLAUDE, ModelRegistry, Route
from vertexswitch.core.settings import ClaudeSettings, GoogleSettings


def test_registry_aliases_from_default_file():
    reg = ModelRegistry.load()
    assert reg.resolve("claude-opus-4-6") == Route(CLAUDE, "claude-opus-4-6")
    assert reg.resolve("claude-haiku-4-5") == Route(CLAUDE, "claude-haiku-4-5@20251001")


def test_registry_dated_hyphen_becomes_at():
    reg = ModelRegistry.load()
    assert reg.resolve("claude-sonnet-4-5-20250929") == Route(CLAUDE, "claude-sonnet-4-5@20250929")


def test_registry_unknown_claude_passthrough_and_non_claude_none():
    reg = ModelRegistry.from_mapping({"claude": {}})
    assert reg.resolve("claude-future-9") == Route(CLAUDE, "claude-future-9")
    assert reg.resolve("gpt-4o") is None


def test_vertex_hosts():
    assert vertex_base_url("global") == "https://aiplatform.googleapis.com/v1"
    assert vertex_base_url("us") == "https://aiplatform.us.rep.googleapis.com/v1"
    assert vertex_base_url("eu") == "https://aiplatform.eu.rep.googleapis.com/v1"
    assert vertex_base_url("us-east5") == "https://us-east5-aiplatform.googleapis.com/v1"


def _backend(region="global"):
    return ClaudeVertexBackend(GoogleSettings(region=region), ClaudeSettings())


def test_messages_urls():
    b = _backend("us-east5")
    base = (
        "https://us-east5-aiplatform.googleapis.com/v1/projects/p/locations/us-east5"
        "/publishers/anthropic/models"
    )
    assert b.messages_url("p", "claude-x", stream=False) == f"{base}/claude-x:rawPredict"
    assert b.messages_url("p", "claude-x", stream=True) == f"{base}/claude-x:streamRawPredict"
    assert b.count_tokens_url("p") == f"{base}/count-tokens:rawPredict"


def test_messages_body_rewrite():
    body = {"model": "m", "max_tokens": 5, "messages": [], "context_management": {"x": 1}}
    out = _backend().messages_body(body)
    assert out == {"max_tokens": 5, "messages": [], "anthropic_version": "vertex-2023-10-16"}
    assert body["model"] == "m"  # 不修改调用方的字典


def test_count_tokens_body_keeps_model():
    out = _backend().count_tokens_body({"model": "alias", "messages": []}, "claude-x@1")
    assert out["model"] == "claude-x@1"
    assert out["anthropic_version"] == "vertex-2023-10-16"


# 官方已宣布下线的模型，默认模型表不应再列出（避免出现在 /v1beta/models 里误导客户端）
RETIRED_GEMINI = {
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite",
    "gemini-2.0-flash",
    "gemini-2.0-flash-001",
    "gemini-3-pro-preview",
}


def test_default_table_has_no_retired_gemini():
    table = ModelRegistry.load().tables["gemini"]
    assert not RETIRED_GEMINI & (set(table) | set(table.values()))


def test_gemini_aliases():
    reg = ModelRegistry.load()
    assert reg.resolve("gemini-3-flash") == Route("gemini", "gemini-3-flash-preview")
    assert reg.resolve("models/gemini-3.1-pro") == Route("gemini", "gemini-3.1-pro-preview")
    # 未列出的照样转发，由上游决定是否存在
    assert reg.resolve("gemini-9-future") == Route("gemini", "gemini-9-future")
