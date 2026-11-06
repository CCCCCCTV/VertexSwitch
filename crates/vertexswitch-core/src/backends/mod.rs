//! 后端模块：把一个已路由的请求发给 Vertex AI，再把上游的回复交还给客户端（SPEC §3）。
//!
//! 入口协议模块只管解析客户端请求、选路由、必要时翻译请求体，然后调用 [`call`]。这里负责：
//! 拿 token 与项目、按后端选区域、拼 URL 与请求头、改写请求体、Gemini 的服务档位（档位头、
//! Flex 排队超时、核对实际档位、响应头 `X-VertexSwitch-Tier`）、在请求记录里写上模型 / 后端 /
//! 档位 / 是否流式、打开请求并把非 2xx 转成错误。拿到的 [`Upstream`] 再选一种方式交还：
//! 原样转交（[`Upstream::relay`]），读成 JSON 交给翻译（[`Upstream::translate_json`]），
//! 或逐个 SSE 事件交给翻译器（[`Upstream::translate_stream`]）。协议之间的翻译规则在 `translate`。

pub mod claude;
pub mod gemini;
pub mod vertex;

use std::convert::Infallible;
use std::time::Duration;

use axum::{
    Json,
    body::Body,
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde_json::Value;

use crate::errors::{Protocol, ProxyError, ProxyResult};
use crate::metrics;
use crate::registry::Backend;
use crate::runtime::Runtime;
use crate::tiers::{LOG_KIND_FALLBACK, Tier};
use crate::tr;
use crate::translate::StreamTranslate;
use crate::translate::sse::SseParser;
use crate::transport::{Transport, UpstreamRequest};

/// 响应头：本次实际使用的 Gemini 服务档位。
pub const TIER_HEADER: &str = "x-vertexswitch-tier";

/// 要调用的上游。模型都是上游 ID（路由解析之后的名字）。
#[derive(Debug, Clone)]
pub enum Target {
    /// Vertex 上的 Claude：Messages。`beta` 为客户端的 `anthropic-beta` 请求头，原样转发。
    ClaudeMessages {
        model: String,
        stream: bool,
        beta: Option<String>,
    },
    /// Vertex 上的 Claude：count_tokens。
    ClaudeCountTokens { model: String, beta: Option<String> },
    /// Gemini 原生接口。`action` 为 generateContent / streamGenerateContent / countTokens，
    /// `query` 原样附在 URL 后（如 `alt=sse`），`tier_override` 是客户端用请求头指定的档位。
    GeminiNative {
        model: String,
        action: String,
        query: String,
        tier_override: Option<String>,
    },
    /// Vertex 的 OpenAI 兼容接口：Gemini（发给上游时写成 `google/…`）或 MaaS 开放模型。
    /// 这个接口带不了档位头，Gemini 一律按 Standard 处理。
    VertexOpenAi {
        backend: Backend,
        model: String,
        stream: bool,
    },
}

impl Target {
    /// (上游模型, 后端, 是否流式)
    fn summary(&self) -> (&str, Backend, bool) {
        match self {
            Target::ClaudeMessages { model, stream, .. } => (model, Backend::Claude, *stream),
            Target::ClaudeCountTokens { model, .. } => (model, Backend::Claude, false),
            Target::GeminiNative { model, action, .. } => {
                (model, Backend::Gemini, action == "streamGenerateContent")
            }
            Target::VertexOpenAi {
                backend,
                model,
                stream,
            } => (model, *backend, *stream),
        }
    }
}

/// 发请求：先拿 token（项目 ID 可能要从密钥文件里读出来），再拼地址；上游返回非 2xx 时转成错误。
pub async fn call(state: &Runtime, target: Target, mut body: Value) -> ProxyResult<Upstream> {
    let (model, backend, stream) = target.summary();
    let model = model.to_owned();
    // 先记下，取 token 失败的请求在记录里也有模型
    metrics::note(|i| {
        i.model = Some(model.clone());
        i.backend = Some(backend.as_str());
        i.stream = stream;
    });
    let mut headers = vec![
        (
            "authorization".to_owned(),
            format!("Bearer {}", state.google_token().await?),
        ),
        ("content-type".to_owned(), "application/json".to_owned()),
    ];
    let settings = &state.settings;
    let mut plan = None;
    let url = match &target {
        Target::ClaudeMessages {
            model,
            stream,
            beta,
        } => {
            add_beta(&mut headers, beta);
            claude::messages_body(&mut body, &settings.claude);
            claude::messages_url(&claude_models(state)?, model, *stream)
        }
        Target::ClaudeCountTokens { model, beta } => {
            add_beta(&mut headers, beta);
            claude::count_tokens_body(&mut body, &settings.claude, model);
            claude::count_tokens_url(&claude_models(state)?)
        }
        Target::GeminiNative {
            model,
            action,
            query,
            tier_override,
        } => {
            gemini::prepare_body(&mut body);
            plan = gemini::tier_plan(settings, model, action, tier_override.as_deref());
            if let Some(plan) = &plan {
                for warning in &plan.warnings {
                    tracing::warn!(model = %model, kind = LOG_KIND_FALLBACK, "{warning}");
                }
                headers.extend(plan.headers.iter().map(|(k, v)| (k.to_string(), v.clone())));
            }
            let region = settings.gemini_region();
            let base = vertex::publisher_models_url(
                &state.vertex_base(region),
                &state.project_id()?,
                region,
                "google",
            );
            if query.is_empty() {
                format!("{base}/{model}:{action}")
            } else {
                format!("{base}/{model}:{action}?{query}")
            }
        }
        Target::VertexOpenAi { backend, model, .. } => {
            let (region, version, upstream_model) = if *backend == Backend::Gemini {
                (
                    settings.gemini_region(),
                    &settings.gemini.openai_api_version,
                    format!("google/{model}"),
                )
            } else {
                (
                    settings.maas_region(),
                    &settings.maas.api_version,
                    model.clone(),
                )
            };
            body["model"] = upstream_model.into();
            vertex::openapi_chat_url(
                &state.vertex_host(region),
                version,
                &state.project_id()?,
                region,
            )
        }
    };
    let tier = match (&plan, &target) {
        (Some(plan), _) => Some(plan.effective),
        (None, Target::VertexOpenAi { backend, .. }) if *backend == Backend::Gemini => {
            Some(Tier::Standard)
        }
        _ => None,
    };
    metrics::note(|i| i.tier = tier.map(Tier::as_str));

    let mut request = UpstreamRequest::post_json(
        url,
        headers,
        serde_json::to_vec(&body).expect("Value 可序列化").into(),
        stream,
    );
    request.min_read_timeout = plan
        .as_ref()
        .and_then(|p| p.server_timeout)
        .map(|s| Duration::from_secs(s) + gemini::TIMEOUT_MARGIN);
    let response = state.transport.open(&request).await?;
    let status = response.status();
    if !status.is_success() {
        let raw = state.transport.read_body(response).await?;
        return Err(ProxyError::from_upstream(status.as_u16(), &raw));
    }
    Ok(Upstream {
        response,
        transport: state.transport.clone(),
        stream,
        tier,
        traffic: gemini::TrafficCheck::new(plan, model),
    })
}

fn add_beta(headers: &mut Vec<(String, String)>, beta: &Option<String>) {
    if let Some(beta) = beta {
        headers.push(("anthropic-beta".to_owned(), beta.clone()));
    }
}

/// Claude 走默认区域（`google.region`）。
fn claude_models(state: &Runtime) -> ProxyResult<String> {
    let region = &state.settings.google.region;
    Ok(vertex::publisher_models_url(
        &state.vertex_base(region),
        &state.project_id()?,
        region,
        "anthropic",
    ))
}

/// 上游的成功回复（状态码已确认是 2xx，正文还没读）。
pub struct Upstream {
    response: reqwest::Response,
    transport: Transport,
    stream: bool,
    tier: Option<Tier>,
    traffic: gemini::TrafficCheck,
}

impl Upstream {
    /// 原样转交：流式边收边转（顺带核对档位），中途出错按入口协议补发一个错误事件；非流式读完再转交。
    pub async fn relay(self, protocol: Protocol) -> ProxyResult<Response> {
        let Self {
            response,
            transport,
            stream,
            tier,
            mut traffic,
        } = self;
        let status = response.status();
        // Gemini 原生流不带 alt=sse 时是 JSON 数组，所以照搬上游的类型；其余协议的流一定是 SSE
        let content_type = if stream && protocol != Protocol::Gemini {
            HeaderValue::from_static("text/event-stream")
        } else {
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .cloned()
                .unwrap_or_else(|| HeaderValue::from_static("application/json"))
        };
        let mut out = if stream {
            relay_chunks(transport.chunks(response), protocol, move |chunk| {
                traffic.feed(chunk)
            })
        } else {
            let data = transport.read_body(response).await?;
            traffic.feed(&data);
            Response::new(Body::from(data))
        };
        *out.status_mut() = status;
        out.headers_mut().insert(header::CONTENT_TYPE, content_type);
        add_tier(&mut out, tier);
        Ok(out)
    }

    /// 读完正文、解析成 JSON，交给 `translate` 转成客户端协议的响应体。
    pub async fn translate_json(
        self,
        translate: impl FnOnce(&Value) -> Value,
    ) -> ProxyResult<Response> {
        let Self {
            response,
            transport,
            tier,
            mut traffic,
            ..
        } = self;
        let data = transport.read_body(response).await?;
        traffic.feed(&data);
        let message: Value = serde_json::from_slice(&data).map_err(|_| {
            ProxyError::new(
                502,
                tr!(
                    "Vertex AI 返回了无法解析的响应",
                    "Vertex AI returned a response that couldn't be parsed"
                ),
            )
        })?;
        let mut out = Json(translate(&message)).into_response();
        add_tier(&mut out, tier);
        Ok(out)
    }

    /// 把上游的 SSE 逐个事件交给翻译器。上游的错误事件、中途断开、没有正常结束，都按入口协议
    /// 补发一个错误事件后结束（请求记录据此记为 stream_error）。
    pub fn translate_stream(
        self,
        protocol: Protocol,
        mut translator: impl StreamTranslate,
    ) -> Response {
        let Self {
            response,
            transport,
            tier,
            mut traffic,
            ..
        } = self;
        let chunks = transport.chunks(response);
        let frames = async_stream::stream! {
            futures_util::pin_mut!(chunks);
            let mut parser = SseParser::default();
            while let Some(item) = chunks.next().await {
                let chunk = match item {
                    Ok(chunk) => chunk,
                    Err(e) => {
                        yield Ok::<_, Infallible>(e.stream_event(protocol));
                        return;
                    }
                };
                traffic.feed(&chunk);
                for (event, raw) in parser.feed(&chunk) {
                    let Ok(data) = serde_json::from_str::<Value>(&raw) else {
                        tracing::warn!("{}", tr!("无法解析的上游 SSE 数据：{}", "Couldn't parse upstream SSE data: {}", raw.chars().take(200).collect::<String>()));
                        continue;
                    };
                    if let Some(message) = upstream_error(&event, &data) {
                        yield Ok(ProxyError::new(500, message).stream_event(protocol));
                        return;
                    }
                    for frame in translator.on_event(&event, &data) {
                        yield Ok(frame);
                    }
                }
            }
            if let Some(frames) = translator.finish() {
                for frame in frames {
                    yield Ok(frame);
                }
            } else {
                yield Ok(ProxyError::new(502, tr!("Vertex AI 流式响应提前结束", "Vertex AI's stream ended early")).stream_event(protocol));
            }
        };
        let mut out = Response::new(Body::from_stream(frames));
        out.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        out.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        add_tier(&mut out, tier);
        out
    }
}

fn add_tier(response: &mut Response, tier: Option<Tier>) {
    if let Some(tier) = tier {
        response
            .headers_mut()
            .insert(TIER_HEADER, HeaderValue::from_static(tier.as_str()));
    }
}

/// 上游在流里报的错：Claude 发 `event: error`（`type: error`），Gemini 发带 `error` 的数据块。
fn upstream_error(event: &str, data: &Value) -> Option<String> {
    let is_error = event == "error"
        || data.get("type").and_then(Value::as_str) == Some("error")
        || data.pointer("/error/message").is_some();
    is_error.then(|| {
        data.pointer("/error/message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| tr!("上游返回错误事件", "Upstream sent an error event"))
    })
}

/// 原样转发上游流；中途出错时按入口协议补发一个错误事件后结束（SPEC §5）。
/// `on_chunk` 用于旁路观察数据（如核对 trafficType）。
fn relay_chunks(
    chunks: impl Stream<Item = ProxyResult<Bytes>> + Send + 'static,
    protocol: Protocol,
    mut on_chunk: impl FnMut(&[u8]) + Send + 'static,
) -> Response {
    let body = async_stream::stream! {
        futures_util::pin_mut!(chunks);
        while let Some(item) = chunks.next().await {
            match item {
                Ok(chunk) => {
                    on_chunk(&chunk);
                    yield Ok::<_, Infallible>(chunk);
                }
                Err(e) => {
                    yield Ok(e.stream_event(protocol));
                    break;
                }
            }
        }
    };
    let mut response = Response::new(Body::from_stream(body));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn upstream_error_events() {
        let claude = json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}});
        assert_eq!(
            upstream_error("error", &claude).as_deref(),
            Some("Overloaded")
        );
        let gemini = json!({"error": {"code": 429, "message": "Resource exhausted"}});
        assert_eq!(
            upstream_error("message", &gemini).as_deref(),
            Some("Resource exhausted")
        );
        assert!(upstream_error("error", &json!({})).is_some());
        assert!(
            upstream_error(
                "content_block_delta",
                &json!({"type": "content_block_delta"})
            )
            .is_none()
        );
        assert!(upstream_error("message", &json!({"candidates": []})).is_none());
    }
}
