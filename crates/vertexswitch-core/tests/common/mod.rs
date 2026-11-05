//! 集成测试共用：用 wiremock 假 Vertex + 固定 token 起一个完整的代理。

#![allow(dead_code)]

use std::sync::Arc;

use tokio::net::TcpListener;
use vertexswitch_core::AppState;
use vertexswitch_core::auth::TokenSource;
use vertexswitch_core::config::Settings;
use vertexswitch_core::registry::ModelRegistry;
use wiremock::MockServer;

pub struct Harness {
    pub base: String,
    pub upstream: MockServer,
    pub state: Arc<AppState>,
    pub client: reqwest::Client,
}

impl Harness {
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// 带管理 token 的请求构造器。
    pub fn admin(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, self.url(path))
            .bearer_auth(&self.state.admin_token)
    }

    /// 假上游上发往 publisher 模型的路径前缀（global 区域、项目 proj）。
    pub fn publisher_path(publisher: &str, region: &str) -> String {
        format!("/v1/projects/proj/locations/{region}/publishers/{publisher}/models")
    }
}

/// 默认测试配置：重试不等待。
pub fn settings() -> Settings {
    let mut s = Settings::default();
    s.retry.backoff_seconds = 0.0;
    s.retry.busy_wait = "0".into();
    s
}

pub async fn start(settings: Settings) -> Harness {
    start_on(settings, MockServer::start().await).await
}

/// 在调用方事先准备好（已挂载部分 mock）的假上游上启动；Vertex 与 Ollama 都指向它。
pub async fn start_on(mut settings: Settings, upstream: MockServer) -> Harness {
    settings.google.api_base_override = Some(upstream.uri());
    settings.ollama.base_url = upstream.uri();
    launch(settings, upstream).await
}

/// 上游地址由调用方在 `settings.google.api_base_override` 里指定（如故意坏掉的上游）。
pub async fn start_with_override(settings: Settings) -> Harness {
    launch(settings, MockServer::start().await).await
}

async fn launch(settings: Settings, upstream: MockServer) -> Harness {
    launch_with(settings, upstream, |s| s).await
}

/// 带配置文件 / 模型表路径启动（管理接口与热加载测试用）；Vertex 与 Ollama 指向假上游。
pub async fn start_with_paths(
    mut settings: Settings,
    config: Option<std::path::PathBuf>,
    models: Option<std::path::PathBuf>,
) -> Harness {
    let upstream = MockServer::start().await;
    settings.google.api_base_override = Some(upstream.uri());
    settings.ollama.base_url = upstream.uri();
    launch_with(settings, upstream, |s| s.with_paths(config, models)).await
}

async fn launch_with(
    settings: Settings,
    upstream: MockServer,
    customize: impl FnOnce(AppState) -> AppState,
) -> Harness {
    let state = Arc::new(customize(AppState::new(
        settings,
        ModelRegistry::builtin(),
        TokenSource::fixed("tok", "proj"),
    )));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(vertexswitch_core::serve(listener, state.clone()));
    Harness {
        base,
        upstream,
        state,
        client: reqwest::Client::new(),
    }
}
