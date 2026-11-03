//! 运行时状态与路由组装。

use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::json;
use tokio::net::TcpListener;
use tower_http::catch_panic::CatchPanicLayer;

use crate::auth::{ServiceAccountAuth, TokenSource};
use crate::backends::vertex;
use crate::config::Settings;
use crate::errors::{ProxyError, ProxyResult, not_found, panic_response, render_errors};
use crate::frontends;
use crate::registry::ModelRegistry;
use crate::transport::Transport;

/// 挂在路由上的运行时状态（不使用全局变量）。
pub struct AppState {
    pub settings: Settings,
    pub registry: RwLock<ModelRegistry>,
    pub tokens: TokenSource,
    pub transport: Transport,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new(settings: Settings, registry: ModelRegistry, tokens: TokenSource) -> Self {
        let transport = Transport::new(settings.timeouts.clone(), settings.retry.clone());
        Self {
            settings,
            registry: RwLock::new(registry),
            tokens,
            transport,
        }
    }

    /// 按配置构造：服务账号认证 + 用户模型表。
    pub fn from_settings(settings: Settings, registry: ModelRegistry) -> Self {
        let http = reqwest::Client::new();
        let tokens = TokenSource::ServiceAccount(Box::new(ServiceAccountAuth::new(
            settings.credentials_file(),
            &settings.google.project_id,
            http,
        )));
        Self::new(settings, registry, tokens)
    }

    pub fn registry(&self) -> std::sync::RwLockReadGuard<'_, ModelRegistry> {
        self.registry.read().expect("锁未中毒")
    }

    pub async fn google_token(&self) -> ProxyResult<String> {
        self.tokens
            .get_token()
            .await
            .map_err(|e| ProxyError::new(500, e.0))
    }

    /// 项目 ID：配置优先，否则来自密钥文件（须在取过 token 之后调用）。
    pub fn project_id(&self) -> ProxyResult<String> {
        let project = if self.settings.google.project_id.is_empty() {
            self.tokens.project_id()
        } else {
            self.settings.google.project_id.clone()
        };
        if project.is_empty() {
            return Err(ProxyError::new(
                500,
                "未配置 Google Cloud 项目 ID（google.project_id）",
            ));
        }
        Ok(project)
    }

    pub fn vertex_base(&self, region: &str) -> String {
        vertex::base_url(region, self.settings.google.api_base_override.as_deref())
    }
}

pub fn router(state: SharedState) -> Router {
    let protected = frontends::gemini::routes().route_layer(middleware::from_fn_with_state(
        state.clone(),
        require_api_key,
    ));
    Router::new()
        .route("/health", get(health))
        .merge(protected)
        .fallback(not_found)
        .layer(CatchPanicLayer::custom(panic_response))
        .layer(middleware::from_fn(render_errors))
        .with_state(state)
}

/// 运行代理直到出错；同时在后台定时刷新 Google token。
pub async fn serve(listener: TcpListener, state: SharedState) -> std::io::Result<()> {
    let refresher = {
        let state = state.clone();
        let interval = Duration::from_secs(state.settings.google.token_refresh_minutes.max(1) * 60);
        tokio::spawn(async move { state.tokens.run_background_refresh(interval).await })
    };
    let result = axum::serve(listener, router(state)).await;
    refresher.abort();
    result
}

/// Anthropic 用 x-api-key，OpenAI 用 Bearer，Gemini 用 x-goog-api-key 或 ?key=（SPEC §6）。
fn extract_key(request: &Request) -> Option<String> {
    let headers = request.headers();
    if let Some(auth) = header_str(headers, "authorization")
        && auth.len() > 7
        && auth[..7].eq_ignore_ascii_case("bearer ")
    {
        return Some(auth[7..].trim().to_owned());
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

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// 常数时间比较（长度不同直接不等）。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn require_api_key(
    State(state): State<SharedState>,
    request: Request,
    next: Next,
) -> Response {
    let expected = &state.settings.server.api_key;
    if !expected.is_empty() {
        let given = extract_key(&request).unwrap_or_default();
        if !constant_time_eq(given.as_bytes(), expected.as_bytes()) {
            return ProxyError::new(401, "API key 无效或缺失").into_response();
        }
    }
    next.run(request).await
}

async fn health(State(state): State<SharedState>) -> Response {
    let settings = &state.settings;
    let mut info = json!({
        "status": "ok",
        "version": crate::VERSION,
        "region": settings.google.region,
        "gemini_region": settings.gemini_region(),
        "gemini_tier": settings.gemini.tier,
    });
    match state.tokens.get_token().await {
        Ok(_) => {
            info["google_token"] = "ok".into();
            info["project_id"] = state.project_id().unwrap_or_default().into();
        }
        Err(e) => {
            info["status"] = "degraded".into();
            info["google_token"] = e.0.into();
        }
    }
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
}
