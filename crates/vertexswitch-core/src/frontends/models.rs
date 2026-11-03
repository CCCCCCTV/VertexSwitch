//! 模型列表（SPEC §8.4）：客户端探测用。
//!
//! `/v1/models` 同时被 Anthropic 和 OpenAI 客户端使用：带 `anthropic-version` 头或 `/anthropic/`
//! 前缀时按 Anthropic 格式返回（只列 Claude），否则按 OpenAI 格式返回（全部后端）。
//! Gemini 格式的列表在 `frontends::gemini` 里。

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, Uri},
    routing::get,
};
use serde_json::{Value, json};

use super::gemini::model_action;
use crate::app::{AppState, SharedState};
use crate::errors::{ProxyError, ProxyResult};
use crate::registry::Backend;

const EPOCH: &str = "1970-01-01T00:00:00Z";

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/v1/models", get(list_models))
        .route("/anthropic/v1/models", get(list_models))
        // POST 是 Gemini 的 /v1/models/{model}:{action}
        .route("/v1/models/{*id}", get(get_model).post(model_action))
        .route("/anthropic/v1/models/{*id}", get(get_model))
}

fn anthropic_model(id: &str) -> Value {
    json!({"type": "model", "id": id, "display_name": id, "created_at": EPOCH})
}

fn openai_model(id: &str) -> Value {
    json!({"id": id, "object": "model", "created": 0, "owned_by": "vertexswitch"})
}

fn wants_anthropic(uri: &Uri, headers: &HeaderMap) -> bool {
    uri.path().starts_with("/anthropic/") || headers.contains_key("anthropic-version")
}

/// OpenAI 格式列出的全部模型：Claude、Gemini、MaaS 别名，以及启用时发现的 Ollama 模型。
fn all_ids(state: &AppState) -> Vec<String> {
    let registry = state.registry();
    let mut ids = registry.aliases(Backend::Claude);
    ids.extend(registry.aliases(Backend::Gemini));
    ids.extend(registry.aliases(Backend::Maas));
    if state.settings.ollama.enabled {
        ids.extend(
            registry
                .aliases(Backend::Ollama)
                .into_iter()
                .map(|m| format!("ollama/{m}")),
        );
    }
    ids
}

async fn list_models(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Json<Value> {
    if wants_anthropic(&uri, &headers) {
        let ids = state.registry().aliases(Backend::Claude);
        return Json(json!({
            "data": ids.iter().map(|id| anthropic_model(id)).collect::<Vec<_>>(),
            "has_more": false,
            "first_id": ids.first(),
            "last_id": ids.last(),
        }));
    }
    let data: Vec<Value> = all_ids(&state).iter().map(|id| openai_model(id)).collect();
    Json(json!({"object": "list", "data": data}))
}

async fn get_model(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> ProxyResult<Json<Value>> {
    if !all_ids(&state).contains(&id) {
        return Err(ProxyError::new(404, format!("未知模型 {id:?}")));
    }
    Ok(Json(if wants_anthropic(&uri, &headers) {
        anthropic_model(&id)
    } else {
        openai_model(&id)
    }))
}
