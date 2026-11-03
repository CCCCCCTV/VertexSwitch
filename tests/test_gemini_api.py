import json
import logging

import httpx
import pytest

from vertexswitch.core.tiers import HEADER_REQUEST_TYPE, HEADER_SERVER_TIMEOUT, HEADER_SHARED_TYPE

GLOBAL_BASE = (
    "https://aiplatform.googleapis.com/v1/projects/proj/locations/global/publishers/google/models"
)
BODY = {"contents": [{"role": "user", "parts": [{"text": "hi"}]}]}


def _resp(traffic="ON_DEMAND"):
    return {
        "candidates": [{"content": {"role": "model", "parts": [{"text": "yo"}]}}],
        "usageMetadata": {"promptTokenCount": 1, "trafficType": traffic},
    }


@pytest.fixture
def capture(make_client):
    seen: list[httpx.Request] = []

    def factory(response=None, **kw):
        def handler(request):
            seen.append(request)
            return response or httpx.Response(200, json=_resp())

        return make_client(handler, **kw), seen

    return factory


def test_generate_content_url_and_standard_headers(capture):
    client, seen = capture()
    r = client.post("/v1beta/models/gemini-3-flash:generateContent", json=BODY)
    assert r.status_code == 200
    assert r.json() == _resp()
    assert r.headers["x-vertexswitch-tier"] == "standard"
    (up,) = seen
    assert str(up.url) == f"{GLOBAL_BASE}/gemini-3-flash-preview:generateContent"
    assert up.headers["authorization"] == "Bearer tok"
    assert HEADER_SHARED_TYPE not in up.headers
    assert HEADER_REQUEST_TYPE not in up.headers


def test_gemini_prefix_and_models_prefix(capture):
    client, seen = capture()
    r = client.post("/gemini/v1beta/models/models/gemini-2.5-pro:generateContent", json=BODY)
    assert r.status_code == 200
    assert str(seen[0].url).endswith("/gemini-2.5-pro:generateContent")


def test_stream_passes_query_but_not_key(capture, settings):
    settings.server.api_key = "k"
    sse = b'data: {"usageMetadata": {"trafficType": "ON_DEMAND"}}\r\n\r\n'
    client, seen = capture(
        httpx.Response(200, content=sse, headers={"content-type": "text/event-stream"})
    )
    r = client.post(
        "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse&key=k", json=BODY
    )
    assert r.status_code == 200
    assert r.content == sse
    assert r.headers["content-type"].startswith("text/event-stream")
    assert seen[0].url.params.get("alt") == "sse"
    assert "key" not in seen[0].url.params


def test_goog_api_key_header_accepted(capture, settings):
    settings.server.api_key = "k"
    client, _ = capture()
    url = "/v1beta/models/gemini-2.5-flash:generateContent"
    assert client.post(url, json=BODY).status_code == 401
    assert client.post(url, json=BODY, headers={"x-goog-api-key": "k"}).status_code == 200


def test_flex_headers_sent_and_local_timeout_extended(capture, settings):
    settings.gemini.tier = "flex"
    client, seen = capture(httpx.Response(200, json=_resp("ON_DEMAND_FLEX")))
    r = client.post("/v1beta/models/gemini-3.5-flash:generateContent", json=BODY)
    assert r.headers["x-vertexswitch-tier"] == "flex"
    up = seen[0]
    assert up.headers[HEADER_SHARED_TYPE] == "flex"
    assert up.headers[HEADER_SERVER_TIMEOUT] == "1800"
    assert up.extensions["timeout"]["read"] >= 1800


def test_flex_on_argon_downgrades(capture, settings, caplog):
    settings.gemini.tier = "flex"
    client, seen = capture()
    with caplog.at_level(logging.WARNING):
        r = client.post("/v1beta/models/gemini-4-argon:generateContent", json=BODY)
    assert r.headers["x-vertexswitch-tier"] == "standard"
    assert HEADER_SHARED_TYPE not in seen[0].headers
    assert "不支持 Flex" in caplog.text


def test_flex_non_global_region_downgrades(capture, settings):
    settings.gemini.tier = "flex"
    settings.gemini.region = "us-central1"
    client, seen = capture()
    client.post("/v1beta/models/gemini-3.5-flash:generateContent", json=BODY)
    assert str(seen[0].url).startswith("https://us-central1-aiplatform.googleapis.com/")
    assert HEADER_SHARED_TYPE not in seen[0].headers


def test_priority_on_us_multiregion(capture, settings):
    settings.gemini.tier = "priority"
    settings.gemini.region = "us"
    client, seen = capture(httpx.Response(200, json=_resp("ON_DEMAND_PRIORITY")))
    client.post("/v1beta/models/gemini-3.5-flash:generateContent", json=BODY)
    assert str(seen[0].url).startswith("https://aiplatform.us.rep.googleapis.com/v1/")
    assert "/locations/us/" in str(seen[0].url)
    assert seen[0].headers[HEADER_SHARED_TYPE] == "priority"


def test_tier_override_header(capture):
    client, seen = capture()
    client.post(
        "/v1beta/models/gemini-3.5-flash:generateContent",
        json=BODY,
        headers={"X-VertexSwitch-Tier": "priority"},
    )
    assert seen[0].headers[HEADER_SHARED_TYPE] == "priority"
    assert "x-vertexswitch-tier" not in seen[0].headers


def test_count_tokens_never_gets_tier_headers(capture, settings):
    settings.gemini.tier = "flex"
    client, seen = capture(httpx.Response(200, json={"totalTokens": 3}))
    r = client.post("/v1beta/models/gemini-3.5-flash:countTokens", json=BODY)
    assert r.json() == {"totalTokens": 3}
    assert str(seen[0].url).endswith(":countTokens")
    assert HEADER_SHARED_TYPE not in seen[0].headers
    assert "x-vertexswitch-tier" not in r.headers


def test_traffic_mismatch_warns_once_in_stream(capture, settings, caplog):
    settings.gemini.tier = "flex"
    chunk = b'data: {"usageMetadata": {"trafficType": "ON_DEMAND"}}\r\n\r\n'
    client, _ = capture(
        httpx.Response(200, content=chunk * 3, headers={"content-type": "text/event-stream"})
    )
    with caplog.at_level(logging.WARNING):
        client.post("/v1beta/models/gemini-3.5-flash:streamGenerateContent?alt=sse", json=BODY)
    assert caplog.text.count("实际按 ON_DEMAND 处理") == 1


def test_traffic_match_no_warning(capture, settings, caplog):
    settings.gemini.tier = "flex"
    client, _ = capture(httpx.Response(200, json=_resp("ON_DEMAND_FLEX")))
    with caplog.at_level(logging.WARNING):
        client.post("/v1beta/models/gemini-3.5-flash:generateContent", json=BODY)
    assert "实际按" not in caplog.text


def test_missing_role_filled_with_user(capture):
    client, seen = capture()
    client.post(
        "/v1beta/models/gemini-2.5-flash:generateContent",
        json={"contents": [{"parts": [{"text": "hi"}]}]},
    )
    assert json.loads(seen[0].content)["contents"][0]["role"] == "user"


def test_google_error_passthrough(capture):
    err = {"error": {"code": 429, "message": "Resource exhausted", "status": "RESOURCE_EXHAUSTED"}}
    client, _ = capture(httpx.Response(429, json=[err]))
    r = client.post("/v1beta/models/gemini-2.5-flash:generateContent", json=BODY)
    assert r.status_code == 429
    assert r.json() == err


@pytest.mark.parametrize(
    "path",
    [
        "/v1beta/models/gemini-2.5-flash:predict",
        "/v1beta/models/gemini-2.5-flash",
        "/v1beta/models/claude-opus-4-6:generateContent",
    ],
)
def test_bad_targets_gemini_error_format(capture, path):
    client, seen = capture()
    r = client.post(path, json=BODY)
    assert r.status_code == 404
    assert r.json()["error"]["status"] == "NOT_FOUND"
    assert seen == []


def test_connection_error_gemini_format(make_client):
    def handler(request):
        raise httpx.ConnectError("refused", request=request)

    r = make_client(handler).post("/v1beta/models/gemini-2.5-flash:generateContent", json=BODY)
    assert r.status_code == 502
    assert r.json()["error"]["status"] == "UNAVAILABLE"


def test_v1_generate_content_path_uses_gemini_errors(capture):
    client, _ = capture()
    r = client.post("/v1/models/gemini-nope-x:predict", json=BODY)
    assert r.status_code == 404
    assert r.json()["error"]["status"] == "NOT_FOUND"


def test_gemini_models_list(capture):
    client, _ = capture()
    models = client.get("/v1beta/models").json()["models"]
    assert "models/gemini-2.5-pro" in {m["name"] for m in models}
    assert client.get("/v1beta/models/gemini-2.5-pro").json()["name"] == "models/gemini-2.5-pro"
    assert client.get("/v1beta/models/nope").status_code == 404
