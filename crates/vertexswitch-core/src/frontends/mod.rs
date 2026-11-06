//! 入口协议模块：解析客户端请求、选路由，必要时翻译请求体，再交给后端模块（`backends`）发出去，
//! 并选择把上游回复原样转交还是翻译成客户端协议。

pub mod anthropic;
pub mod gemini;
pub mod models;
pub mod openai;

use serde_json::Value;

use crate::errors::{ProxyError, ProxyResult};
use crate::tr;

/// 请求体必须是 JSON 对象。
pub fn parse_json_object(body: &[u8]) -> ProxyResult<Value> {
    let value: Value = serde_json::from_slice(body).map_err(|e| {
        ProxyError::new(
            400,
            tr!(
                "请求体不是合法的 JSON：{e}",
                "The request body isn't valid JSON: {e}"
            ),
        )
    })?;
    if !value.is_object() {
        return Err(ProxyError::new(
            400,
            tr!(
                "请求体必须是 JSON 对象",
                "The request body must be a JSON object"
            ),
        ));
    }
    Ok(value)
}

/// 请求体里的 `model`（去掉首尾空白，不能为空）。
pub fn require_model(body: &Value) -> ProxyResult<String> {
    body.get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ProxyError::new(400, tr!("model: 缺少模型名", "model: missing model name")))
}

/// 请求体里的 `"stream": true`。
pub fn wants_stream(body: &Value) -> bool {
    body.get("stream").and_then(Value::as_bool) == Some(true)
}
