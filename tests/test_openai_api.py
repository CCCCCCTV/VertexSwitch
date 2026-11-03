import json

import httpx
import pytest

OPENAPI_GLOBAL = (
    "https://aiplatform.googleapis.com/{ver}/projects/proj/locations/global"
    "/endpoints/openapi/chat/completions"
)
COMPLETION = {
    "id": "c1",
    "object": "chat.completion",
    "choices": [{"index": 0, "message": {"role": "assistant", "content": "pong"}}],
}


def _chat(model, **extra):
    return {"model": model, "messages": [{"role": "user", "content": "ping"}], **extra}


@pytest.fixture
def capture(make_client):
    seen: list[httpx.Request] = []

    def factory(response=None, **kw):
        def handler(request):
            seen.append(request)
            if callable(response):
                return response(request)
            return response or httpx.Response(200, json=COMPLETION)

        return make_client(handler, **kw), seen

    return factory


@pytest.mark.parametrize(
    "path",
    [
        "/v1/chat/completions",
        "/chat/completions",
        "/openai/v1/chat/completions",
        "/api/v1/chat/completions",
    ],
)
def test_gemini_via_openai_compat(capture, settings, path):
    settings.gemini.tier = "flex"  # 兼容接口不支持档位头，必须不发
    client, seen = capture()
    r = client.post(path, json=_chat("gemini-3-flash", temperature=0.1))
    assert r.status_code == 200
    assert r.json() == COMPLETION
    (up,) = seen
    assert str(up.url) == OPENAPI_GLOBAL.format(ver="v1beta1")
    assert up.headers["authorization"] == "Bearer tok"
    sent = json.loads(up.content)
    assert sent["model"] == "google/gemini-3-flash-preview"
    assert sent["temperature"] == 0.1
    assert not any(h.lower().startswith("x-vertex-ai-llm") for h in up.headers)
    assert "x-server-timeout" not in up.headers


def test_google_prefixed_model_name(capture):
    client, seen = capture()
    client.post("/v1/chat/completions", json=_chat("google/gemini-3.5-flash"))
    assert json.loads(seen[0].content)["model"] == "google/gemini-3.5-flash"


def test_maas_publisher_model_uses_v1_and_maas_region(capture, settings):
    settings.maas.region = "us-central1"
    client, seen = capture()
    client.post("/v1/chat/completions", json=_chat("deepseek-ai/deepseek-v3.1-maas"))
    assert str(seen[0].url) == (
        "https://us-central1-aiplatform.googleapis.com/v1/projects/proj/locations/us-central1"
        "/endpoints/openapi/chat/completions"
    )
    assert json.loads(seen[0].content)["model"] == "deepseek-ai/deepseek-v3.1-maas"


def test_openai_stream_passthrough(capture):
    sse = b'data: {"choices":[{"delta":{"content":"po"}}]}\n\ndata: [DONE]\n\n'
    client, _ = capture(
        httpx.Response(200, content=sse, headers={"content-type": "text/event-stream"})
    )
    r = client.post("/v1/chat/completions", json=_chat("gemini-3.5-flash", stream=True))
    assert r.content == sse
    assert r.headers["content-type"].startswith("text/event-stream")


def test_ollama_needs_no_google_token(capture, settings):
    settings.ollama.enabled = True

    class Boom:
        project_id = ""

        async def get_token(self):
            raise AssertionError("Ollama 不应获取 Google token")

    client, seen = capture()
    client.app.state.ctx.tokens = Boom()
    r = client.post("/v1/chat/completions", json=_chat("ollama/qwen3:8b"))
    assert r.status_code == 200
    assert str(seen[0].url) == "http://127.0.0.1:11434/v1/chat/completions"
    assert "authorization" not in seen[0].headers
    assert json.loads(seen[0].content)["model"] == "qwen3:8b"


def test_ollama_disabled(capture):
    client, seen = capture()
    r = client.post("/v1/chat/completions", json=_chat("ollama/qwen3:8b"))
    assert r.status_code == 400
    assert "error" in r.json()
    assert seen == []


def test_ollama_discovery_on_startup(capture, settings):
    settings.ollama.enabled = True

    def respond(request):
        if request.url.path == "/api/tags":
            return httpx.Response(200, json={"models": [{"name": "llama3.2:3b"}]})
        return httpx.Response(200, json=COMPLETION)

    client, seen = capture(respond)
    with client:  # 触发 lifespan
        assert "ollama/llama3.2:3b" in {m["id"] for m in client.get("/v1/models").json()["data"]}
        r = client.post("/v1/chat/completions", json=_chat("llama3.2:3b"))
    assert r.status_code == 200
    assert str(seen[-1].url) == "http://127.0.0.1:11434/v1/chat/completions"


def test_ollama_connection_error_is_not_quota(capture, settings):
    settings.ollama.enabled = True

    def refuse(request):
        raise httpx.ConnectError("refused", request=request)

    client, _ = capture(refuse)
    r = client.post("/v1/chat/completions", json=_chat("ollama/x"))
    assert r.status_code == 502
    assert "Ollama" in r.json()["error"]["message"]


def test_unknown_model_openai_format(capture):
    client, seen = capture()
    r = client.post("/v1/chat/completions", json=_chat("gpt-4o"))
    assert r.status_code == 404
    assert r.json()["error"]["type"] == "not_found_error"
    assert seen == []


def test_upstream_error_status_and_message(capture):
    err = [{"error": {"code": 400, "message": "bad thing", "status": "INVALID_ARGUMENT"}}]
    client, _ = capture(httpx.Response(400, json=err))
    r = client.post("/v1/chat/completions", json=_chat("gemini-3.5-flash"))
    assert r.status_code == 400
    assert r.json()["error"]["message"] == "bad thing"


# ---------------------------------------------------------------- Claude 转换


CLAUDE_MSG = {
    "id": "msg_1",
    "type": "message",
    "role": "assistant",
    "content": [
        {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"city": "Paris"}}
    ],
    "stop_reason": "tool_use",
    "usage": {"input_tokens": 9, "output_tokens": 4},
}
TOOLS = [
    {
        "type": "function",
        "function": {
            "name": "get_weather",
            "parameters": {"type": "object", "properties": {"city": {"type": "string"}}},
        },
    }
]


def test_claude_non_stream_tool_call(capture):
    client, seen = capture(httpx.Response(200, json=CLAUDE_MSG))
    r = client.post(
        "/v1/chat/completions",
        json=_chat("claude-haiku-4-5", tools=TOOLS, tool_choice="required", max_tokens=20),
    )
    assert r.status_code == 200
    up = seen[0]
    assert str(up.url).endswith("/publishers/anthropic/models/claude-haiku-4-5@20251001:rawPredict")
    sent = json.loads(up.content)
    assert sent["anthropic_version"] == "vertex-2023-10-16"
    assert "model" not in sent
    assert sent["tool_choice"] == {"type": "any"}
    assert sent["tools"][0]["input_schema"]["properties"]["city"] == {"type": "string"}

    out = r.json()
    assert out["model"] == "claude-haiku-4-5"
    choice = out["choices"][0]
    assert choice["finish_reason"] == "tool_calls"
    assert json.loads(choice["message"]["tool_calls"][0]["function"]["arguments"]) == {
        "city": "Paris"
    }


def _anthropic_sse(events):
    return "".join(f"event: {e['type']}\ndata: {json.dumps(e)}\n\n" for e in events).encode()


STREAM_EVENTS = [
    {"type": "message_start", "message": {"id": "msg_s", "usage": {"input_tokens": 5}}},
    {"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "toolu_s", "name": "get_weather", "input": {}}},
    {"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": '{"city":'}},
    {"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": ' "Oslo"}'}},
    {"type": "content_block_stop", "index": 0},
    {"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 8}},
    {"type": "message_stop"},
]  # fmt: skip


def _openai_chunks(text: str) -> list:
    payloads = [line[6:] for line in text.split("\n") if line.startswith("data: ")]
    assert payloads[-1] == "[DONE]"
    return [json.loads(p) for p in payloads[:-1]]


def test_claude_stream_tool_call(capture):
    client, seen = capture(
        httpx.Response(
            200,
            content=_anthropic_sse(STREAM_EVENTS),
            headers={"content-type": "text/event-stream"},
        )
    )
    r = client.post(
        "/v1/chat/completions",
        json=_chat(
            "claude-opus-4-6", tools=TOOLS, stream=True, stream_options={"include_usage": True}
        ),
    )
    assert r.status_code == 200
    assert str(seen[0].url).endswith(":streamRawPredict")
    chunks = _openai_chunks(r.text)
    args = "".join(
        tc["function"].get("arguments", "")
        for c in chunks
        if c["choices"]
        for tc in c["choices"][0]["delta"].get("tool_calls", [])
    )
    assert json.loads(args) == {"city": "Oslo"}
    assert [c["choices"][0]["finish_reason"] for c in chunks if c["choices"]][-1] == "tool_calls"
    assert chunks[-1]["usage"]["completion_tokens"] == 8


def test_claude_stream_error_event_becomes_openai_error(capture):
    events = [
        STREAM_EVENTS[0],
        {"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}},
    ]
    client, _ = capture(httpx.Response(200, content=_anthropic_sse(events)))
    r = client.post("/v1/chat/completions", json=_chat("claude-opus-4-6", stream=True))
    last = [line for line in r.text.split("\n") if line.startswith("data: ")][-1]
    assert json.loads(last[6:])["error"]["message"] == "Overloaded"


def test_claude_stream_truncated_reports_error(capture):
    client, _ = capture(httpx.Response(200, content=_anthropic_sse(STREAM_EVENTS[:3])))
    r = client.post("/v1/chat/completions", json=_chat("claude-opus-4-6", stream=True))
    assert "[DONE]" not in r.text
    assert "提前结束" in r.text


def test_claude_upstream_error_openai_format(capture):
    err = {"type": "error", "error": {"type": "rate_limit_error", "message": "slow down"}}
    client, _ = capture(httpx.Response(429, json=err))
    r = client.post("/v1/chat/completions", json=_chat("claude-opus-4-6"))
    assert r.status_code == 429
    assert r.json()["error"] == {
        "message": "slow down",
        "type": "rate_limit_error",
        "param": None,
        "code": 429,
    }


def test_claude_url_image_rejected_before_upstream(capture):
    client, seen = capture()
    body = {
        "model": "claude-opus-4-6",
        "messages": [
            {
                "role": "user",
                "content": [{"type": "image_url", "image_url": {"url": "https://x/a.png"}}],
            }
        ],
    }
    r = client.post("/v1/chat/completions", json=body)
    assert r.status_code == 400
    assert seen == []
