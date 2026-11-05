//! `vertexswitch-core`：无界面运行代理核心。

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use vertexswitch_core::AppState;
use vertexswitch_core::config::{Settings, app_dir, config_path};
use vertexswitch_core::logs::{LogHub, LogLayer};
use vertexswitch_core::registry::ModelRegistry;
use vertexswitch_core::secrets::load_admin_token;
use vertexswitch_core::tr;

#[derive(Parser)]
#[command(name = "vertexswitch-core", version, about = "VertexSwitch 代理核心")]
struct Args {
    /// 配置文件路径（默认 %APPDATA%\VertexSwitch\config.toml）
    #[arg(long)]
    config: Option<PathBuf>,
    /// 覆盖监听地址（只作用于本次运行；通过管理接口保存配置时会一并写入）
    #[arg(long)]
    host: Option<String>,
    /// 覆盖监听端口（同上）
    #[arg(long)]
    port: Option<u16>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// 打印管理接口的 token（保存在系统凭据库里）
    AdminToken,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    vertexswitch_core::i18n::load();
    let logs = Arc::new(LogHub::new(1000));
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                // 重定向到文件时不输出终端颜色控制符
                .with_ansi(std::io::stdout().is_terminal()),
        )
        .with(LogLayer::new(logs.clone()))
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let args = Args::parse();
    if let Some(Command::AdminToken) = args.command {
        println!("{}", load_admin_token().0);
        return Ok(());
    }
    let config_file = args.config.unwrap_or_else(config_path);
    let mut settings = Settings::load(&config_file)?;
    // 命令行覆盖只作用于本次运行，不写回配置文件
    if let Some(host) = args.host {
        settings.server.host = host;
    }
    if let Some(port) = args.port {
        settings.server.port = port;
    }
    let models_file = app_dir().join("models.toml");
    let registry = ModelRegistry::load()?;
    let (admin_token, token_location) = load_admin_token();

    let addr = format!("{}:{}", settings.server.host, settings.server.port);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!(
        "{}",
        tr!(
            "VertexSwitch {} 监听 http://{addr}",
            "VertexSwitch {} listening on http://{addr}",
            vertexswitch_core::VERSION
        )
    );
    tracing::info!(
        "{}",
        tr!(
            "管理接口 token 位于{}（vertexswitch-core admin-token 可打印）",
            "Admin token is stored in {} (print it with: vertexswitch-core admin-token)",
            token_location.describe()
        )
    );

    let state = Arc::new(
        AppState::from_settings(settings, registry)
            .with_logs(logs)
            .with_admin_token(admin_token)
            .with_paths(Some(config_file), Some(models_file)),
    );
    vertexswitch_core::serve(listener, state).await?;
    Ok(())
}
