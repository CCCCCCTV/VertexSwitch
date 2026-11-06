//! Anthropic Messages 入口（SPEC §8.1，最高优先级）：`/v1/messages`、`/v1/messages/count_tokens`。
//!
//! - Claude 模型：请求原样转给 Vertex 上的 Claude（只改写 model / anthropic_version），响应（含 SSE 流）
//!   原样回传；流开始后出错时补发一个 `event: error`
//! - Gemini 模型：翻译成 Gemini 原生 generateContent / countTokens（SPEC §14），响应翻译回 Claude 格式；
//!   服务档位与 Gemini 原生入口一样生效

use axum::{
    Router, body::Bytes, extract::State, http::HeaderMap, response::Response, routing::post,
};
use serde_json::Value;

use super::{parse_json_object, require_model, wants_stream};
use crate::app::SharedState;
use crate::backends::{self, Target, gemini};
use crate::errors::{Protocol, ProxyError, ProxyResult};
use crate::registry::{Backend, Route};
use crate::runtime::Runtime;
use crate::tr;
use crate::translate::anthropic_to_gemini;

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

/// 请求里的模型：Claude 或 Gemini（MaaS 开放模型不走这个入口）。返回 (客户端写的名字, 路由)。
fn require_route(state: &Runtime, body: &Value) -> ProxyResult<(String, Route)> {
    let model = require_model(body)?;
    let route = state
        .registry()
        .resolve(&model)
        .filter(|r| matches!(r.backend, Backend::Claude | Backend::Gemini))
        .ok_or_else(|| {
            ProxyError::new(
                404,
                tr!(
                    "model: 未知模型 {model:?}（这个接口可以调用 Claude 和 Gemini 模型）",
                    "model: unknown model {model:?} (this endpoint serves Claude and Gemini models)"
                ),
            )
        })?;
    Ok((model, route))
}

/// 客户端的 `anthropic-beta` 原样转发给 Claude；`anthropic-version` 不转发（版本在请求体里）。
fn beta(headers: &HeaderMap) -> Option<String> {
    headers
        .get("anthropic-beta")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

fn tier_override(headers: &HeaderMap) -> Option<String> {
    headers
        .get(gemini::TIER_OVERRIDE_HEADER)
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
    let (client_model, route) = require_route(&state, &body)?;
    let stream = wants_stream(&body);
    if route.backend == Backend::Claude {
        let target = Target::ClaudeMessages {
            model: route.upstream_model,
            stream,
            beta: beta(&headers),
        };
        return backends::call(&state, target, body)
            .await?
            .relay(Protocol::Anthropic)
            .await;
    }

    let request = anthropic_to_gemini::request_to_gemini(&body, &route.upstream_model)?;
    let target = Target::GeminiNative {
        model: route.upstream_model,
        action: if stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        }
        .to_owned(),
        query: if stream { "alt=sse" } else { "" }.to_owned(),
        tier_override: tier_override(&headers),
    };
    let upstream = backends::call(&state, target, request).await?;
    if stream {
        let translator = anthropic_to_gemini::StreamTranslator::new(&client_model);
        Ok(upstream.translate_stream(Protocol::Anthropic, translator))
    } else {
        upstream
            .translate_json(|response| {
                anthropic_to_gemini::response_to_anthropic(response, &client_model)
            })
            .await
    }
}

async fn count_tokens(
    State(app): State<SharedState>,
    headers: HeaderMap,
    body: Bytes,
) -> ProxyResult<Response> {
    let state = app.runtime();
    let body = parse_json_object(&body)?;
    let (_, route) = require_route(&state, &body)?;
    if route.backend == Backend::Claude {
        let target = Target::ClaudeCountTokens {
            model: route.upstream_model,
            beta: beta(&headers),
        };
        return backends::call(&state, target, body)
            .await?
            .relay(Protocol::Anthropic)
            .await;
    }

    let request = anthropic_to_gemini::count_tokens_request(&body, &route.upstream_model)?;
    let target = Target::GeminiNative {
        model: route.upstream_model,
        action: "countTokens".to_owned(),
        query: String::new(),
        tier_override: None,
    };
    backends::call(&state, target, request)
        .await?
        .translate_json(anthropic_to_gemini::count_tokens_response)
        .await
}
