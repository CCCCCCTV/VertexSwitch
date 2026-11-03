import httpx
import pytest

from vertexswitch.core.errors import ProxyError
from vertexswitch.core.settings import RetrySettings, TimeoutSettings
from vertexswitch.core.transport import Transport

pytestmark = pytest.mark.anyio


class FakeClock:
    """每次 send 前后推进固定时长，模拟上游挂住。"""

    def __init__(self):
        self.now = 0.0

    def __call__(self) -> float:
        return self.now


def _transport(handler, clock=None, attempts=2):
    sleeps: list[float] = []

    async def sleep(s):
        sleeps.append(s)

    t = Transport(
        httpx.AsyncClient(transport=httpx.MockTransport(handler)),
        TimeoutSettings(),
        RetrySettings(attempts=attempts, backoff_seconds=0.4),
        sleep=sleep,
        clock=clock or FakeClock(),
    )
    return t, sleeps


async def _open(t):
    return await t.open("POST", "https://x/y", headers={}, content=b"{}", stream=False)


async def test_retries_connect_errors_then_succeeds():
    calls = 0

    def handler(request):
        nonlocal calls
        calls += 1
        if calls < 3:
            raise httpx.ConnectError("boom", request=request)
        return httpx.Response(200, json={"ok": True})

    t, sleeps = _transport(handler)
    resp = await _open(t)
    assert resp.status_code == 200
    assert calls == 3
    assert sleeps == pytest.approx([0.4, 0.8])


async def test_gives_up_with_502_after_retries():
    calls = 0

    def handler(request):
        nonlocal calls
        calls += 1
        raise httpx.RemoteProtocolError("Server disconnected", request=request)

    t, _ = _transport(handler)
    with pytest.raises(ProxyError) as exc:
        await _open(t)
    assert exc.value.status == 502
    assert calls == 3


async def test_long_hang_then_disconnect_is_quota_504_without_retry():
    clock = FakeClock()
    calls = 0

    def handler(request):
        nonlocal calls
        calls += 1
        clock.now += 25
        raise httpx.RemoteProtocolError("Server disconnected", request=request)

    t, sleeps = _transport(handler, clock=clock)
    with pytest.raises(ProxyError) as exc:
        await _open(t)
    assert exc.value.status == 504
    assert "配额" in exc.value.message
    assert calls == 1
    assert sleeps == []


async def test_long_hang_connect_error_is_still_retried():
    # ConnectError 说明连接都没建立，不是配额特征
    clock = FakeClock()
    calls = 0

    def handler(request):
        nonlocal calls
        calls += 1
        clock.now += 25
        if calls == 1:
            raise httpx.ConnectError("refused", request=request)
        return httpx.Response(200)

    t, _ = _transport(handler, clock=clock)
    assert (await _open(t)).status_code == 200


async def test_read_timeout_is_504_without_retry():
    calls = 0

    def handler(request):
        nonlocal calls
        calls += 1
        raise httpx.ReadTimeout("slow", request=request)

    t, _ = _transport(handler)
    with pytest.raises(ProxyError) as exc:
        await _open(t)
    assert exc.value.status == 504
    assert calls == 1


async def test_stream_uses_idle_timeout():
    seen = {}

    def handler(request):
        seen.update(request.extensions["timeout"])
        return httpx.Response(200)

    t, _ = _transport(handler)
    await t.open("POST", "https://x/y", headers={}, content=b"", stream=True)
    assert seen["read"] == TimeoutSettings().stream_idle
    await _open(t)
    assert seen["read"] == TimeoutSettings().read
