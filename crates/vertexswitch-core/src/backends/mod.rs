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

use crate::config::{RegionSwitchSettings, Settings};
use crate::errors::{Protocol, ProxyError, ProxyResult};
use crate::metrics::{self, RegionProbe};
use crate::regions::{RegionSwitch, Trouble};
use crate::registry::Backend;
use crate::runtime::Runtime;
use crate::tiers::{LOG_KIND_FALLBACK, Tier, TierPlan};
use crate::tr;
use crate::translate::StreamTranslate;
use crate::translate::sse::SseParser;
use crate::transport::{Reroute, Transport, UpstreamRequest};

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
    let (response, plan) = match &target {
        Target::ClaudeMessages {
            model,
            stream,
            beta,
        } => {
            add_beta(&mut headers, beta);
            claude::messages_body(&mut body, &settings.claude);
            let url = claude::messages_url(&claude_models(state)?, model, *stream);
            (open_plain(state, url, headers, &body, *stream).await?, None)
        }
        Target::ClaudeCountTokens { model, beta } => {
            add_beta(&mut headers, beta);
            claude::count_tokens_body(&mut body, &settings.claude, model);
            let url = claude::count_tokens_url(&claude_models(state)?);
            (open_plain(state, url, headers, &body, false).await?, None)
        }
        Target::GeminiNative {
            model,
            action,
            query,
            tier_override,
        } => {
            gemini::prepare_body(&mut body);
            let route = GeminiRoute {
                settings,
                project: state.project_id()?,
                model,
                kind: GeminiKind::Native {
                    action,
                    query,
                    tier_override: tier_override.as_deref(),
                },
                headers,
                body: json_bytes(&body),
                stream,
            };
            open_gemini(state, &route).await?
        }
        Target::VertexOpenAi {
            backend: Backend::Gemini,
            model,
            stream,
        } => {
            body["model"] = format!("google/{model}").into();
            let route = GeminiRoute {
                settings,
                project: state.project_id()?,
                model,
                kind: GeminiKind::OpenAi,
                headers,
                body: json_bytes(&body),
                stream: *stream,
            };
            open_gemini(state, &route).await?
        }
        Target::VertexOpenAi { model, stream, .. } => {
            let region = settings.maas_region();
            body["model"] = model.clone().into();
            let url = vertex::openapi_chat_url(
                &state.vertex_host(region),
                &settings.maas.api_version,
                &state.project_id()?,
                region,
            );
            (open_plain(state, url, headers, &body, *stream).await?, None)
        }
    };
    let tier = match (&plan, &target) {
        (Some(plan), _) => Some(plan.effective),
        (None, Target::VertexOpenAi { backend, .. }) if *backend == Backend::Gemini => {
            Some(Tier::Standard)
        }
        _ => None,
    };
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

fn json_bytes(body: &Value) -> Bytes {
    serde_json::to_vec(body).expect("Value 可序列化").into()
}

/// Claude 与 MaaS：地址固定，照常发（繁忙时原地等待重试）。
async fn open_plain(
    state: &Runtime,
    url: String,
    headers: Vec<(String, String)>,
    body: &Value,
    stream: bool,
) -> ProxyResult<reqwest::Response> {
    let request = UpstreamRequest::post_json(url, headers, json_bytes(body), stream);
    state.transport.open_with(request, None).await
}

/// 发往 Gemini 的一个请求：按区域拼地址和档位头。开了区域切换时同一个请求可能换区域重发。
struct GeminiRoute<'a> {
    settings: &'a Settings,
    project: String,
    model: &'a str,
    kind: GeminiKind<'a>,
    headers: Vec<(String, String)>,
    body: Bytes,
    stream: bool,
}

enum GeminiKind<'a> {
    /// 原生接口（唯一能带档位头的通道）
    Native {
        action: &'a str,
        query: &'a str,
        tier_override: Option<&'a str>,
    },
    /// OpenAI 兼容接口，一律按 Standard
    OpenAi,
}

impl GeminiRoute<'_> {
    /// 发往 `region` 的请求与档位方案；顺便在请求记录里记下区域和档位。
    fn build(&self, region: &str) -> (UpstreamRequest, Option<TierPlan>) {
        let api_base = self.settings.google.api_base_override.as_deref();
        let mut headers = self.headers.clone();
        let model = self.model;
        let (url, plan) = match &self.kind {
            GeminiKind::Native {
                action,
                query,
                tier_override,
            } => {
                let plan =
                    gemini::tier_plan_in(self.settings, model, action, *tier_override, region);
                if let Some(plan) = &plan {
                    for warning in &plan.warnings {
                        tracing::warn!(model = %model, kind = LOG_KIND_FALLBACK, "{warning}");
                    }
                    headers.extend(plan.headers.iter().map(|(k, v)| (k.to_string(), v.clone())));
                }
                let base = vertex::publisher_models_url(
                    &vertex::base_url(region, api_base),
                    &self.project,
                    region,
                    "google",
                );
                let url = if query.is_empty() {
                    format!("{base}/{model}:{action}")
                } else {
                    format!("{base}/{model}:{action}?{query}")
                };
                (url, plan)
            }
            GeminiKind::OpenAi => (
                vertex::openapi_chat_url(
                    &vertex::host(region, api_base),
                    &self.settings.gemini.openai_api_version,
                    &self.project,
                    region,
                ),
                None,
            ),
        };
        let tier = match (&plan, &self.kind) {
            (Some(plan), _) => Some(plan.effective),
            (None, GeminiKind::OpenAi) => Some(Tier::Standard),
            (None, GeminiKind::Native { .. }) => None,
        };
        metrics::note(|i| {
            i.region = Some(region.to_owned());
            i.tier = tier.map(Tier::as_str);
        });
        let mut request = UpstreamRequest::post_json(url, headers, self.body.clone(), self.stream);
        request.min_read_timeout = plan
            .as_ref()
            .and_then(|p| p.server_timeout)
            .map(|s| Duration::from_secs(s) + gemini::TIMEOUT_MARGIN);
        (request, plan)
    }
}

/// 发 Gemini 请求：没开区域切换时发往平时区域；开了时发往这个模型的当前区域，出问题时换区域重发，
/// 流式请求成功后把表现交给区域切换（SPEC §9a）。返回响应与最后用的档位方案。
async fn open_gemini(
    state: &Runtime,
    route: &GeminiRoute<'_>,
) -> ProxyResult<(reqwest::Response, Option<TierPlan>)> {
    let home = route.settings.gemini_region();
    let cfg = &route.settings.gemini.region_switch;
    if !cfg.enabled {
        let (request, plan) = route.build(home);
        return Ok((state.transport.open_with(request, None).await?, plan));
    }
    let region = state.regions.region_for(cfg, home, route.model);
    let (request, plan) = route.build(&region);
    let mut hop = RegionHop {
        route,
        switch: &state.regions,
        cfg,
        home,
        tried: vec![region.clone()],
        region,
        plan,
    };
    let response = state.transport.open_with(request, Some(&mut hop)).await?;
    if route.stream {
        metrics::note(|i| {
            i.region_probe = Some(RegionProbe {
                switch: state.regions.clone(),
                settings: cfg.clone(),
                home: home.to_owned(),
                model: route.model.to_owned(),
            });
        });
    }
    Ok((response, hop.plan))
}

/// 一个请求的换区域重发：429 / 503 / 404 时问区域切换下一站；这一轮都试过了就交还给繁忙重试。
struct RegionHop<'a> {
    route: &'a GeminiRoute<'a>,
    switch: &'a RegionSwitch,
    cfg: &'a RegionSwitchSettings,
    home: &'a str,
    /// 这个请求当前发往的区域
    region: String,
    plan: Option<TierPlan>,
    /// 这一轮试过的区域
    tried: Vec<String>,
}

impl Reroute for RegionHop<'_> {
    fn reroute(&mut self, status: u16) -> Option<UpstreamRequest> {
        let trouble = match status {
            429 | 503 => Trouble::Busy(status),
            404 => Trouble::Missing,
            _ => return None,
        };
        let model = self.route.model;
        let Some(next) = self.switch.on_trouble(
            self.cfg,
            self.home,
            model,
            &self.region,
            trouble,
            &self.tried,
        ) else {
            // 这一轮都试过了：交还给繁忙重试，等过之后从当前区域重新来一轮
            self.tried = vec![self.region.clone()];
            return None;
        };
        let from = &self.region;
        tracing::info!(
            model = %model,
            "{}",
            tr!(
                "{model} 在 {from} 返回 {status}，这个请求改发 {next}",
                "{model} returned {status} in {from}; resending this request to {next}"
            )
        );
        self.tried.push(next.clone());
        let (request, plan) = self.route.build(&next);
        self.region = next;
        self.plan = plan;
        Some(request)
    }
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
