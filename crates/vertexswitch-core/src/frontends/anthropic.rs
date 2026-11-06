//! Anthropic Messages 入口（SPEC §8.1，最高优先级）：`/v1/messages`、`/v1/messages/count_tokens`。
//!
//! 请求原样转给 Vertex 上的 Claude（只改写 model / anthropic_version），响应（含 SSE 流）原样回传；
//! 流开始后出错时补发一个 `event: error`。

use axum::{
    Router, body::Bytes, extract::State, http::HeaderMap, response::Response, routing::post,
};
use serde_json::Value;

use super::{parse_json_object, require_model, wants_stream};
use crate::app::SharedState;
use crate::backends::{self, Target};
use crate::errors::{Protocol, ProxyError, ProxyResult};
use crate::registry::Backend;
use crate::runtime::Runtime;
use crate::tr;

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

/// 请求里的模型必须是 Claude；返回上游 ID。
fn require_claude(state: &Runtime, body: &Value) -> ProxyResult<String> {
    let model = require_model(body)?;
    state
        .registry()
        .resolve(&model)
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

/// 客户端的 `anthropic-beta` 原样转发；`anthropic-version` 不转发（版本在请求体里）。
fn beta(headers: &HeaderMap) -> Option<String> {
    headers
        .get("anthropic-beta")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

async fn messages(
    State(app): State<SharedState>,
    headers: HeaderMap,
    body: Bytes,
) -> ProxyResult<Response> {
    let state = app.runtime();
    let body = parse_json_object(&body)?;
    let target = Target::ClaudeMessages {
        model: require_claude(&state, &body)?,
        stream: wants_stream(&body),
        beta: beta(&headers),
    };
    backends::call(&state, target, body)
        .await?
        .relay(Protocol::Anthropic)
        .await
}

async fn count_tokens(
    State(app): State<SharedState>,
    headers: HeaderMap,
    body: Bytes,
) -> ProxyResult<Response> {
    let state = app.runtime();
    let body = parse_json_object(&body)?;
    let target = Target::ClaudeCountTokens {
        model: require_claude(&state, &body)?,
        beta: beta(&headers),
    };
    backends::call(&state, target, body)
        .await?
        .relay(Protocol::Anthropic)
        .await
}
