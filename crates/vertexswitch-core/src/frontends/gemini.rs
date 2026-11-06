//! Gemini 原生入口（SPEC §8.2）与 Gemini 格式模型列表（SPEC §8.4）。
//!
//! `POST /v1beta/models/{model}:{action}`，action 为 generateContent / streamGenerateContent /
//! countTokens。查询参数（如 `alt=sse`）透传，代理自己的 `key` 除外。生成类请求按配置附加
//! 服务档位头，并用响应里的 `usageMetadata.trafficType` 核对实际档位。

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, RawQuery, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::{Value, json};

use super::parse_json_object;
use crate::app::SharedState;
use crate::backends::{self, Target, gemini};
use crate::errors::{Protocol, ProxyError, ProxyResult};
use crate::registry::Backend;
use crate::tr;

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
            tr!(
                "不支持的 Gemini 方法：{target:?}",
                "Unsupported Gemini method: {target:?}"
            ),
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
        .ok_or_else(|| {
            ProxyError::new(
                404,
                tr!(
                    "未知的 Gemini 模型 {model:?}",
                    "Unknown Gemini model {model:?}"
                ),
            )
        })?;
    let body = parse_json_object(&body)?;
    let target = Target::GeminiNative {
        model: route.upstream_model,
        action: action.to_owned(),
        query: forwarded_query(query.as_deref()),
        tier_override: headers
            .get(gemini::TIER_OVERRIDE_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
    };
    backends::call(&state, target, body)
        .await?
        .relay(Protocol::Gemini)
        .await
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
        return Err(ProxyError::new(
            404,
            tr!("未知模型 {id:?}", "Unknown model {id:?}"),
        ));
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
