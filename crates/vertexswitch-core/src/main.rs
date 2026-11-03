//! `vertexswitch-core`：无界面运行代理核心。

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
use vertexswitch_core::AppState;
use vertexswitch_core::config::{Settings, config_path};
use vertexswitch_core::registry::ModelRegistry;

#[derive(Parser)]
#[command(name = "vertexswitch-core", version, about = "VertexSwitch 代理核心")]
struct Args {
    /// 配置文件路径（默认 %APPDATA%\VertexSwitch\config.toml）
    #[arg(long)]
    config: Option<PathBuf>,
    /// 覆盖监听地址
    #[arg(long)]
    host: Option<String>,
    /// 覆盖监听端口
    #[arg(long)]
    port: Option<u16>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        // 重定向到文件时不输出终端颜色控制符
        .with_ansi(std::io::stdout().is_terminal())
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let mut settings = Settings::load(&args.config.unwrap_or_else(config_path))?;
    if let Some(host) = args.host {
        settings.server.host = host;
    }
    if let Some(port) = args.port {
        settings.server.port = port;
    }
    let registry = ModelRegistry::load()?;
    let addr = format!("{}:{}", settings.server.host, settings.server.port);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!(
        "VertexSwitch {} 监听 http://{addr}",
        vertexswitch_core::VERSION
    );
    let state = Arc::new(AppState::from_settings(settings, registry));
    vertexswitch_core::serve(listener, state).await?;
    Ok(())
}
