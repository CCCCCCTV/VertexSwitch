//! OpenAI Chat Completions 入口（SPEC §8.3）：`/v1/chat/completions` 及若干前缀别名。
//!
//! 按 `model` 分流：
//! - Gemini → 需要服务档位头时转成原生 generateContent（兼容接口带档位头会被 Vertex 拒绝，400），
//!   否则原样转发到 Vertex 的 OpenAI 兼容接口；`gemini.openai_native` 可改成总是 / 从不转换
//! - MaaS 开放模型 → 同一兼容接口，`publisher/model`
//! - Claude → 转成 Anthropic Messages 调 Vertex，再把响应（含流式）转回 OpenAI 格式

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
use crate::translate::{openai_to_anthropic, openai_to_gemini};

const PATHS: &[&str] = &[
    "/v1/chat/completions",
    "/chat/completions",
    "/openai/v1/chat/completions",
    "/api/v1/chat/completions",
];

pub fn routes() -> Router<SharedState> {
    PATHS.iter().fold(Router::new(), |router, path| {
        router.route(path, post(chat_completions))
    })
}

async fn chat_completions(
    State(app): State<SharedState>,
    headers: HeaderMap,
    body: Bytes,
) -> ProxyResult<Response> {
    let state = app.runtime();
    let body = parse_json_object(&body)?;
    let model = require_model(&body)?;
    let route = state.registry().resolve(&model).ok_or_else(|| {
        ProxyError::new(404, tr!("未知模型 {model:?}", "Unknown model {model:?}"))
    })?;
    match route.backend {
        Backend::Claude => via_claude(&state, &body, route, &model).await,
        Backend::Gemini => {
            let override_tier = headers
                .get(gemini::TIER_OVERRIDE_HEADER)
                .and_then(|v| v.to_str().ok());
            if gemini::openai_wants_native(&state.settings, &route.upstream_model, override_tier) {
                via_gemini_native(&state, &body, route, &model, override_tier).await
            } else {
                passthrough(&state, body, route).await
            }
        }
        Backend::Maas => passthrough(&state, body, route).await,
    }
}

/// 原样转发到 Vertex 的 OpenAI 兼容接口（Gemini / MaaS）。
async fn passthrough(state: &Runtime, body: Value, route: Route) -> ProxyResult<Response> {
    let target = Target::VertexOpenAi {
        backend: route.backend,
        model: route.upstream_model,
        stream: wants_stream(&body),
    };
    backends::call(state, target, body)
        .await?
        .relay(Protocol::OpenAi)
        .await
}

/// 客户端要不要流式最后一块里的用量（`stream_options.include_usage`）。
fn include_usage(body: &Value) -> bool {
    body.pointer("/stream_options/include_usage")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// OpenAI 请求转成 Gemini 原生 generateContent（带档位头），响应（含流式）转回 OpenAI 格式。
async fn via_gemini_native(
    state: &Runtime,
    body: &Value,
    route: Route,
    client_model: &str,
    override_tier: Option<&str>,
) -> ProxyResult<Response> {
    let request = openai_to_gemini::request_to_gemini(body, &route.upstream_model)?;
    let stream = wants_stream(body);
    let target = Target::GeminiNative {
        model: route.upstream_model,
        action: if stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        }
        .to_owned(),
        query: if stream { "alt=sse" } else { "" }.to_owned(),
        tier_override: override_tier.map(str::to_owned),
    };
    let upstream = backends::call(state, target, request).await?;
    if stream {
        let translator = openai_to_gemini::StreamTranslator::new(client_model, include_usage(body));
        Ok(upstream.translate_stream(Protocol::OpenAi, translator))
    } else {
        upstream
            .translate_json(|response| openai_to_gemini::response_to_openai(response, client_model))
            .await
    }
}

/// OpenAI 请求转成 Anthropic Messages 调 Vertex 上的 Claude，响应（含流式）转回 OpenAI 格式。
async fn via_claude(
    state: &Runtime,
    body: &Value,
    route: Route,
    client_model: &str,
) -> ProxyResult<Response> {
    let request =
        openai_to_anthropic::request_to_anthropic(body, state.settings.openai.default_max_tokens)?;
    let stream = wants_stream(&request);
    let target = Target::ClaudeMessages {
        model: route.upstream_model,
        stream,
        beta: None,
    };
    let upstream = backends::call(state, target, request).await?;
    if stream {
        let translator =
            openai_to_anthropic::StreamTranslator::new(client_model, include_usage(body));
        Ok(upstream.translate_stream(Protocol::OpenAi, translator))
    } else {
        upstream
            .translate_json(|message| {
                openai_to_anthropic::response_to_openai(message, client_model)
            })
            .await
    }
}
