//! Gemini 原生入口（SPEC §8.2）与 Gemini 格式模型列表（SPEC §8.4）。
//!
//! `POST /v1beta/models/{model}:{action}`，action 为 generateContent / streamGenerateContent /
//! countTokens。查询参数（如 `alt=sse`）透传，代理自己的 `key` 除外。生成类请求按配置附加
//! 服务档位头，并用响应里的 `usageMetadata.trafficType` 核对实际档位。

use std::time::Duration;

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{Path, RawQuery, State},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::{Value, json};

use super::{parse_json_object, relay};
use crate::app::SharedState;
use crate::backends::{gemini, vertex};
use crate::errors::{Protocol, ProxyError, ProxyResult};
use crate::metrics;
use crate::registry::Backend;
use crate::tiers::{TierPlan, TrafficTypeWatcher, check_traffic_type};
use crate::transport::{UpstreamRequest, VERTEX};

pub const TIER_OVERRIDE_HEADER: &str = "x-vertexswitch-tier";
/// Flex 排队到 X-Server-Timeout 才返回时，本地多等这么久再判超时
pub(super) const TIMEOUT_MARGIN: Duration = Duration::from_secs(60);

pub fn routes() -> Router<SharedState> {
    let mut router = Router::new();
    for prefix in ["", "/gemini"] {
        router = router
            .route(&format!("{prefix}/v1beta/models"), get(list_models))
            .route(
                &format!("{prefix}/v1beta/models/{{*target}}"),
                get(get_model).post(model_action),
            );
    }
    // 部分客户端走 v1 路径：POST /v1/models/{model}:{action}。该路径的 GET 是 OpenAI / Anthropic
    // 格式的模型查询，两者在 frontends::models 里注册到同一条路由上。
    router
}

fn split_target(target: &str) -> ProxyResult<(&str, &str)> {
    match target.rsplit_once(':') {
        Some((model, action)) if !model.is_empty() && gemini::ACTIONS.contains(&action) => {
            Ok((model, action))
        }
        _ => Err(ProxyError::new(
            404,
            format!("不支持的 Gemini 方法：{target:?}"),
        )),
    }
}

/// 去掉代理自己的 `key` 参数后的查询串。
fn forwarded_query(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return String::new();
    };
    let mut out = form_urlencoded::Serializer::new(String::new());
    for (k, v) in form_urlencoded::parse(raw.as_bytes()).filter(|(k, _)| k != "key") {
        out.append_pair(&k, &v);
    }
    out.finish()
}

/// 收到第一个 trafficType 时核对一次档位。
pub(super) struct TrafficCheck {
    plan: Option<TierPlan>,
    model: String,
    watcher: TrafficTypeWatcher,
}

impl TrafficCheck {
    pub(super) fn new(plan: Option<TierPlan>, model: String) -> Self {
        Self {
            plan,
            model,
            watcher: TrafficTypeWatcher::default(),
        }
    }

    pub(super) fn feed(&mut self, chunk: &[u8]) {
        let Some(plan) = &self.plan else {
            return;
        };
        if let Some(traffic) = self.watcher.feed(chunk)
            && let Some(warning) = check_traffic_type(plan, &traffic)
        {
            tracing::warn!("{}：{warning}", self.model);
        }
    }
}

pub async fn model_action(
    State(app): State<SharedState>,
    Path(target): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> ProxyResult<Response> {
    let state = app.runtime();
    let (model, action) = split_target(&target)?;
    let route = state
        .registry()
        .resolve(model)
        .filter(|r| r.backend == Backend::Gemini)
        .ok_or_else(|| ProxyError::new(404, format!("未知的 Gemini 模型 {model:?}")))?;
    let mut body = parse_json_object(&body)?;
    gemini::prepare_body(&mut body);

    let mut upstream_headers = vec![
        (
            "authorization".to_owned(),
            format!("Bearer {}", state.google_token().await?),
        ),
        ("content-type".to_owned(), "application/json".to_owned()),
    ];
    let override_tier = headers
        .get(TIER_OVERRIDE_HEADER)
        .and_then(|v| v.to_str().ok());
    let plan = gemini::tier_plan(
        &state.settings,
        &route.upstream_model,
        action,
        override_tier,
    );
    if let Some(plan) = &plan {
        for warning in &plan.warnings {
            tracing::warn!("{}：{warning}", route.upstream_model);
        }
        upstream_headers.extend(plan.headers.iter().map(|(k, v)| (k.to_string(), v.clone())));
    }

    let region = state.settings.gemini_region();
    let base = vertex::publisher_models_url(
        &state.vertex_base(region),
        &state.project_id()?,
        region,
        "google",
    );
    let mut url = format!("{base}/{}:{action}", route.upstream_model);
    let query = forwarded_query(query.as_deref());
    if !query.is_empty() {
        url = format!("{url}?{query}");
    }
    let stream = action == "streamGenerateContent";
    metrics::note(|i| {
        i.model = Some(route.upstream_model.clone());
        i.backend = Some(Backend::Gemini.as_str());
        i.tier = plan.as_ref().map(|p| p.effective.as_str());
        i.stream = stream;
    });
    let mut request = UpstreamRequest::post_json(
        url,
        upstream_headers,
        serde_json::to_vec(&body).expect("Value 可序列化").into(),
        stream,
    );
    request.min_read_timeout = plan
        .as_ref()
        .and_then(|p| p.server_timeout)
        .map(|s| Duration::from_secs(s) + TIMEOUT_MARGIN);

    let response = state.transport.open(&request).await?;
    let status = response.status();
    if !status.is_success() {
        let raw = state.transport.read_body(response).await?;
        return Err(ProxyError::from_upstream(status.as_u16(), &raw));
    }

    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    let tier_header = plan
        .as_ref()
        .map(|p| HeaderValue::from_static(p.effective.as_str()));
    let mut check = TrafficCheck::new(plan, route.upstream_model);

    let mut out = if stream {
        let chunks = state.transport.chunks(response, VERTEX);
        relay(chunks, Protocol::Gemini, move |chunk| check.feed(chunk))
    } else {
        let data = state.transport.read_body(response).await?;
        check.feed(&data);
        Response::new(Body::from(data))
    };
    *out.status_mut() = status;
    out.headers_mut().insert(header::CONTENT_TYPE, content_type);
    if let Some(tier) = tier_header {
        out.headers_mut().insert("x-vertexswitch-tier", tier);
    }
    Ok(out)
}

fn gemini_model(id: &str) -> Value {
    json!({
        "name": format!("models/{id}"),
        "displayName": id,
        "supportedGenerationMethods": gemini::ACTIONS,
    })
}

async fn list_models(State(app): State<SharedState>) -> Json<Value> {
    let state = app.runtime();
    let models: Vec<Value> = state
        .registry()
        .aliases(Backend::Gemini)
        .iter()
        .map(|id| gemini_model(id))
        .collect();
    Json(json!({ "models": models }))
}

async fn get_model(
    State(app): State<SharedState>,
    Path(target): Path<String>,
) -> ProxyResult<impl IntoResponse> {
    let state = app.runtime();
    let id = target.strip_prefix("models/").unwrap_or(&target);
    if !state
        .registry()
        .aliases(Backend::Gemini)
        .iter()
        .any(|a| a == id)
    {
        return Err(ProxyError::new(404, format!("未知模型 {id:?}")));
    }
    Ok(Json(gemini_model(id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_without_key() {
        assert_eq!(forwarded_query(Some("alt=sse&key=k")), "alt=sse");
        assert_eq!(forwarded_query(Some("key=k")), "");
        assert_eq!(forwarded_query(None), "");
    }

    #[test]
    fn target_split() {
        assert_eq!(
            split_target("models/gemini-x:generateContent").unwrap(),
            ("models/gemini-x", "generateContent")
        );
        assert!(split_target("gemini-x:predict").is_err());
        assert!(split_target("gemini-x").is_err());
    }
}
