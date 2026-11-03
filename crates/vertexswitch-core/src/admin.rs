//! 管理接口 `/admin/api/*`：状态、配置读写、模型表、日志与请求记录（含 SSE 实时流）、指标、
//! 测试请求。只接受本机连接，并要求独立的管理 token（`Authorization: Bearer <token>`）。
//!
//! 图形界面与命令行 curl 用的是同一套接口。

use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Instant;

use axum::{
    Json, Router,
    body::Body,
    extract::{ConnectInfo, Query, Request, State},
    middleware::Next,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures_util::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::app::{SharedState, bearer, constant_time_eq, health_info, router};
use crate::config::Settings;
use crate::errors::{ProxyError, ProxyResult};
use crate::registry::Backend;

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/admin/api/status", get(status))
        .route(
            "/admin/api/config",
            get(get_config).put(put_config).patch(patch_config),
        )
        .route("/admin/api/config/reload", post(reload_config))
        .route("/admin/api/models", get(models))
        .route("/admin/api/models/reload", post(reload_models))
        .route("/admin/api/ollama/refresh", post(refresh_ollama))
        .route("/admin/api/logs", get(logs))
        .route("/admin/api/logs/stream", get(logs_stream))
        .route("/admin/api/requests", get(requests))
        .route("/admin/api/requests/stream", get(requests_stream))
        .route("/admin/api/metrics", get(metrics))
        .route("/admin/api/test", post(test_request))
}

/// 只接受本机连接 + 管理 token。
pub async fn require_admin(
    State(state): State<SharedState>,
    request: Request,
    next: Next,
) -> Response {
    let local = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .is_none_or(|ConnectInfo(addr)| addr.ip().is_loopback());
    if !local {
        return ProxyError::new(403, "管理接口只接受本机连接").into_response();
    }
    let given = bearer(request.headers()).unwrap_or_default();
    if !constant_time_eq(given.as_bytes(), state.admin_token.as_bytes()) {
        return ProxyError::new(401, "管理 token 无效或缺失").into_response();
    }
    next.run(request).await
}

async fn status(State(state): State<SharedState>) -> Json<Value> {
    let runtime = state.runtime();
    let registry = runtime.registry();
    let mut info = health_info(&state).await;
    info["listen"] = format!(
        "{}:{}",
        runtime.settings.server.host, runtime.settings.server.port
    )
    .into();
    info["uptime_secs"] = state.metrics.uptime_secs().into();
    info["in_flight"] = state.metrics.in_flight().into();
    info["models"] = json!({
        "claude": registry.aliases(Backend::Claude).len(),
        "gemini": registry.aliases(Backend::Gemini).len(),
        "maas": registry.aliases(Backend::Maas).len(),
        "ollama": registry.aliases(Backend::Ollama).len(),
    });
    info["config_path"] = state
        .config_path
        .as_ref()
        .map(|p| p.display().to_string())
        .into();
    Json(info)
}

async fn get_config(State(state): State<SharedState>) -> Json<Settings> {
    Json(state.runtime().settings.clone())
}

/// 校验、写盘（有配置文件时）并立即应用。
fn save_and_apply(state: &SharedState, settings: Settings) -> ProxyResult<Json<Value>> {
    if let Some(path) = &state.config_path {
        settings
            .save(path)
            .map_err(|e| ProxyError::new(500, format!("写入配置文件失败：{e}")))?;
    }
    let outcome = state.apply_settings(settings);
    Ok(Json(json!({
        "settings": state.runtime().settings,
        "restart_required": outcome.restart_required,
    })))
}

fn parse_settings(value: Value) -> ProxyResult<Settings> {
    serde_json::from_value(value).map_err(|e| ProxyError::new(400, format!("配置格式错误：{e}")))
}

async fn put_config(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> ProxyResult<Json<Value>> {
    let settings = parse_settings(body)?;
    save_and_apply(&state, settings)
}

/// JSON Merge Patch（RFC 7396）：只改给出的字段，`null` 表示恢复默认。
async fn patch_config(
    State(state): State<SharedState>,
    Json(patch): Json<Value>,
) -> ProxyResult<Json<Value>> {
    let mut current = serde_json::to_value(&state.runtime().settings).expect("配置可序列化");
    merge_patch(&mut current, &patch);
    let settings = parse_settings(current)?;
    save_and_apply(&state, settings)
}

pub fn merge_patch(target: &mut Value, patch: &Value) {
    let Value::Object(patch) = patch else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = json!({});
    }
    let map = target.as_object_mut().expect("刚确保是对象");
    for (key, value) in patch {
        if value.is_null() {
            map.remove(key);
        } else {
            merge_patch(map.entry(key.clone()).or_insert(Value::Null), value);
        }
    }
}

async fn reload_config(State(state): State<SharedState>) -> ProxyResult<Json<Value>> {
    let outcome = state.reload_config().map_err(|e| ProxyError::new(400, e))?;
    Ok(Json(json!({
        "settings": state.runtime().settings,
        "restart_required": outcome.restart_required,
    })))
}

async fn models(State(state): State<SharedState>) -> Json<Value> {
    let runtime = state.runtime();
    let registry = runtime.registry();
    Json(json!({
        "claude": table_json(registry.entries(Backend::Claude)),
        "gemini": table_json(registry.entries(Backend::Gemini)),
        "maas": table_json(registry.entries(Backend::Maas)),
        "ollama": registry.aliases(Backend::Ollama),
        "source": match &state.models_path {
            Some(p) if p.exists() => p.display().to_string(),
            _ => "builtin".to_owned(),
        },
    }))
}

fn table_json(entries: &[(String, String)]) -> Value {
    Value::Object(
        entries
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect(),
    )
}

async fn reload_models(State(state): State<SharedState>) -> ProxyResult<Json<Value>> {
    let count = state
        .reload_models()
        .map_err(|e| ProxyError::new(400, format!("模型表有误：{e}")))?;
    Ok(Json(json!({ "aliases": count })))
}

async fn refresh_ollama(State(state): State<SharedState>) -> Json<Value> {
    let count = crate::frontends::openai::refresh_ollama_models(&state).await;
    Json(json!({ "ollama_models": count }))
}

#[derive(Deserialize)]
struct Since {
    /// 只返回序号大于它的条目
    since: Option<u64>,
    limit: Option<usize>,
}

async fn logs(State(state): State<SharedState>, Query(q): Query<Since>) -> Json<Value> {
    let since = q.since.unwrap_or(0);
    Json(json!(
        state.logs.recent(q.limit.unwrap_or(200), |e| e.seq > since)
    ))
}

async fn requests(State(state): State<SharedState>, Query(q): Query<Since>) -> Json<Value> {
    let since = q.since.unwrap_or(0);
    Json(json!(
        state
            .metrics
            .requests
            .recent(q.limit.unwrap_or(200), |r| r.seq > since)
    ))
}

/// 广播流 → SSE。订阅者跟不上时跳过丢失的条目（客户端可用 `?since=` 补拉）。
fn sse<T: serde::Serialize + Clone + Send + 'static>(
    rx: tokio::sync::broadcast::Receiver<T>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream =
        broadcast_stream(rx).map(|item| Ok(Event::default().json_data(item).expect("可序列化")));
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn logs_stream(State(state): State<SharedState>) -> impl IntoResponse {
    sse(state.logs.subscribe())
}

async fn requests_stream(State(state): State<SharedState>) -> impl IntoResponse {
    sse(state.metrics.requests.subscribe())
}

async fn metrics(State(state): State<SharedState>) -> Json<Value> {
    Json(state.metrics.summary())
}

#[derive(Deserialize)]
struct TestRequest {
    model: String,
    #[serde(default = "default_prompt")]
    prompt: String,
    #[serde(default = "default_test_tokens")]
    max_tokens: u64,
}

fn default_prompt() -> String {
    "Reply with exactly: pong".into()
}

fn default_test_tokens() -> u64 {
    32
}

/// 用 OpenAI Chat Completions 入口在进程内发一个最小请求，验证模型与凭据是否可用。
/// 会真实调用上游并产生少量费用。
async fn test_request(
    State(state): State<SharedState>,
    Json(req): Json<TestRequest>,
) -> ProxyResult<Json<Value>> {
    let body = json!({
        "model": req.model,
        "messages": [{"role": "user", "content": req.prompt}],
        "max_tokens": req.max_tokens,
    });
    let mut builder =
        Request::post("/v1/chat/completions").header("content-type", "application/json");
    let api_key = state.runtime().settings.server.api_key.clone();
    if !api_key.is_empty() {
        builder = builder.header("authorization", format!("Bearer {api_key}"));
    }
    let request = builder
        .body(Body::from(body.to_string()))
        .expect("请求可构造");
    let started = Instant::now();
    let response = router(state.clone())
        .oneshot(request)
        .await
        .unwrap_or_else(|e| match e {});
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .map_err(|e| ProxyError::new(502, format!("读取测试响应失败：{e}")))?;
    let parsed: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Ok(Json(json!({
        "ok": (200..300).contains(&status),
        "status": status,
        "latency_ms": started.elapsed().as_millis() as u64,
        "reply": parsed["choices"][0]["message"]["content"],
        "error": parsed.get("error"),
    })))
}

/// 广播接收端 → Stream。订阅者跟不上时跳过丢失的条目；发送端关闭时结束。
fn broadcast_stream<T: Clone + Send + 'static>(
    rx: tokio::sync::broadcast::Receiver<T>,
) -> impl Stream<Item = T> + Send {
    use tokio::sync::broadcast::error::RecvError;
    futures_util::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(item) => return Some((item, rx)),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return None,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_patch_rfc7396() {
        let mut target = json!({"a": {"b": 1, "c": 2}, "d": 3});
        merge_patch(&mut target, &json!({"a": {"b": 9, "c": null}, "e": [1]}));
        assert_eq!(target, json!({"a": {"b": 9}, "d": 3, "e": [1]}));
    }
}
