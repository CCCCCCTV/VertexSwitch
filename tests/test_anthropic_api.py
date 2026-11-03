import json

import httpx
import pytest

GLOBAL_BASE = (
    "https://aiplatform.googleapis.com/v1/projects/proj/locations/global"
    "/publishers/anthropic/models"
)

MESSAGE = {
    "id": "msg_1",
    "type": "message",
    "role": "assistant",
    "content": [{"type": "text", "text": "hi"}],
    "model": "claude-opus-4-6",
    "stop_reason": "end_turn",
    "usage": {"input_tokens": 3, "output_tokens": 1},
}


def _req(**extra):
    body = {
        "model": "claude-haiku-4-5",
        "max_tokens": 10,
        "messages": [{"role": "user", "content": "hi"}],
    }
    body.update(extra)
    return body


def test_non_stream_rewrites_and_forwards(make_client):
    seen = []

    def handler(request: httpx.Request):
        seen.append(request)
        return httpx.Response(200, json=MESSAGE)

    client = make_client(handler)
    r = client.post(
        "/v1/messages",
        json=_req(context_management={"edits": []}),
        headers={"anthropic-beta": "foo-2025", "anthropic-version": "2023-06-01"},
    )
    assert r.status_code == 200
    assert r.json() == MESSAGE

    (up,) = seen
    assert str(up.url) == f"{GLOBAL_BASE}/claude-haiku-4-5@20251001:rawPredict"
    assert up.headers["authorization"] == "Bearer tok"
    assert up.headers["anthropic-beta"] == "foo-2025"
    assert "anthropic-version" not in up.headers
    sent = json.loads(up.content)
    assert "model" not in sent
    assert "context_management" not in sent
    assert sent["anthropic_version"] == "vertex-2023-10-16"


def test_anthropic_prefix_route(make_client):
    client = make_client(lambda r: httpx.Response(200, json=MESSAGE))
    assert client.post("/anthropic/v1/messages", json=_req()).status_code == 200


def test_stream_passthrough(make_client):
    sse = (
        b'event: message_start\ndata: {"type":"message_start"}\n\n'
        b'event: message_stop\ndata: {"type":"message_stop"}\n\n'
    )
    seen = []

    def handler(request):
        seen.append(str(request.url))
        return httpx.Response(200, content=sse, headers={"content-type": "text/event-stream"})

    client = make_client(handler)
    r = client.post("/v1/messages", json=_req(stream=True))
    assert r.status_code == 200
    assert r.headers["content-type"].startswith("text/event-stream")
    assert r.content == sse
    assert seen[0].endswith(":streamRawPredict")


class BrokenStream(httpx.AsyncByteStream):
    async def __aiter__(self):
        yield b'event: message_start\ndata: {"type":"message_start"}\n\nevent: content_bl'
        raise httpx.ReadError("connection reset")

    async def aclose(self):
        pass


def test_stream_error_after_start_emits_error_event(make_client):
    client = make_client(lambda r: httpx.Response(200, stream=BrokenStream()))
    r = client.post("/v1/messages", json=_req(stream=True))
    assert r.status_code == 200
    text = r.text
    assert text.startswith("event: message_start")
    tail = text.rsplit("event: error\n", 1)[1]
    payload = json.loads(tail.removeprefix("data: ").strip())
    assert payload["type"] == "error"
    assert payload["error"]["type"] == "api_error"


def test_upstream_anthropic_error_passthrough(make_client):
    err = {"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}
    client = make_client(lambda r: httpx.Response(529, json=err))
    r = client.post("/v1/messages", json=_req(stream=True))
    assert r.status_code == 529
    assert r.json() == err


def test_upstream_google_error_converted(make_client):
    google = [{"error": {"code": 429, "message": "Quota exceeded", "status": "RESOURCE_EXHAUSTED"}}]
    client = make_client(lambda r: httpx.Response(429, json=google))
    r = client.post("/v1/messages", json=_req())
    assert r.status_code == 429
    assert r.json() == {
        "type": "error",
        "error": {"type": "rate_limit_error", "message": "Quota exceeded"},
    }


def test_connection_failure_returns_anthropic_502(make_client):
    def handler(request):
        raise httpx.ConnectError("refused", request=request)

    client = make_client(handler)
    r = client.post("/v1/messages", json=_req())
    assert r.status_code == 502
    body = r.json()
    assert body["type"] == "error"
    assert body["error"]["type"] == "api_error"


@pytest.mark.parametrize(
    ("payload", "status"),
    [
        (b"not json", 400),
        (b"[1,2]", 400),
        (json.dumps({"max_tokens": 1}).encode(), 400),
        (json.dumps({"model": "gpt-4o", "max_tokens": 1}).encode(), 404),
    ],
)
def test_bad_requests_use_anthropic_format(make_client, payload, status):
    client = make_client(lambda r: pytest.fail("不应请求上游"))
    r = client.post("/v1/messages", content=payload, headers={"content-type": "application/json"})
    assert r.status_code == status
    assert r.json()["type"] == "error"


def test_count_tokens(make_client):
    seen = []

    def handler(request):
        seen.append(request)
        return httpx.Response(200, json={"input_tokens": 7})

    client = make_client(handler)
    r = client.post(
        "/v1/messages/count_tokens",
        json={"model": "claude-opus-4-6", "messages": [{"role": "user", "content": "hi"}]},
    )
    assert r.status_code == 200
    assert r.json() == {"input_tokens": 7}
    assert str(seen[0].url) == f"{GLOBAL_BASE}/count-tokens:rawPredict"
    assert json.loads(seen[0].content)["model"] == "claude-opus-4-6"


def test_api_key_required_when_configured(make_client, settings):
    settings.server.api_key = "secret"
    client = make_client(lambda r: httpx.Response(200, json=MESSAGE))
    assert client.post("/v1/messages", json=_req()).status_code == 401
    assert client.get("/v1/models").status_code == 401
    ok = client.post("/v1/messages", json=_req(), headers={"x-api-key": "secret"})
    assert ok.status_code == 200
    ok = client.post("/v1/messages", json=_req(), headers={"Authorization": "Bearer secret"})
    assert ok.status_code == 200
    assert client.get("/health").status_code == 200


def test_unknown_route_error_format(make_client):
    client = make_client(lambda r: httpx.Response(200))
    r = client.get("/v1/nope")
    assert r.status_code == 404
    assert "error" in r.json()


def test_models_list_formats(make_client):
    client = make_client(lambda r: httpx.Response(200))
    anth = client.get("/v1/models", headers={"anthropic-version": "2023-06-01"}).json()
    assert anth["data"][0]["type"] == "model"
    oai = client.get("/v1/models").json()
    assert oai["object"] == "list"
    assert client.get("/v1/models/claude-opus-4-6").json()["id"] == "claude-opus-4-6"


def test_health_reports_token(make_client):
    client = make_client(lambda r: httpx.Response(200))
    body = client.get("/health").json()
    assert body["status"] == "ok"
    assert body["project_id"] == "proj"
