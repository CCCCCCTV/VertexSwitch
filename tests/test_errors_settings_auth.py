import pytest

from vertexswitch.core.auth import AuthError, ServiceAccountTokenProvider
from vertexswitch.core.errors import (
    Protocol,
    ProxyError,
    error_body,
    from_upstream,
    protocol_for_path,
    stream_error_event,
)
from vertexswitch.core.settings import Settings, load_settings, save_settings


def test_protocol_for_path():
    assert protocol_for_path("/v1/messages") is Protocol.ANTHROPIC
    assert protocol_for_path("/anthropic/v1/models") is Protocol.ANTHROPIC
    assert protocol_for_path("/v1beta/models/x:generateContent") is Protocol.GEMINI
    assert protocol_for_path("/v1/chat/completions") is Protocol.OPENAI


def test_error_body_formats():
    err = ProxyError(504, "slow")
    assert error_body(Protocol.ANTHROPIC, err) == {
        "type": "error",
        "error": {"type": "timeout_error", "message": "slow"},
    }
    assert error_body(Protocol.GEMINI, err)["error"]["status"] == "DEADLINE_EXCEEDED"
    assert error_body(Protocol.OPENAI, err)["error"]["message"] == "slow"


def test_stream_error_event_shapes():
    err = ProxyError(502, "bad")
    assert b"event: error\ndata: " in stream_error_event(Protocol.ANTHROPIC, err)
    assert stream_error_event(Protocol.OPENAI, err).strip().startswith(b'data: {"error"')


def test_from_upstream_non_json():
    err = from_upstream(503, b"<html>Service Unavailable</html>")
    assert err.status == 503
    assert "Service Unavailable" in err.message


def test_settings_roundtrip(tmp_path):
    s = Settings()
    s.server.port = 9999
    s.google.region = "us"
    path = save_settings(s, tmp_path / "config.toml")
    assert load_settings(path) == s


def test_missing_settings_file_gives_defaults(tmp_path):
    s = load_settings(tmp_path / "nope.toml")
    assert s.server.port == 8788
    assert s.google.region == "global"


@pytest.mark.anyio
async def test_auth_missing_key_file_raises_auth_error(tmp_path):
    provider = ServiceAccountTokenProvider(str(tmp_path / "missing.json"))
    with pytest.raises(AuthError):
        await provider.get_token()
    assert provider.last_error


@pytest.mark.anyio
async def test_auth_not_configured():
    with pytest.raises(AuthError, match="未配置"):
        await ServiceAccountTokenProvider("").get_token()
