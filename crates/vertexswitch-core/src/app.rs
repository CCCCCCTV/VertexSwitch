//! 应用状态与路由组装。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};

use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tower_http::catch_panic::CatchPanicLayer;

use crate::admin;
use crate::auth::TokenSource;
use crate::config::Settings;
use crate::errors::{ProxyError, not_found, panic_response, render_errors};
use crate::frontends;
use crate::logs::LogHub;
use crate::metrics::{Metrics, record_requests};
use crate::registry::ModelRegistry;
use crate::runtime::Runtime;

/// 挂在路由上的应用状态（不使用全局变量）。
pub struct AppState {
    runtime: RwLock<Arc<Runtime>>,
    pub logs: Arc<LogHub>,
    pub metrics: Arc<Metrics>,
    /// 管理接口的 Bearer token
    pub admin_token: String,
    /// 配置文件；为 None 时（测试）改配置不写盘，也不监视文件
    pub config_path: Option<PathBuf>,
    /// 用户模型表；文件存在时优先于内置表，变化时自动重载
    pub models_path: Option<PathBuf>,
    refresher: Mutex<Option<JoinHandle<()>>>,
}

pub type SharedState = Arc<AppState>;

/// 应用新配置的结果。
pub struct ApplyOutcome {
    /// 监听地址变了，需要重启监听（由外壳负责）
    pub restart_required: bool,
}

impl AppState {
    pub fn new(settings: Settings, registry: ModelRegistry, tokens: TokenSource) -> Self {
        Self {
            runtime: RwLock::new(Arc::new(Runtime::new(settings, registry, tokens))),
            logs: Arc::new(LogHub::new(1000)),
            metrics: Arc::new(Metrics::default()),
            admin_token: random_token(),
            config_path: None,
            models_path: None,
            refresher: Mutex::new(None),
        }
    }

    /// 按配置构造：服务账号认证。
    pub fn from_settings(settings: Settings, registry: ModelRegistry) -> Self {
        let tokens = Runtime::service_account_tokens(&settings);
        Self::new(settings, registry, tokens)
    }

    pub fn with_logs(mut self, logs: Arc<LogHub>) -> Self {
        self.logs = logs;
        self
    }

    pub fn with_admin_token(mut self, token: String) -> Self {
        self.admin_token = token;
        self
    }

    pub fn with_paths(mut self, config: Option<PathBuf>, models: Option<PathBuf>) -> Self {
        self.config_path = config;
        self.models_path = models;
        self
    }

    /// 当前运行时快照。一个请求应只取一次并用到底。
    pub fn runtime(&self) -> Arc<Runtime> {
        self.runtime.read().expect("锁未中毒").clone()
    }

    fn set_runtime(&self, runtime: Runtime) {
        *self.runtime.write().expect("锁未中毒") = Arc::new(runtime);
    }

    /// 换上新配置并立即生效；token 来源变了就重启后台刷新。
    pub fn apply_settings(self: &Arc<Self>, settings: Settings) -> ApplyOutcome {
        let old = self.runtime();
        let restart_required = settings.server.host != old.settings.server.host
            || settings.server.port != old.settings.server.port;
        let new = old.with_settings(settings);
        let tokens_changed = !Arc::ptr_eq(&new.tokens, &old.tokens);
        self.set_runtime(new);
        if tokens_changed {
            self.start_refresher();
        }
        tracing::info!("配置已更新");
        ApplyOutcome { restart_required }
    }

    /// 换上新模型表，保留已发现的 Ollama 模型。
    pub fn set_registry(&self, mut registry: ModelRegistry) {
        let old = self.runtime();
        registry.ollama_models = old.registry().ollama_models.clone();
        self.set_runtime(old.with_registry(registry));
    }

    pub fn set_ollama_models(&self, models: std::collections::BTreeSet<String>) {
        let old = self.runtime();
        let mut registry = old.registry().clone();
        registry.ollama_models = models;
        self.set_runtime(old.with_registry(registry));
    }

    /// 从磁盘重新读取模型表（用户文件不存在时用内置表）。
    pub fn reload_models(&self) -> Result<usize, String> {
        let registry = match &self.models_path {
            Some(path) if path.exists() => {
                ModelRegistry::load_file(path).map_err(|e| e.to_string())?
            }
            _ => ModelRegistry::builtin(),
        };
        let count = registry.len();
        self.set_registry(registry);
        tracing::info!("模型表已重新加载（{count} 个别名）");
        Ok(count)
    }

    /// 从磁盘重新读取配置并应用。
    pub fn reload_config(self: &Arc<Self>) -> Result<ApplyOutcome, String> {
        let path = self
            .config_path
            .as_ref()
            .ok_or_else(|| "未指定配置文件".to_owned())?;
        let settings = Settings::load(path).map_err(|e| e.to_string())?;
        Ok(self.apply_settings(settings))
    }

    /// 启动（或重启）后台 token 刷新。
    pub fn start_refresher(self: &Arc<Self>) {
        let runtime = self.runtime();
        let interval =
            Duration::from_secs(runtime.settings.google.token_refresh_minutes.max(1) * 60);
        let tokens = runtime.tokens.clone();
        let handle = tokio::spawn(async move { tokens.run_background_refresh(interval).await });
        if let Some(old) = self.refresher.lock().expect("锁未中毒").replace(handle) {
            old.abort();
        }
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        if let Some(handle) = self.refresher.lock().ok().and_then(|mut h| h.take()) {
            handle.abort();
        }
    }
}

pub fn random_token() -> String {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).expect("系统随机数可用");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 读取管理 token；文件不存在时生成一个并写入（仅当前用户可读的配置目录下）。
pub fn load_or_create_admin_token(path: &std::path::Path) -> std::io::Result<String> {
    if let Ok(text) = std::fs::read_to_string(path) {
        let token = text.trim().to_owned();
        if !token.is_empty() {
            return Ok(token);
        }
    }
    let token = random_token();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, &token)?;
    Ok(token)
}

pub fn router(state: SharedState) -> Router {
    let protected = frontends::anthropic::routes()
        .merge(frontends::gemini::routes())
        .merge(frontends::openai::routes())
        .merge(frontends::models::routes())
        .route("/metrics", get(prometheus))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_api_key,
        ));
    let admin = admin::routes().route_layer(middleware::from_fn_with_state(
        state.clone(),
        admin::require_admin,
    ));
    Router::new()
        .route("/health", get(health))
        .merge(protected)
        .merge(admin)
        .fallback(not_found)
        .layer(CatchPanicLayer::custom(panic_response))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            record_requests,
        ))
        .layer(middleware::from_fn(render_errors))
        .with_state(state)
}

/// 运行代理直到出错；后台刷新 Google token、监视配置与模型表文件。
pub async fn serve(listener: TcpListener, state: SharedState) -> std::io::Result<()> {
    let _background = start_background(&state).await;
    serve_until(listener, state, std::future::pending()).await
}

/// 后台任务（token 刷新、文件监视）；丢弃时停止文件监视。
pub struct Background {
    watcher: JoinHandle<()>,
}

impl Drop for Background {
    fn drop(&mut self) {
        self.watcher.abort();
    }
}

/// 启动后台任务：token 刷新、配置与模型表文件监视；启用 Ollama 时先发现一次模型。
/// 与监听无关，换端口重启监听时不需要重来。
pub async fn start_background(state: &SharedState) -> Background {
    state.start_refresher();
    let watcher = tokio::spawn(watch_files(Arc::downgrade(state)));
    if state.runtime().settings.ollama.enabled {
        frontends::openai::refresh_ollama_models(state).await;
    }
    Background { watcher }
}

/// 只运行 HTTP 服务，`shutdown` 完成时优雅停止（等进行中的请求结束）。
pub async fn serve_until(
    listener: TcpListener,
    state: SharedState,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let app = router(state).into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
}

fn mtime(path: &Option<PathBuf>) -> Option<SystemTime> {
    std::fs::metadata(path.as_ref()?).ok()?.modified().ok()
}

/// 每 2 秒检查配置与模型表文件，有变化就热加载；内容有误时保留原配置并告警。
async fn watch_files(state: std::sync::Weak<AppState>) {
    let (mut config_seen, mut models_seen) = match state.upgrade() {
        Some(s) => (mtime(&s.config_path), mtime(&s.models_path)),
        None => return,
    };
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let Some(state) = state.upgrade() else {
            return;
        };
        let config_now = mtime(&state.config_path);
        if config_now != config_seen {
            config_seen = config_now;
            if let Some(path) = &state.config_path
                && config_now.is_some()
            {
                match Settings::load(path) {
                    Ok(settings) if settings != state.runtime().settings => {
                        state.apply_settings(settings);
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("配置文件有误，保留当前配置：{e}"),
                }
            }
        }
        let models_now = mtime(&state.models_path);
        if models_now != models_seen {
            models_seen = models_now;
            if let Err(e) = state.reload_models() {
                tracing::warn!("模型表有误，保留当前模型表：{e}");
            }
        }
    }
}

/// Anthropic 用 x-api-key，OpenAI 用 Bearer，Gemini 用 x-goog-api-key 或 ?key=（SPEC §6）。
fn extract_key(request: &Request) -> Option<String> {
    let headers = request.headers();
    if let Some(token) = bearer(headers) {
        return Some(token);
    }
    header_str(headers, "x-api-key")
        .or_else(|| header_str(headers, "x-goog-api-key"))
        .map(str::to_owned)
        .or_else(|| {
            let query = request.uri().query()?;
            form_urlencoded::parse(query.as_bytes())
                .find(|(k, _)| k == "key")
                .map(|(_, v)| v.into_owned())
        })
}

pub fn bearer(headers: &HeaderMap) -> Option<String> {
    let auth = header_str(headers, "authorization")?;
    (auth.len() > 7 && auth[..7].eq_ignore_ascii_case("bearer "))
        .then(|| auth[7..].trim().to_owned())
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// 常数时间比较（长度不同直接不等）。
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn require_api_key(
    State(state): State<SharedState>,
    request: Request,
    next: Next,
) -> Response {
    let expected = state.runtime().settings.server.api_key.clone();
    if !expected.is_empty() {
        let given = extract_key(&request).unwrap_or_default();
        if !constant_time_eq(given.as_bytes(), expected.as_bytes()) {
            return ProxyError::new(401, "API key 无效或缺失").into_response();
        }
    }
    next.run(request).await
}

async fn prometheus(State(state): State<SharedState>) -> Response {
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.metrics.prometheus(),
    )
        .into_response()
}

/// 健康检查信息（/health 与管理接口共用）。
pub async fn health_info(state: &AppState) -> serde_json::Value {
    let runtime = state.runtime();
    let settings = &runtime.settings;
    let mut info = json!({
        "status": "ok",
        "version": crate::VERSION,
        "region": settings.google.region,
        "gemini_region": settings.gemini_region(),
        "gemini_tier": settings.gemini.tier,
    });
    match runtime.tokens.get_token().await {
        Ok(_) => {
            info["google_token"] = "ok".into();
            info["project_id"] = runtime.project_id().unwrap_or_default().into();
        }
        Err(e) => {
            info["status"] = "degraded".into();
            info["google_token"] = e.0.into();
        }
    }
    info
}

async fn health(State(state): State<SharedState>) -> Response {
    let info = health_info(&state).await;
    let status = if info["status"] == "ok" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(info)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ct_eq() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secret2"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn admin_token_file_created_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("admin_token");
        let first = load_or_create_admin_token(&path).unwrap();
        assert_eq!(first.len(), 48);
        assert_eq!(load_or_create_admin_token(&path).unwrap(), first);
    }
}
