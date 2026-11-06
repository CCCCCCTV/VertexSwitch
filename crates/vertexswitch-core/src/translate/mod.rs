//! 协议之间的转换。
//!
//! 每个翻译模块提供请求翻译、非流式响应翻译，以及实现 [`StreamTranslate`] 的流式翻译器。
//! 发请求、解析上游 SSE、处理上游错误事件与提前结束都由后端模块负责（见 `backends`），翻译器只管正常事件。

pub mod anthropic_to_gemini;
pub mod openai_to_anthropic;
pub mod openai_to_gemini;
pub mod sse;

use bytes::Bytes;
use serde_json::Value;

/// 把上游的流式事件逐个翻译成客户端协议的 SSE 帧。
pub trait StreamTranslate: Send + 'static {
    /// 一个上游事件（`event` 为 SSE 的事件名，没有时是 `message`）→ 若干已编码好的帧。
    fn on_event(&mut self, event: &str, data: &Value) -> Vec<Bytes>;

    /// 上游的流结束了：返回收尾的帧；流没有正常走完（缺少结束事件）时返回 None，
    /// 由后端模块补发“流式响应提前结束”的错误事件。
    fn finish(&mut self) -> Option<Vec<Bytes>>;
}

/// OpenAI 流式的一帧：`data: …` 加空行。
pub fn openai_frame(payload: &str) -> Bytes {
    format!("data: {payload}\n\n").into()
}
