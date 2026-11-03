"""Google 服务账号 access token 管理。

- 服务账号 JSON → OAuth access token（scope cloud-platform，有效期约 60 分钟）
- 并发获取共用一把锁，只有一个协程真正刷新
- 后台定时刷新；刷新失败只记日志，不让进程退出
"""

from __future__ import annotations

import asyncio
import datetime as dt
import logging
from typing import Protocol

logger = logging.getLogger(__name__)

SCOPES = ("https://www.googleapis.com/auth/cloud-platform",)
# token 剩余有效期少于这个值就提前刷新
_EARLY_REFRESH = dt.timedelta(minutes=5)


class AuthError(Exception):
    pass


class TokenProvider(Protocol):
    project_id: str

    async def get_token(self) -> str: ...


class StaticTokenProvider:
    """固定 token，用于测试。"""

    def __init__(self, token: str = "test-token", project_id: str = "test-project"):
        self._token = token
        self.project_id = project_id

    async def get_token(self) -> str:
        return self._token


class ServiceAccountTokenProvider:
    def __init__(self, credentials_file: str, project_id: str = ""):
        self._credentials_file = credentials_file
        self._explicit_project = project_id
        self._credentials = None
        self._lock = asyncio.Lock()
        self.project_id = project_id
        self.last_error: str | None = None

    def _load(self):
        from google.oauth2 import service_account

        if not self._credentials_file:
            raise AuthError("未配置服务账号密钥文件（google.credentials_file）")
        try:
            creds = service_account.Credentials.from_service_account_file(
                self._credentials_file, scopes=SCOPES
            )
        except (OSError, ValueError) as e:
            raise AuthError(f"无法读取服务账号密钥文件：{e}") from e
        if not self._explicit_project:
            self.project_id = creds.project_id or ""
        return creds

    def _refresh_sync(self) -> None:
        from google.auth.exceptions import GoogleAuthError
        from google.auth.transport.requests import Request

        if self._credentials is None:
            self._credentials = self._load()
        try:
            self._credentials.refresh(Request())
        except GoogleAuthError as e:
            raise AuthError(f"获取 Google access token 失败：{e}") from e

    def _fresh(self) -> bool:
        creds = self._credentials
        if creds is None or not creds.token or creds.expiry is None:
            return False
        # google-auth 的 expiry 是不带时区的 UTC 时间
        now = dt.datetime.now(dt.UTC).replace(tzinfo=None)
        return creds.expiry - now > _EARLY_REFRESH

    async def get_token(self) -> str:
        if self._fresh():
            return self._credentials.token
        async with self._lock:
            if not self._fresh():
                await self.refresh()
            return self._credentials.token

    async def refresh(self) -> None:
        try:
            await asyncio.to_thread(self._refresh_sync)
        except AuthError as e:
            self.last_error = str(e)
            raise
        self.last_error = None
        logger.info("Google access token 已刷新")

    async def run_background_refresh(self, interval_minutes: float) -> None:
        """后台循环：启动时先取一次，然后按间隔刷新。异常只记录。"""
        while True:
            try:
                async with self._lock:
                    await self.refresh()
            except AuthError as e:
                logger.warning("%s", e)
            await asyncio.sleep(interval_minutes * 60)
