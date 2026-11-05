//! VertexSwitch 代理核心：在本机提供 Anthropic / Gemini / OpenAI 接口，转发到 Vertex AI。
//!
//! 行为规格见仓库根目录的 `docs/SPEC.md`。

pub mod admin;
pub mod app;
pub mod auth;
pub mod backends;
pub mod config;
pub mod errors;
pub mod frontends;
pub mod i18n;
pub mod logs;
pub mod metrics;
pub mod registry;
pub mod runtime;
pub mod secrets;
pub mod tiers;
pub mod translate;
pub mod transport;
pub mod wait_formula;

pub use app::{AppState, SharedState, router, serve, serve_until, start_background};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
