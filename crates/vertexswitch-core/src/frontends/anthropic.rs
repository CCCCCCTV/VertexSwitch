//! Anthropic Messages 入口（SPEC §8.1，最高优先级）：`/v1/messages`、`/v1/messages/count_tokens`。
//!
//! 请求原样转给 Vertex 上的 Claude（只改写 model / anthropic_version），响应（含 SSE 流）原样回传。
//! 流开始后出错时补发一个 `event: error`。

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, HeaderValue, header},
    response::Response,
    routing::post,
};
use serde_json::Value;

use super::{parse_json_object, relay};
use crate::app::SharedState;
use crate::backends::{claude, vertex};
use crate::errors::{Protocol, ProxyError, ProxyResult};
use crate::metrics;
use crate::registry::Backend;
use crate::runtime::Runtime;
use crate::tr;
use crate::transport::{UpstreamRequest, VERTEX};

/// 透传给 Vertex 的客户端请求头。客户端的 anthropic-version 不转发（版本在请求体里）。
const FORWARD_HEADERS: &[&str] = &["anthropic-beta"];

pub fn routes() -> Router<SharedState> {
    let mut router = Router::new();
    for prefix in ["", "/anthropic"] {
        router = router
            .route(&format!("{prefix}/v1/messages"), post(messages))
            .route(
                &format!("{prefix}/v1/messages/count_tokens"),
                post(count_tokens),
            );
    }
    router
}

fn require_claude(state: &Runtime, body: &Value) -> ProxyResult<String> {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .ok_or_else(|| {
            ProxyError::new(400, tr!("model: 缺少模型名", "model: missing model name"))
        })?;
    state
        .registry()
        .resolve(model)
        .filter(|r| r.backend == Backend::Claude)
        .map(|r| r.upstream_model)
        .ok_or_else(|| {
            ProxyError::new(
                404,
                tr!(
                    "model: 未知的 Claude 模型 {model:?}",
                    "model: unknown Claude model {model:?}"
                ),
            )
        })
}

fn note_claude(model: &str, stream: bool) {
    metrics::note(|i| {
        i.model = Some(model.to_owned());
        i.backend = Some(Backend::Claude.as_str());
        i.stream = stream;
    });
}

/// 先取 token（项目 ID 可能要从密钥文件里读出来），再拼上游请求头。
async fn upstream_headers(
    state: &Runtime,
    client_headers: &HeaderMap,
) -> ProxyResult<Vec<(String, String)>> {
    let mut headers = vec![
        (
            "authorization".to_owned(),
            format!("Bearer {}", state.google_token().await?),
        ),
        ("content-type".to_owned(), "application/json".to_owned()),
    ];
    for name in FORWARD_HEADERS {
        if let Some(value) = client_headers.get(*name).and_then(|v| v.to_str().ok()) {
            headers.push(((*name).to_owned(), value.to_owned()));
        }
    }
    Ok(headers)
}

fn publisher_models(state: &Runtime) -> ProxyResult<String> {
    let region = &state.settings.google.region;
    Ok(vertex::publisher_models_url(
        &state.vertex_base(region),
        &state.project_id()?,
        region,
        "anthropic",
    ))
}

fn content_type_of(response: &reqwest::Response) -> HeaderValue {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"))
}

async fn messages(
    State(app): State<SharedState>,
    headers: HeaderMap,
    body: Bytes,
) -> ProxyResult<Response> {
    let state = app.runtime();
    let mut body = parse_json_object(&body)?;
    let upstream_model = require_claude(&state, &body)?;
    let stream = body.get("stream").and_then(Value::as_bool) == Some(true);
    note_claude(&upstream_model, stream);
    let upstream_headers = upstream_headers(&state, &headers).await?;
    let url = claude::messages_url(&publisher_models(&state)?, &upstream_model, stream);
    claude::messages_body(&mut body, &state.settings.claude);

    let request = UpstreamRequest::post_json(
        url,
        upstream_headers,
        serde_json::to_vec(&body).expect("Value 可序列化").into(),
        stream,
    );
    let response = state.transport.open(&request).await?;
    let status = response.status();
    if !status.is_success() {
        let raw = state.transport.read_body(response).await?;
        return Err(ProxyError::from_upstream(status.as_u16(), &raw));
    }

    let mut out = if stream {
        let mut r = relay(
            state.transport.chunks(response, VERTEX),
            Protocol::Anthropic,
            |_| {},
        );
        r.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        r
    } else {
        let content_type = content_type_of(&response);
        let data = state.transport.read_body(response).await?;
        let mut r = Response::new(Body::from(data));
        r.headers_mut().insert(header::CONTENT_TYPE, content_type);
        r
    };
    *out.status_mut() = status;
    Ok(out)
}

async fn count_tokens(
    State(app): State<SharedState>,
    headers: HeaderMap,
    body: Bytes,
) -> ProxyResult<Response> {
    let state = app.runtime();
    let mut body = parse_json_object(&body)?;
    let upstream_model = require_claude(&state, &body)?;
    note_claude(&upstream_model, false);
    let upstream_headers = upstream_headers(&state, &headers).await?;
    let url = claude::count_tokens_url(&publisher_models(&state)?);
    claude::count_tokens_body(&mut body, &state.settings.claude, &upstream_model);

    let request = UpstreamRequest::post_json(
        url,
        upstream_headers,
        serde_json::to_vec(&body).expect("Value 可序列化").into(),
        false,
    );
    let response = state.transport.open(&request).await?;
    let status = response.status();
    let content_type = content_type_of(&response);
    let data = state.transport.read_body(response).await?;
    if !status.is_success() {
        return Err(ProxyError::from_upstream(status.as_u16(), &data));
    }
    let mut out = Response::new(Body::from(data));
    *out.status_mut() = status;
    out.headers_mut().insert(header::CONTENT_TYPE, content_type);
    Ok(out)
}
