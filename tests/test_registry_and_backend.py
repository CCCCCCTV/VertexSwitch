from vertexswitch.backends.claude_vertex import ClaudeVertexBackend, vertex_base_url
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
