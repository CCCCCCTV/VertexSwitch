//! 入口协议：把客户端请求转给对应后端，并把响应按入口协议返回。

pub mod anthropic;
pub mod gemini;
pub mod models;
pub mod openai;

use axum::{
    body::Body,
    http::{HeaderValue, header},
    response::Response,
};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde_json::Value;

use crate::errors::{Protocol, ProxyError, ProxyResult};

/// 请求体必须是 JSON 对象。
pub fn parse_json_object(body: &[u8]) -> ProxyResult<Value> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|e| ProxyError::new(400, format!("请求体不是合法的 JSON：{e}")))?;
    if !value.is_object() {
        return Err(ProxyError::new(400, "请求体必须是 JSON 对象"));
    }
    Ok(value)
}

/// 原样转发上游流；中途出错时按入口协议补发一个错误事件后结束（SPEC §5）。
/// `on_chunk` 用于旁路观察数据（如核对 trafficType）。
pub fn relay(
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
                    yield Ok::<_, std::convert::Infallible>(chunk);
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
