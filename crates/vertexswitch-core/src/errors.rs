//! 按入口协议渲染错误（SPEC §5）。
//!
//! 处理函数返回 [`ProxyError`]；它先以"占位响应 + 扩展"的形式离开处理函数，
//! 再由 [`render_errors`] 中间件按请求路径判定的入口协议写成对应格式的 JSON。

use axum::{
    Json,
    body::Body,
    extract::Request,
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Anthropic,
    OpenAi,
    Gemini,
}

/// 代理内部统一的错误。
#[derive(Debug, Clone, thiserror::Error)]
#[error("{status}: {message}")]
pub struct ProxyError {
    pub status: u16,
    pub message: String,
    /// 上游原样的错误体（已解析）；已是目标协议格式时直接透传
    pub upstream_body: Option<Value>,
}

impl ProxyError {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            upstream_body: None,
        }
    }

    /// 把上游的非 2xx 响应包装成错误，保留原始错误体。
    pub fn from_upstream(status: u16, raw: &[u8]) -> Self {
        let body: Option<Value> = serde_json::from_slice(raw).ok();
        let message = body.as_ref().and_then(extract_message).unwrap_or_else(|| {
            let text = String::from_utf8_lossy(raw);
            let text = text.trim();
            if text.is_empty() {
                format!("上游返回 HTTP {status}")
            } else {
                text.chars().take(2000).collect()
            }
        });
        Self {
            status,
            message,
            upstream_body: body,
        }
    }

    pub fn status_code(&self) -> StatusCode {
        StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    pub fn body(&self, protocol: Protocol) -> Value {
        match protocol {
            Protocol::Anthropic => {
                if let Some(body) = self
                    .upstream_body
                    .as_ref()
                    .filter(|b| is_anthropic_error(b))
                {
                    return body.clone();
                }
                json!({
                    "type": "error",
                    "error": { "type": anthropic_error_type(self.status), "message": self.message },
                })
            }
            Protocol::Gemini => {
                if let Some(body) = self.upstream_body.as_ref().and_then(google_error) {
                    return body;
                }
                json!({
                    "error": {
                        "code": self.status,
                        "message": self.message,
                        "status": google_status(self.status),
                    }
                })
            }
            Protocol::OpenAi => json!({
                "error": {
                    "message": self.message,
                    "type": openai_error_type(self.status),
                    "param": null,
                    "code": self.status,
                }
            }),
        }
    }

    pub fn render(&self, protocol: Protocol) -> Response {
        (self.status_code(), Json(self.body(protocol))).into_response()
    }

    /// 流式响应开始后出错时发给客户端的最后一个事件。开头的空行结束可能被截断的上一个事件。
    pub fn stream_event(&self, protocol: Protocol) -> bytes::Bytes {
        let payload = self.body(protocol).to_string();
        match protocol {
            Protocol::Anthropic => format!("\n\nevent: error\ndata: {payload}\n\n"),
            _ => format!("\n\ndata: {payload}\n\n"),
        }
        .into()
    }
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let mut response = self.status_code().into_response();
        response.extensions_mut().insert(self);
        response
    }
}

pub type ProxyResult<T> = Result<T, ProxyError>;

/// 根据请求路径判断入口协议。
pub fn protocol_for_path(path: &str) -> Protocol {
    if path.starts_with("/anthropic/") || path.starts_with("/v1/messages") {
        return Protocol::Anthropic;
    }
    let last = path.rsplit('/').next().unwrap_or("");
    if path.starts_with("/gemini/")
        || path.starts_with("/v1beta/")
        || (path.contains("/models/") && last.contains(':'))
    {
        return Protocol::Gemini;
    }
    Protocol::OpenAi
}

/// 中间件：把 [`ProxyError`] 以及框架自身的 404 / 405 等空响应渲染成入口协议格式。
pub async fn render_errors(request: Request, next: Next) -> Response {
    let protocol = protocol_for_path(request.uri().path());
    let mut response = next.run(request).await;
    if let Some(err) = response.extensions_mut().remove::<ProxyError>() {
        return err.render(protocol);
    }
    let status = response.status();
    let has_body = response.headers().contains_key(header::CONTENT_TYPE);
    if (status.is_client_error() || status.is_server_error()) && !has_body {
        let reason = status.canonical_reason().unwrap_or("error");
        return ProxyError::new(status.as_u16(), reason).render(protocol);
    }
    response
}

/// 未知路由。
pub async fn not_found() -> ProxyError {
    ProxyError::new(404, "未知的路径")
}

/// 处理函数 panic 时仍按 JSON 返回（协议由外层 [`render_errors`] 决定）。
pub fn panic_response(_: Box<dyn std::any::Any + Send + 'static>) -> Response<Body> {
    ProxyError::new(500, "代理内部错误").into_response()
}

fn extract_message(body: &Value) -> Option<String> {
    let body = match body {
        Value::Array(items) => items.first()?,
        other => other,
    };
    match body.get("error") {
        Some(Value::Object(err)) => err.get("message").and_then(Value::as_str).map(Into::into),
        Some(Value::String(s)) => Some(s.clone()),
        _ => body.get("message").and_then(Value::as_str).map(Into::into),
    }
}

fn is_anthropic_error(body: &Value) -> bool {
    body.get("type").and_then(Value::as_str) == Some("error")
        && body.get("error").and_then(|e| e.get("type")).is_some()
}

/// Vertex 的 Google 格式错误体（可能包在单元素数组里）。
fn google_error(body: &Value) -> Option<Value> {
    let body = match body {
        Value::Array(items) if items.len() == 1 => &items[0],
        other => other,
    };
    let err = body.get("error")?;
    (err.get("code").is_some() && err.get("message").is_some()).then(|| json!({ "error": err }))
}

/// https://platform.claude.com/docs/en/api/errors
pub fn anthropic_error_type(status: u16) -> &'static str {
    match status {
        401 => "authentication_error",
        402 => "billing_error",
        403 => "permission_error",
        404 => "not_found_error",
        413 => "request_too_large",
        429 => "rate_limit_error",
        500 => "api_error",
        504 => "timeout_error",
        529 => "overloaded_error",
        400..=499 => "invalid_request_error",
        _ => "api_error",
    }
}

fn google_status(status: u16) -> &'static str {
    match status {
        400 | 413 => "INVALID_ARGUMENT",
        401 => "UNAUTHENTICATED",
        403 => "PERMISSION_DENIED",
        404 => "NOT_FOUND",
        409 => "ABORTED",
        429 => "RESOURCE_EXHAUSTED",
        499 => "CANCELLED",
        500 => "INTERNAL",
        501 => "UNIMPLEMENTED",
        502 | 503 => "UNAVAILABLE",
        504 => "DEADLINE_EXCEEDED",
        _ => "UNKNOWN",
    }
}

fn openai_error_type(status: u16) -> &'static str {
    match status {
        400 => "invalid_request_error",
        401 => "authentication_error",
        403 => "permission_error",
        404 => "not_found_error",
        429 => "rate_limit_error",
        _ => "api_error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_detection() {
        assert_eq!(protocol_for_path("/v1/messages"), Protocol::Anthropic);
        assert_eq!(
            protocol_for_path("/anthropic/v1/models"),
            Protocol::Anthropic
        );
        assert_eq!(
            protocol_for_path("/v1beta/models/x:generateContent"),
            Protocol::Gemini
        );
        assert_eq!(protocol_for_path("/v1/models/x:predict"), Protocol::Gemini);
        assert_eq!(protocol_for_path("/v1/chat/completions"), Protocol::OpenAi);
        assert_eq!(protocol_for_path("/v1/models"), Protocol::OpenAi);
    }

    #[test]
    fn bodies_per_protocol() {
        let err = ProxyError::new(504, "slow");
        assert_eq!(
            err.body(Protocol::Anthropic),
            json!({"type": "error", "error": {"type": "timeout_error", "message": "slow"}})
        );
        assert_eq!(
            err.body(Protocol::Gemini)["error"]["status"],
            "DEADLINE_EXCEEDED"
        );
        assert_eq!(err.body(Protocol::OpenAi)["error"]["message"], "slow");
    }

    #[test]
    fn anthropic_types_fallback() {
        assert_eq!(anthropic_error_type(418), "invalid_request_error");
        assert_eq!(anthropic_error_type(503), "api_error");
    }

    #[test]
    fn upstream_google_error_in_array() {
        let raw =
            br#"[{"error":{"code":429,"message":"Quota exceeded","status":"RESOURCE_EXHAUSTED"}}]"#;
        let err = ProxyError::from_upstream(429, raw);
        assert_eq!(err.message, "Quota exceeded");
        assert_eq!(
            err.body(Protocol::Gemini),
            json!({"error": {"code": 429, "message": "Quota exceeded", "status": "RESOURCE_EXHAUSTED"}})
        );
        assert_eq!(
            err.body(Protocol::Anthropic),
            json!({"type": "error", "error": {"type": "rate_limit_error", "message": "Quota exceeded"}})
        );
    }

    #[test]
    fn upstream_anthropic_error_passthrough() {
        let raw = br#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        let err = ProxyError::from_upstream(529, raw);
        assert_eq!(
            err.body(Protocol::Anthropic),
            serde_json::from_slice::<Value>(raw).unwrap()
        );
    }

    #[test]
    fn upstream_non_json_and_empty() {
        let err = ProxyError::from_upstream(503, b"<html>Service Unavailable</html>");
        assert!(err.message.contains("Service Unavailable"));
        assert_eq!(
            ProxyError::from_upstream(502, b"").message,
            "上游返回 HTTP 502"
        );
    }

    #[test]
    fn stream_event_shapes() {
        let err = ProxyError::new(502, "bad");
        let a = err.stream_event(Protocol::Anthropic);
        assert!(a.starts_with(b"\n\nevent: error\ndata: "));
        let o = err.stream_event(Protocol::OpenAi);
        assert!(o.starts_with(b"\n\ndata: {\"error\""));
    }
}
