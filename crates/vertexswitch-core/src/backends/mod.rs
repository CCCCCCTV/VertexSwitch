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
use futures_util::future::BoxFuture;
use futures_util::{Stream, StreamExt};
use serde_json::Value;

use crate::config::Settings;
use crate::credentials::Trouble as CredentialTrouble;
use crate::errors::{Protocol, ProxyError, ProxyResult};
use crate::metrics::{self, RegionProbe};
use crate::regions::Trouble as RegionTrouble;
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

/// 发请求：选凭证与区域、拿 token、拼地址；出问题时换区域 / 换凭证重发（SPEC §9a、§9b）；
/// 上游最终返回非 2xx 时转成错误。
pub async fn call(state: &Runtime, target: Target, mut body: Value) -> ProxyResult<Upstream> {
    let (model, backend, stream) = target.summary();
    let model = model.to_owned();
    // 先记下，取 token 失败的请求在记录里也有模型
    metrics::note(|i| {
        i.model = Some(model.clone());
        i.backend = Some(backend.as_str());
        i.stream = stream;
    });
    let settings = &state.settings;
    let kind = match &target {
        Target::ClaudeMessages { beta, .. } => {
            claude::messages_body(&mut body, &settings.claude);
            RouteKind::ClaudeMessages {
                beta: beta.as_deref(),
            }
        }
        Target::ClaudeCountTokens { model, beta } => {
            claude::count_tokens_body(&mut body, &settings.claude, model);
            RouteKind::ClaudeCountTokens {
                beta: beta.as_deref(),
            }
        }
        Target::GeminiNative {
            action,
            query,
            tier_override,
            ..
        } => {
            gemini::prepare_body(&mut body);
            RouteKind::GeminiNative {
                action,
                query,
                tier_override: tier_override.as_deref(),
            }
        }
        Target::VertexOpenAi {
            backend: Backend::Gemini,
            model,
            ..
        } => {
            body["model"] = format!("google/{model}").into();
            RouteKind::GeminiOpenAi
        }
        Target::VertexOpenAi { model, .. } => {
            body["model"] = model.clone().into();
            RouteKind::Maas
        }
    };
    let route = Route {
        settings,
        model: &model,
        kind,
        body: json_bytes(&body),
        stream,
    };
    let (response, plan) = open(state, &route).await?;
    let tier = match (&plan, &route.kind) {
        (Some(plan), _) => Some(plan.effective),
        (None, RouteKind::GeminiOpenAi) => Some(Tier::Standard),
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

fn add_beta(headers: &mut Vec<(String, String)>, beta: Option<&str>) {
    if let Some(beta) = beta {
        headers.push(("anthropic-beta".to_owned(), beta.to_owned()));
    }
}

/// 一个请求要发往的上游：给定凭证和区域就能拼出请求。换区域、换凭证重发时用它重新拼。
struct Route<'a> {
    settings: &'a Settings,
    model: &'a str,
    kind: RouteKind<'a>,
    body: Bytes,
    stream: bool,
}

enum RouteKind<'a> {
    /// Vertex 上的 Claude：Messages
    ClaudeMessages { beta: Option<&'a str> },
    /// Vertex 上的 Claude：count_tokens
    ClaudeCountTokens { beta: Option<&'a str> },
    /// Gemini 原生接口（唯一能带档位头的通道）
    GeminiNative {
        action: &'a str,
        query: &'a str,
        tier_override: Option<&'a str>,
    },
    /// Gemini 的 OpenAI 兼容接口，一律按 Standard
    GeminiOpenAi,
    /// MaaS 开放模型（OpenAI 兼容接口）
    Maas,
}

/// 这次用哪个凭证发：序号（从 0 数）、token、项目。
struct Credential {
    index: usize,
    token: String,
    project: String,
}

impl Route<'_> {
    fn is_gemini(&self) -> bool {
        matches!(
            self.kind,
            RouteKind::GeminiNative { .. } | RouteKind::GeminiOpenAi
        )
    }

    /// 平时发往的区域：Claude 用默认区域，Gemini 用 Gemini 区域，MaaS 用 MaaS 区域。
    fn home_region(&self) -> &str {
        match self.kind {
            RouteKind::ClaudeMessages { .. } | RouteKind::ClaudeCountTokens { .. } => {
                &self.settings.google.region
            }
            RouteKind::Maas => self.settings.maas_region(),
            RouteKind::GeminiNative { .. } | RouteKind::GeminiOpenAi => {
                self.settings.gemini_region()
            }
        }
    }

    /// 只有 Gemini 参与区域切换。
    fn switches_regions(&self) -> bool {
        self.is_gemini() && self.settings.gemini.region_switch.enabled
    }

    /// 用凭证 `cred` 发往 `region` 的请求与档位方案；顺便在请求记录里记下区域、档位和凭证。
    fn build(&self, cred: &Credential, region: &str) -> (UpstreamRequest, Option<TierPlan>) {
        let api_base = self.settings.google.api_base_override.as_deref();
        let mut headers = vec![
            ("authorization".to_owned(), format!("Bearer {}", cred.token)),
            ("content-type".to_owned(), "application/json".to_owned()),
        ];
        let model = self.model;
        let publisher = |publisher: &str| {
            vertex::publisher_models_url(
                &vertex::base_url(region, api_base),
                &cred.project,
                region,
                publisher,
            )
        };
        let mut plan = None;
        let url = match &self.kind {
            RouteKind::ClaudeMessages { beta } => {
                add_beta(&mut headers, *beta);
                claude::messages_url(&publisher("anthropic"), model, self.stream)
            }
            RouteKind::ClaudeCountTokens { beta } => {
                add_beta(&mut headers, *beta);
                claude::count_tokens_url(&publisher("anthropic"))
            }
            RouteKind::GeminiNative {
                action,
                query,
                tier_override,
            } => {
                plan = gemini::tier_plan_in(self.settings, model, action, *tier_override, region);
                if let Some(plan) = &plan {
                    for warning in &plan.warnings {
                        tracing::warn!(model = %model, kind = LOG_KIND_FALLBACK, "{warning}");
                    }
                    headers.extend(plan.headers.iter().map(|(k, v)| (k.to_string(), v.clone())));
                }
                let base = publisher("google");
                if query.is_empty() {
                    format!("{base}/{model}:{action}")
                } else {
                    format!("{base}/{model}:{action}?{query}")
                }
            }
            RouteKind::GeminiOpenAi => vertex::openapi_chat_url(
                &vertex::host(region, api_base),
                &self.settings.gemini.openai_api_version,
                &cred.project,
                region,
            ),
            RouteKind::Maas => vertex::openapi_chat_url(
                &vertex::host(region, api_base),
                &self.settings.maas.api_version,
                &cred.project,
                region,
            ),
        };
        let tier = match (&plan, &self.kind) {
            (Some(plan), _) => Some(plan.effective),
            (None, RouteKind::GeminiOpenAi) => Some(Tier::Standard),
            _ => None,
        };
        metrics::note(|i| {
            i.region = Some(region.to_owned());
            i.tier = tier.map(Tier::as_str);
            i.credential = Some(cred.index + 1);
        });
        let mut request = UpstreamRequest::post_json(url, headers, self.body.clone(), self.stream);
        request.min_read_timeout = plan
            .as_ref()
            .and_then(|p| p.server_timeout)
            .map(|s| Duration::from_secs(s) + gemini::TIMEOUT_MARGIN);
        (request, plan)
    }
}

/// 发请求：从这个模型当前的凭证和区域开始，出问题时由 [`Failover`] 换区域 / 换凭证重发；
/// Gemini 流式请求在开了区域切换时，成功后把表现交给区域切换。返回响应与最后用的档位方案。
async fn open(
    state: &Runtime,
    route: &Route<'_>,
) -> ProxyResult<(reqwest::Response, Option<TierPlan>)> {
    let settings = route.settings;
    let model = route.model;
    let first = state.credentials.current(
        model,
        state.credential_count(),
        settings.google.credential_reset_minutes,
    );
    let mut tried_creds = Vec::new();
    let cred = acquire(state, model, first, &mut tried_creds).await?;
    let home = route.home_region();
    let region_switch = &settings.gemini.region_switch;
    let region = if route.switches_regions() {
        state.regions.region_for(region_switch, home, model)
    } else {
        home.to_owned()
    };
    let (request, plan) = route.build(&cred, &region);
    let mut failover = Failover {
        route,
        state,
        cred,
        tried_regions: vec![region.clone()],
        region,
        plan,
        tried_creds,
    };
    let response = state
        .transport
        .open_with(request, Some(&mut failover))
        .await?;
    if route.stream && route.switches_regions() {
        metrics::note(|i| {
            i.region_probe = Some(RegionProbe {
                switch: state.regions.clone(),
                settings: region_switch.clone(),
                home: home.to_owned(),
                model: model.to_owned(),
            });
        });
    }
    Ok((response, failover.plan))
}

/// 从凭证 `start` 开始拿到一个能用的凭证（token 与项目）；拿不到就换下一个（取 token 失败的
/// 在重置前跳过）。只有一个凭证、或都拿不到时返回最后的错误。
async fn acquire(
    state: &Runtime,
    model: &str,
    start: usize,
    tried: &mut Vec<usize>,
) -> ProxyResult<Credential> {
    let mut index = start;
    loop {
        tried.push(index);
        let result = match state.credential(index).get_token().await {
            Ok(token) => state.credential_project(index).map(|project| Credential {
                index,
                token,
                project,
            }),
            Err(e) => Err(ProxyError::new(500, e.0)),
        };
        let error = match result {
            Ok(cred) => return Ok(cred),
            Err(e) => e,
        };
        match state.credentials.on_trouble(
            model,
            index,
            CredentialTrouble::TokenFailed,
            tried,
            state.credential_count(),
            state.settings.google.credential_reset_minutes,
            |i| state.credential_label(i),
        ) {
            Some(next) => index = next,
            None => return Err(error),
        }
    }
}

/// 一个请求的换地方重发（SPEC §9a、§9b）：
/// - 429 / 503：先换区域（开了区域切换时），这个凭证所有区域都繁忙再换凭证；区域 × 凭证一圈都繁忙
///   就交还给繁忙重试，等过之后从当前位置再来一圈。
/// - 401 / 403：换凭证。
/// - 404：不在平时区域时先换区域；在平时区域（或没有可换的区域）时换凭证（这个项目用不了这个模型）。
struct Failover<'a> {
    route: &'a Route<'a>,
    state: &'a Runtime,
    cred: Credential,
    /// 这个请求当前发往的区域
    region: String,
    plan: Option<TierPlan>,
    /// 当前凭证这一轮试过的区域
    tried_regions: Vec<String>,
    /// 这一轮试过的凭证
    tried_creds: Vec<usize>,
}

impl Failover<'_> {
    /// 同一个凭证换下一个区域（只有 Gemini 且开了区域切换时）。
    fn next_region(&mut self, trouble: RegionTrouble, status: u16) -> Option<UpstreamRequest> {
        if !self.route.switches_regions() {
            return None;
        }
        let model = self.route.model;
        let next = self.state.regions.on_trouble(
            &self.route.settings.gemini.region_switch,
            self.route.home_region(),
            model,
            &self.region,
            trouble,
            &self.tried_regions,
        )?;
        let from = &self.region;
        tracing::info!(
            model = %model,
            "{}",
            tr!(
                "{model} 在 {from} 返回 {status}，这个请求改发 {next}",
                "{model} returned {status} in {from}; resending this request to {next}"
            )
        );
        self.tried_regions.push(next.clone());
        let (request, plan) = self.route.build(&self.cred, &next);
        self.region = next;
        self.plan = plan;
        Some(request)
    }

    /// 换下一个凭证；取不到 token 的跳过。新凭证从这个模型当前的区域开始，区域重新试一轮。
    async fn next_credential(
        &mut self,
        trouble: CredentialTrouble,
        status: u16,
    ) -> Option<UpstreamRequest> {
        let state = self.state;
        let count = state.credential_count();
        if count <= 1 {
            return None;
        }
        let model = self.route.model;
        let reset = self.route.settings.google.credential_reset_minutes;
        let (mut failed, mut trouble) = (self.cred.index, trouble);
        let cred = loop {
            let next = state.credentials.on_trouble(
                model,
                failed,
                trouble,
                &self.tried_creds,
                count,
                reset,
                |i| state.credential_label(i),
            )?;
            self.tried_creds.push(next);
            let token = state.credential(next).get_token().await;
            match token.map(|token| (token, state.credential_project(next))) {
                Ok((token, Ok(project))) => {
                    break Credential {
                        index: next,
                        token,
                        project,
                    };
                }
                _ => (failed, trouble) = (next, CredentialTrouble::TokenFailed),
            }
        };
        let (from, to) = (
            state.credential_label(self.cred.index),
            state.credential_label(cred.index),
        );
        tracing::info!(
            model = %model,
            "{}",
            tr!(
                "{model} 用{from}返回 {status}，这个请求改用{to}",
                "{model} returned {status} with {from}; resending this request with {to}"
            )
        );
        let home = self.route.home_region();
        self.region = if self.route.switches_regions() {
            state
                .regions
                .region_for(&self.route.settings.gemini.region_switch, home, model)
        } else {
            home.to_owned()
        };
        self.tried_regions = vec![self.region.clone()];
        self.cred = cred;
        let (request, plan) = self.route.build(&self.cred, &self.region);
        self.plan = plan;
        Some(request)
    }
}

impl Reroute for Failover<'_> {
    fn reroute(&mut self, status: u16) -> BoxFuture<'_, Option<UpstreamRequest>> {
        Box::pin(async move {
            match status {
                429 | 503 => {
                    if let Some(request) = self.next_region(RegionTrouble::Busy(status), status) {
                        return Some(request);
                    }
                    if let Some(request) = self
                        .next_credential(CredentialTrouble::Busy(status), status)
                        .await
                    {
                        return Some(request);
                    }
                    // 区域 × 凭证一圈都繁忙：交还给繁忙重试，等过之后从当前位置再来一圈
                    self.tried_regions = vec![self.region.clone()];
                    self.tried_creds = vec![self.cred.index];
                    None
                }
                401 | 403 => {
                    self.next_credential(CredentialTrouble::Denied(status), status)
                        .await
                }
                404 => {
                    if self.region != self.route.home_region()
                        && let Some(request) = self.next_region(RegionTrouble::Missing, status)
                    {
                        return Some(request);
                    }
                    self.next_credential(CredentialTrouble::Missing, status)
                        .await
                }
                _ => None,
            }
        })
    }
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
