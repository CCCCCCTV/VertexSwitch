//! `vertexswitch-core`：无界面运行代理核心。

use std::io::IsTerminal;
use std::net::SocketAddr;

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

const DEFAULT_ADDR: &str = "127.0.0.1:8788";

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        // 重定向到文件时不输出终端颜色控制符
        .with_ansi(std::io::stdout().is_terminal())
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let addr: SocketAddr = std::env::args()
        .nth(1)
        .as_deref()
        .unwrap_or(DEFAULT_ADDR)
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(
        "VertexSwitch {} 监听 http://{addr}",
        vertexswitch_core::VERSION
    );
    vertexswitch_core::serve(listener).await
}
