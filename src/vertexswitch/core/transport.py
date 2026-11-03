"""向上游发请求：超时、重试、配额耗尽识别。

只在**收到任何响应字节之前**连接出问题时重试。Vertex 在配额耗尽时常见的表现是
挂住 20 秒以上然后不发任何响应就断开（本应返回 429），这种情况不重试，直接返回 504。

状态码约定：504 = 等上游超时，502 = 上游连接或响应坏了。
"""

from __future__ import annotations

import asyncio
import logging
import time
from collections.abc import Awaitable, Callable

import httpx

from .errors import ProxyError
from .settings import RetrySettings, TimeoutSettings

logger = logging.getLogger(__name__)

# 在拿到响应头之前发生、可以安全重试的错误
_RETRYABLE = (
    httpx.ConnectError,
    httpx.ConnectTimeout,
    httpx.PoolTimeout,
    httpx.RemoteProtocolError,
    httpx.ReadError,
    httpx.WriteError,
)
# 这几种表示"服务端主动断开且没有任何响应"，挂得够久就是配额耗尽
_DISCONNECT = (httpx.RemoteProtocolError, httpx.ReadError)

QUOTA_MESSAGE = (
    "Vertex AI 在 {seconds:.0f} 秒内没有返回任何响应就断开了连接，"
    "通常表示配额耗尽（RESOURCE_EXHAUSTED）。请稍后重试或在 Google Cloud 控制台检查配额。"
)


class Transport:
    def __init__(
        self,
        client: httpx.AsyncClient,
        timeouts: TimeoutSettings,
        retry: RetrySettings,
        *,
        sleep: Callable[[float], Awaitable[None]] = asyncio.sleep,
        clock: Callable[[], float] = time.monotonic,
    ):
        self.client = client
        self.timeouts = timeouts
        self.retry = retry
        self._sleep = sleep
        self._clock = clock

    def _timeout(self, stream: bool, min_read: float | None) -> httpx.Timeout:
        t = self.timeouts
        read = t.stream_idle if stream else t.read
        if min_read is not None:
            read = max(read, min_read)
        return httpx.Timeout(
            connect=t.connect,
            read=read,
            write=t.write,
            pool=t.pool,
        )

    async def open(
        self,
        method: str,
        url: str,
        *,
        headers: dict[str, str],
        content: bytes,
        stream: bool,
        min_read_timeout: float | None = None,
    ) -> httpx.Response:
        """发出请求并返回已收到响应头、尚未读取正文的响应。调用方负责 ``aclose()``。

        ``min_read_timeout``：上游可能合法地排队这么久（如 Flex 的 X-Server-Timeout），
        本地读超时不得小于它。
        """
        attempts = self.retry.attempts + 1
        for attempt in range(1, attempts + 1):
            timeout = self._timeout(stream, min_read_timeout)
            request = self.client.build_request(
                method, url, headers=headers, content=content, timeout=timeout
            )
            started = self._clock()
            try:
                return await self.client.send(request, stream=True)
            except httpx.ReadTimeout as e:
                raise ProxyError(504, "等待 Vertex AI 响应超时") from e
            except httpx.WriteTimeout as e:
                raise ProxyError(504, "向 Vertex AI 发送请求超时") from e
            except _RETRYABLE as e:
                elapsed = self._clock() - started
                if isinstance(e, _DISCONNECT) and elapsed >= self.retry.quota_hang_seconds:
                    logger.warning("上游 %.1fs 无响应后断开，按配额耗尽处理：%r", elapsed, e)
                    raise ProxyError(504, QUOTA_MESSAGE.format(seconds=elapsed)) from e
                if attempt >= attempts:
                    raise ProxyError(
                        502, f"无法连接 Vertex AI（已重试 {attempt - 1} 次）：{_describe(e)}"
                    ) from e
                logger.info("上游连接错误，第 %d 次重试：%r", attempt, e)
                await self._sleep(self.retry.backoff_seconds * attempt)
            except httpx.HTTPError as e:
                raise ProxyError(502, f"请求 Vertex AI 失败：{_describe(e)}") from e
        raise AssertionError("unreachable")

    @staticmethod
    async def read_body(response: httpx.Response) -> bytes:
        """读取非流式响应正文；读取过程中断开映射为 502/504。"""
        try:
            return await response.aread()
        except httpx.ReadTimeout as e:
            raise ProxyError(504, "读取 Vertex AI 响应超时") from e
        except httpx.HTTPError as e:
            raise ProxyError(502, f"读取 Vertex AI 响应失败：{_describe(e)}") from e
        finally:
            await response.aclose()


def _describe(e: Exception) -> str:
    text = str(e)
    return f"{type(e).__name__}: {text}" if text else type(e).__name__
