//! VertexSwitch 代理核心：在本机提供 Anthropic / Gemini / OpenAI 接口，转发到 Vertex AI。
//!
//! 行为规格见仓库根目录的 `docs/SPEC.md`。

use axum::{Json, Router, routing::get};
use serde_json::{Value, json};
use tokio::net::TcpListener;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 组装全部路由。
pub fn router() -> Router {
    Router::new().route("/health", get(health))
}

/// 在给定监听器上运行代理，直到出错或进程退出。
pub async fn serve(listener: TcpListener) -> std::io::Result<()> {
    axum::serve(listener, router()).await
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "version": VERSION }))
}
