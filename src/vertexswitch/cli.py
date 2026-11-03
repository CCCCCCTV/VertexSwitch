"""``vertexswitch-core``：命令行启动代理核心。"""

from __future__ import annotations

import argparse
import logging
from pathlib import Path

import uvicorn

from .app import create_app
from .core.settings import config_path, load_settings


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(prog="vertexswitch-core", description="VertexSwitch 代理核心")
    parser.add_argument("--config", type=Path, help=f"配置文件路径（默认 {config_path()}）")
    parser.add_argument("--host", help="覆盖监听地址")
    parser.add_argument("--port", type=int, help="覆盖监听端口")
    parser.add_argument("--log-level", default="info")
    args = parser.parse_args(argv)

    logging.basicConfig(
        level=args.log_level.upper(), format="%(asctime)s %(levelname)s %(name)s: %(message)s"
    )
    settings = load_settings(args.config)
    if args.host:
        settings.server.host = args.host
    if args.port:
        settings.server.port = args.port

    uvicorn.run(
        create_app(settings),
        host=settings.server.host,
        port=settings.server.port,
        log_level=args.log_level,
        # 默认 5 秒会和客户端连接池的复用竞争，导致偶发的 "Server disconnected"
        timeout_keep_alive=settings.server.keep_alive_seconds,
    )


if __name__ == "__main__":
    main()
