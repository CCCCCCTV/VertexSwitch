//! OpenAI Chat Completions 入口（SPEC §8.3）：`/v1/chat/completions` 及若干前缀别名。
//!
//! 按 `model` 分流：
//! - Gemini → Vertex 的 OpenAI 兼容接口（原样转发，**不带档位头**，Vertex 会对该接口返回 400）
//! - MaaS 开放模型 → 同一接口，`publisher/model`
//! - Ollama → 本地 `/v1/chat/completions`（不需要 Google token，不按 Vertex 配额特征处理）
//! - Claude → 转成 Anthropic Messages 调 Vertex，再把响应（含流式）转回 OpenAI 格式

use std::time::Duration;

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
    routing::post,
};
use futures_util::StreamExt;
use serde_json::Value;

use super::{parse_json_object, relay};
use crate::app::{AppState, SharedState};
use crate::backends::{claude, vertex};
use crate::errors::{Protocol, ProxyError, ProxyResult};
use crate::registry::{Backend, Route};
use crate::translate::openai_to_anthropic::{
    SseParser, StreamTranslator, request_to_anthropic, response_to_openai,
};
use crate::transport::{UpstreamRequest, VERTEX};

const PATHS: &[&str] = &[
    "/v1/chat/completions",
    "/chat/completions",
    "/openai/v1/chat/completions",
    "/api/v1/chat/completions",
];
pub const OLLAMA: &str = "Ollama";

pub fn routes() -> Router<SharedState> {
    PATHS.iter().fold(Router::new(), |router, path| {
        router.route(path, post(chat_completions))
    })
}

async fn chat_completions(State(state): State<SharedState>, body: Bytes) -> ProxyResult<Response> {
    let body = parse_json_object(&body)?;
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .ok_or_else(|| ProxyError::new(400, "model: 缺少模型名"))?
        .to_owned();
    let route = state
        .registry()
        .resolve(&model)
        .ok_or_else(|| ProxyError::new(404, format!("未知模型 {model:?}")))?;
    match route.backend {
        Backend::Claude => via_claude(&state, &body, &route, &model).await,
        _ => passthrough(&state, body, &route).await,
    }
}

fn sse_data(payload: &str) -> Bytes {
    format!("data: {payload}\n\n").into()
}

async fn passthrough(state: &AppState, mut body: Value, route: &Route) -> ProxyResult<Response> {
    let settings = &state.settings;
    let mut headers = vec![("content-type".to_owned(), "application/json".to_owned())];
    let (url, upstream_model, upstream, detect_quota) = match route.backend {
        Backend::Ollama => {
            if !settings.ollama.enabled {
                return Err(ProxyError::new(
                    400,
                    "Ollama 未启用（ollama.enabled = false）",
                ));
            }
            let url = format!(
                "{}/v1/chat/completions",
                settings.ollama.base_url.trim_end_matches('/')
            );
            (url, route.upstream_model.clone(), OLLAMA, false)
        }
        backend => {
            headers.push((
                "authorization".into(),
                format!("Bearer {}", state.google_token().await?),
            ));
            let (region, version, model) = if backend == Backend::Gemini {
                (
                    settings.gemini_region(),
                    &settings.gemini.openai_api_version,
                    format!("google/{}", route.upstream_model),
                )
            } else {
                (
                    settings.maas_region(),
                    &settings.maas.api_version,
                    route.upstream_model.clone(),
                )
            };
            let url = vertex::openapi_chat_url(
                &state.vertex_host(region),
                version,
                &state.project_id()?,
                region,
            );
            (url, model, VERTEX, true)
        }
    };
    body["model"] = upstream_model.into();
    let stream = body.get("stream").and_then(Value::as_bool) == Some(true);
    let mut request = UpstreamRequest::post_json(
        url,
        headers,
        serde_json::to_vec(&body).expect("Value 可序列化").into(),
        stream,
    );
    request.upstream = upstream;
    request.detect_quota = detect_quota;

    let response = state.transport.open(&request).await?;
    let status = response.status();
    if !status.is_success() {
        let raw = state.transport.read_body(response).await?;
        return Err(ProxyError::from_upstream(status.as_u16(), &raw));
    }
    let mut out = if stream {
        let mut r = relay(
            state.transport.chunks(response, upstream),
            Protocol::OpenAi,
            |_| {},
        );
        r.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        r
    } else {
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .cloned()
            .unwrap_or_else(|| HeaderValue::from_static("application/json"));
        let data = state.transport.read_body(response).await?;
        let mut r = Response::new(Body::from(data));
        r.headers_mut().insert(header::CONTENT_TYPE, content_type);
        r
    };
    *out.status_mut() = status;
    Ok(out)
}

async fn via_claude(
    state: &AppState,
    body: &Value,
    route: &Route,
    client_model: &str,
) -> ProxyResult<Response> {
    let mut anthropic = request_to_anthropic(body, state.settings.openai.default_max_tokens)?;
    let stream = anthropic.get("stream").and_then(Value::as_bool) == Some(true);
    let headers = vec![
        (
            "authorization".to_owned(),
            format!("Bearer {}", state.google_token().await?),
        ),
        ("content-type".to_owned(), "application/json".to_owned()),
    ];
    let region = &state.settings.google.region;
    let publisher = vertex::publisher_models_url(
        &state.vertex_base(region),
        &state.project_id()?,
        region,
        "anthropic",
    );
    let url = claude::messages_url(&publisher, &route.upstream_model, stream);
    claude::messages_body(&mut anthropic, &state.settings.claude);
    let request = UpstreamRequest::post_json(
        url,
        headers,
        serde_json::to_vec(&anthropic)
            .expect("Value 可序列化")
            .into(),
        stream,
    );
    let response = state.transport.open(&request).await?;
    let status = response.status();
    if !status.is_success() {
        let raw = state.transport.read_body(response).await?;
        return Err(ProxyError::from_upstream(status.as_u16(), &raw));
    }

    if !stream {
        let data = state.transport.read_body(response).await?;
        let message: Value = serde_json::from_slice(&data)
            .map_err(|_| ProxyError::new(502, "Vertex AI 返回了无法解析的响应"))?;
        return Ok(Json(response_to_openai(&message, client_model)).into_response());
    }

    let include_usage = body
        .get("stream_options")
        .and_then(|o| o.get("include_usage"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut translator = StreamTranslator::new(client_model, include_usage);
    let chunks = state.transport.chunks(response, VERTEX);
    let events = async_stream::stream! {
        futures_util::pin_mut!(chunks);
        let mut parser = SseParser::default();
        while let Some(item) = chunks.next().await {
            let chunk = match item {
                Ok(chunk) => chunk,
                Err(e) => {
                    yield Ok::<_, std::convert::Infallible>(e.stream_event(Protocol::OpenAi));
                    return;
                }
            };
            for (event, raw) in parser.feed(&chunk) {
                let Ok(data) = serde_json::from_str::<Value>(&raw) else {
                    tracing::warn!("无法解析的上游 SSE 数据：{}", raw.chars().take(200).collect::<String>());
                    continue;
                };
                if event == "error" || data.get("type").and_then(Value::as_str) == Some("error") {
                    let message = data
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(Value::as_str)
                        .unwrap_or("上游返回错误事件");
                    let err = ProxyError::new(500, message);
                    yield Ok(sse_data(&err.body(Protocol::OpenAi).to_string()));
                    return;
                }
                for out in translator.handle(&event, &data) {
                    yield Ok(sse_data(&out.to_string()));
                }
            }
        }
        if translator.finished {
            yield Ok(sse_data("[DONE]"));
        } else {
            yield Ok(ProxyError::new(502, "Vertex AI 流式响应提前结束").stream_event(Protocol::OpenAi));
        }
    };
    let mut r = Response::new(Body::from_stream(events));
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    Ok(r)
}

/// 从 Ollama `/api/tags` 发现本地模型；Ollama 没运行时只记日志，保留原有列表。
pub async fn refresh_ollama_models(state: &AppState) -> usize {
    let url = format!(
        "{}/api/tags",
        state.settings.ollama.base_url.trim_end_matches('/')
    );
    let result = async {
        let response = state
            .transport
            .client
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await?
            .error_for_status()?;
        response.json::<Value>().await
    }
    .await;
    match result {
        Ok(data) => {
            let models: std::collections::BTreeSet<String> = data
                .get("models")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|m| m.get("name").and_then(Value::as_str).map(str::to_owned))
                .collect();
            let count = models.len();
            state.registry.write().expect("锁未中毒").ollama_models = models;
            tracing::info!("发现 {count} 个 Ollama 模型");
            count
        }
        Err(e) => {
            tracing::warn!("Ollama 模型发现失败：{e}");
            state.registry().aliases(Backend::Ollama).len()
        }
    }
}
