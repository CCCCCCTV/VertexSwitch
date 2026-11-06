//! OpenAI Chat Completions 入口（SPEC §8.3）：`/v1/chat/completions` 及若干前缀别名。
//!
//! 按 `model` 分流：
//! - Gemini → 需要服务档位头时转成原生 generateContent（兼容接口带档位头会被 Vertex 拒绝，400），
//!   否则原样转发到 Vertex 的 OpenAI 兼容接口；`gemini.openai_native` 可改成总是 / 从不转换
//! - MaaS 开放模型 → 同一接口，`publisher/model`
//! - Claude → 转成 Anthropic Messages 调 Vertex，再把响应（含流式）转回 OpenAI 格式

use std::time::Duration;

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
    routing::post,
};
use futures_util::StreamExt;
use serde_json::Value;

use super::gemini::{TIER_OVERRIDE_HEADER, TIMEOUT_MARGIN, TrafficCheck};
use super::{parse_json_object, relay};
use crate::app::SharedState;
use crate::backends::{claude, gemini, vertex};
use crate::errors::{Protocol, ProxyError, ProxyResult};
use crate::metrics;
use crate::registry::{Backend, Route};
use crate::runtime::Runtime;
use crate::tiers::{LOG_KIND_FALLBACK, Tier, TierPlan};
use crate::tr;
use crate::translate::openai_to_anthropic::{
    SseParser, StreamTranslator, request_to_anthropic, response_to_openai,
};
use crate::translate::openai_to_gemini;
use crate::transport::{UpstreamRequest, VERTEX};

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
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .ok_or_else(|| ProxyError::new(400, tr!("model: 缺少模型名", "model: missing model name")))?
        .to_owned();
    let route = state.registry().resolve(&model).ok_or_else(|| {
        ProxyError::new(404, tr!("未知模型 {model:?}", "Unknown model {model:?}"))
    })?;
    let stream = body.get("stream").and_then(Value::as_bool) == Some(true);
    metrics::note(|i| {
        i.model = Some(route.upstream_model.clone());
        i.backend = Some(route.backend.as_str());
        i.stream = stream;
    });
    match route.backend {
        Backend::Claude => via_claude(&state, &body, &route, &model).await,
        Backend::Gemini => {
            let override_tier = headers
                .get(TIER_OVERRIDE_HEADER)
                .and_then(|v| v.to_str().ok());
            to_gemini(&state, body, &route, &model, override_tier).await
        }
        _ => passthrough(&state, body, &route).await,
    }
}

/// Gemini：要发档位头就转成原生格式，否则走兼容接口（见模块说明）。
async fn to_gemini(
    state: &Runtime,
    body: Value,
    route: &Route,
    client_model: &str,
    override_tier: Option<&str>,
) -> ProxyResult<Response> {
    let settings = &state.settings;
    let plan = gemini::tier_plan(
        settings,
        &route.upstream_model,
        "generateContent",
        override_tier,
    )
    .expect("生成请求总有档位方案");
    for warning in &plan.warnings {
        tracing::warn!(model = %route.upstream_model, kind = LOG_KIND_FALLBACK, "{warning}");
    }
    let native = match settings.gemini.openai_native.as_str() {
        "always" => true,
        "never" => false,
        _ => !plan.headers.is_empty(),
    };
    if native {
        return via_gemini_native(state, &body, route, client_model, plan).await;
    }
    if plan.sent_tier_header() {
        tracing::warn!(
            model = %route.upstream_model,
            kind = LOG_KIND_FALLBACK,
            "{}",
            tr!(
                "gemini.openai_native = never，OpenAI 兼容接口带不了档位头，本次按 Standard 发送",
                "gemini.openai_native = never: the OpenAI-compatible endpoint can't carry tier headers, so this request goes out as Standard"
            )
        );
    }
    metrics::note(|i| i.tier = Some(Tier::Standard.as_str()));
    passthrough(state, body, route).await
}

fn sse_data(payload: &str) -> Bytes {
    format!("data: {payload}\n\n").into()
}

async fn passthrough(state: &Runtime, mut body: Value, route: &Route) -> ProxyResult<Response> {
    let settings = &state.settings;
    let headers = vec![
        ("content-type".to_owned(), "application/json".to_owned()),
        (
            "authorization".to_owned(),
            format!("Bearer {}", state.google_token().await?),
        ),
    ];
    let (region, version, upstream_model) = if route.backend == Backend::Gemini {
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
    body["model"] = upstream_model.into();
    let stream = body.get("stream").and_then(Value::as_bool) == Some(true);
    let request = UpstreamRequest::post_json(
        url,
        headers,
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

/// OpenAI 请求转成 Gemini 原生 generateContent（带档位头），响应（含流式）转回 OpenAI 格式。
async fn via_gemini_native(
    state: &Runtime,
    body: &Value,
    route: &Route,
    client_model: &str,
    plan: TierPlan,
) -> ProxyResult<Response> {
    let model = &route.upstream_model;
    let request_body = openai_to_gemini::request_to_gemini(body, model)?;
    let stream = body.get("stream").and_then(Value::as_bool) == Some(true);
    let mut headers = vec![
        (
            "authorization".to_owned(),
            format!("Bearer {}", state.google_token().await?),
        ),
        ("content-type".to_owned(), "application/json".to_owned()),
    ];
    headers.extend(plan.headers.iter().map(|(k, v)| (k.to_string(), v.clone())));
    let region = state.settings.gemini_region();
    let base = vertex::publisher_models_url(
        &state.vertex_base(region),
        &state.project_id()?,
        region,
        "google",
    );
    let url = if stream {
        format!("{base}/{model}:streamGenerateContent?alt=sse")
    } else {
        format!("{base}/{model}:generateContent")
    };
    metrics::note(|i| i.tier = Some(plan.effective.as_str()));
    let mut request = UpstreamRequest::post_json(
        url,
        headers,
        serde_json::to_vec(&request_body)
            .expect("Value 可序列化")
            .into(),
        stream,
    );
    request.min_read_timeout = plan
        .server_timeout
        .map(|s| Duration::from_secs(s) + TIMEOUT_MARGIN);

    let response = state.transport.open(&request).await?;
    let status = response.status();
    if !status.is_success() {
        let raw = state.transport.read_body(response).await?;
        return Err(ProxyError::from_upstream(status.as_u16(), &raw));
    }
    let tier = HeaderValue::from_static(plan.effective.as_str());
    let mut check = TrafficCheck::new(Some(plan), model.clone());

    if !stream {
        let data = state.transport.read_body(response).await?;
        check.feed(&data);
        let message: Value = serde_json::from_slice(&data).map_err(|_| {
            ProxyError::new(
                502,
                tr!(
                    "Vertex AI 返回了无法解析的响应",
                    "Vertex AI returned a response that couldn't be parsed"
                ),
            )
        })?;
        let mut r =
            Json(openai_to_gemini::response_to_openai(&message, client_model)).into_response();
        r.headers_mut().insert("x-vertexswitch-tier", tier);
        return Ok(r);
    }

    let include_usage = body
        .get("stream_options")
        .and_then(|o| o.get("include_usage"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut translator = openai_to_gemini::StreamTranslator::new(client_model, include_usage);
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
            check.feed(&chunk);
            for (_, raw) in parser.feed(&chunk) {
                let Ok(data) = serde_json::from_str::<Value>(&raw) else {
                    tracing::warn!("{}", tr!("无法解析的上游 SSE 数据：{}", "Couldn't parse upstream SSE data: {}", raw.chars().take(200).collect::<String>()));
                    continue;
                };
                if let Some(message) = data.pointer("/error/message").and_then(Value::as_str) {
                    let err = ProxyError::new(500, message);
                    // 与其他流内错误一样由 stream_event 生成，请求记录才能认出“流中出错”
                    yield Ok(err.stream_event(Protocol::OpenAi));
                    return;
                }
                for out in translator.handle(&data) {
                    yield Ok(sse_data(&out.to_string()));
                }
            }
        }
        if translator.finished {
            if let Some(usage) = translator.finish() {
                yield Ok(sse_data(&usage.to_string()));
            }
            yield Ok(sse_data("[DONE]"));
        } else {
            yield Ok(ProxyError::new(502, tr!("Vertex AI 流式响应提前结束", "Vertex AI's stream ended early")).stream_event(Protocol::OpenAi));
        }
    };
    let mut r = Response::new(Body::from_stream(events));
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    r.headers_mut().insert("x-vertexswitch-tier", tier);
    Ok(r)
}

async fn via_claude(
    state: &Runtime,
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
        let message: Value = serde_json::from_slice(&data).map_err(|_| {
            ProxyError::new(
                502,
                tr!(
                    "Vertex AI 返回了无法解析的响应",
                    "Vertex AI returned a response that couldn't be parsed"
                ),
            )
        })?;
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
                    tracing::warn!("{}", tr!("无法解析的上游 SSE 数据：{}", "Couldn't parse upstream SSE data: {}", raw.chars().take(200).collect::<String>()));
                    continue;
                };
                if event == "error" || data.get("type").and_then(Value::as_str) == Some("error") {
                    let message = data
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| tr!("上游返回错误事件", "Upstream sent an error event"));
                    let err = ProxyError::new(500, message);
                    // 与其他流内错误一样由 stream_event 生成，请求记录才能认出“流中出错”
                    yield Ok(err.stream_event(Protocol::OpenAi));
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
            yield Ok(ProxyError::new(502, tr!("Vertex AI 流式响应提前结束", "Vertex AI's stream ended early")).stream_event(Protocol::OpenAi));
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
