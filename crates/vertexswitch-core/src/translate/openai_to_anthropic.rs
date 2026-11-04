//! OpenAI Chat Completions ⇄ Anthropic Messages 转换（SPEC §10）：纯函数 + 流式状态机。
//!
//! 覆盖：system / developer 消息、文本与 base64 图片、工具定义、tool_choice、助手的 tool_calls、
//! tool 角色消息 → tool_result、停止原因、用量，以及流式中的文本、思考、工具调用参数
//! （`input_json_delta`）。
//!
//! 参考：<https://platform.claude.com/docs/en/api/messages/create>、
//! <https://platform.claude.com/docs/en/build-with-claude/streaming>

use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::errors::{ProxyError, ProxyResult};

pub(crate) static DATA_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)^data:(?P<media>[\w/+.-]+);base64,(?P<data>.+)$").expect("合法正则")
});

pub(crate) fn bad(message: impl Into<String>) -> ProxyError {
    ProxyError::new(400, message)
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------- 请求

/// OpenAI 的 content（字符串或分段数组）里的纯文本。
pub(crate) fn text_of(content: Option<&Value>) -> ProxyResult<String> {
    match content {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Array(parts)) => Ok(parts
            .iter()
            .filter(|p| p.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect()),
        Some(_) => Err(bad("messages[].content 格式不正确")),
    }
}

fn image_block(part: &Value) -> ProxyResult<Value> {
    let image = part.get("image_url");
    let url = image
        .and_then(|i| i.get("url"))
        .or(image)
        .and_then(Value::as_str)
        .ok_or_else(|| bad("image_url.url 缺失"))?;
    // Vertex 上的 Claude 不支持 URL 图片来源，只能内嵌 base64
    let caps = DATA_URL.captures(url).ok_or_else(|| {
        bad("Vertex AI 上的 Claude 不支持 URL 图片，请改用 data:image/...;base64, 内嵌图片")
    })?;
    Ok(json!({
        "type": "image",
        "source": {"type": "base64", "media_type": &caps["media"], "data": &caps["data"]},
    }))
}

fn user_blocks(content: Option<&Value>) -> ProxyResult<Vec<Value>> {
    match content {
        Some(Value::String(s)) => Ok(vec![json!({"type": "text", "text": s})]),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|part| match part.get("type").and_then(Value::as_str) {
                Some("text") => Ok(json!({
                    "type": "text",
                    "text": part.get("text").and_then(Value::as_str).unwrap_or_default(),
                })),
                Some("image_url") => image_block(part),
                other => Err(bad(format!("不支持的消息内容类型：{other:?}"))),
            })
            .collect(),
        _ => Err(bad("user 消息的 content 必须是字符串或数组")),
    }
}

fn assistant_blocks(msg: &Value) -> ProxyResult<Vec<Value>> {
    let mut blocks = Vec::new();
    let text = text_of(msg.get("content"))?;
    if !text.is_empty() {
        blocks.push(json!({"type": "text", "text": text}));
    }
    let calls = msg.get("tool_calls").and_then(Value::as_array);
    for call in calls.into_iter().flatten() {
        let function = call.get("function").cloned().unwrap_or_default();
        let input = match function.get("arguments") {
            None | Some(Value::Null) => json!({}),
            Some(Value::String(raw)) if raw.is_empty() => json!({}),
            Some(Value::String(raw)) => serde_json::from_str(raw)
                .map_err(|e| bad(format!("tool_calls 的 arguments 不是合法 JSON：{e}")))?,
            Some(other) => other.clone(),
        };
        blocks.push(json!({
            "type": "tool_use",
            "id": call.get("id"),
            "name": function.get("name"),
            "input": input,
        }));
    }
    Ok(blocks)
}

fn convert_tools(tools: &[Value]) -> ProxyResult<Vec<Value>> {
    tools
        .iter()
        .map(|tool| {
            let kind = tool.get("type").and_then(Value::as_str);
            if kind != Some("function") {
                return Err(bad(format!("不支持的工具类型：{kind:?}")));
            }
            let function = tool.get("function").cloned().unwrap_or_default();
            let mut item = Map::new();
            item.insert(
                "name".into(),
                function.get("name").cloned().unwrap_or_default(),
            );
            if let Some(desc) = function
                .get("description")
                .and_then(Value::as_str)
                .filter(|d| !d.is_empty())
            {
                item.insert("description".into(), desc.into());
            }
            let schema = function
                .get("parameters")
                .filter(|p| !p.is_null())
                .cloned()
                .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
            item.insert("input_schema".into(), schema);
            Ok(Value::Object(item))
        })
        .collect()
}

fn convert_tool_choice(
    choice: Option<&Value>,
    parallel: Option<&Value>,
) -> ProxyResult<Option<Value>> {
    let mut result = match choice {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(match s.as_str() {
            "auto" => json!({"type": "auto"}),
            "none" => json!({"type": "none"}),
            "required" => json!({"type": "any"}),
            other => return Err(bad(format!("不支持的 tool_choice：{other:?}"))),
        }),
        Some(obj) if obj.get("type").and_then(Value::as_str) == Some("function") => Some(json!({
            "type": "tool",
            "name": obj.get("function").and_then(|f| f.get("name")),
        })),
        Some(other) => return Err(bad(format!("不支持的 tool_choice：{other}"))),
    };
    if parallel == Some(&Value::Bool(false)) {
        let choice = result.get_or_insert_with(|| json!({"type": "auto"}));
        if choice["type"] != "none" {
            choice["disable_parallel_tool_use"] = true.into();
        }
    }
    Ok(result)
}

/// OpenAI 请求体 → Anthropic Messages 请求体（不含 model，由调用方填）。
pub fn request_to_anthropic(body: &Value, default_max_tokens: u64) -> ProxyResult<Value> {
    match body.get("n") {
        None | Some(Value::Null) => {}
        Some(n) if n.as_u64() == Some(1) => {}
        Some(_) => return Err(bad("Claude 不支持 n > 1")),
    }
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .filter(|m| !m.is_empty())
        .ok_or_else(|| bad("messages 不能为空"))?;

    let mut system_parts: Vec<String> = Vec::new();
    let mut out_messages: Vec<Value> = Vec::new();
    // Anthropic 要求 user / assistant 交替：同角色的连续消息合并
    let mut append = |role: &str, blocks: Vec<Value>| {
        if blocks.is_empty() {
            return;
        }
        if let Some(last) = out_messages.last_mut()
            && last["role"] == role
        {
            last["content"]
                .as_array_mut()
                .expect("content 总是数组")
                .extend(blocks);
            return;
        }
        out_messages.push(json!({"role": role, "content": blocks}));
    };

    for msg in messages {
        if !msg.is_object() {
            return Err(bad("messages[] 必须是对象"));
        }
        match msg.get("role").and_then(Value::as_str) {
            Some("system" | "developer") => system_parts.push(text_of(msg.get("content"))?),
            Some("user") => append("user", user_blocks(msg.get("content"))?),
            Some("assistant") => append("assistant", assistant_blocks(msg)?),
            Some("tool") => append(
                "user",
                vec![json!({
                    "type": "tool_result",
                    "tool_use_id": msg.get("tool_call_id"),
                    "content": text_of(msg.get("content"))?,
                })],
            ),
            other => return Err(bad(format!("不支持的消息角色：{other:?}"))),
        }
    }

    let max_tokens = ["max_completion_tokens", "max_tokens"]
        .iter()
        .find_map(|k| body.get(*k).and_then(Value::as_u64).filter(|v| *v > 0))
        .unwrap_or(default_max_tokens);
    let mut out = Map::new();
    out.insert("messages".into(), out_messages.into());
    out.insert("max_tokens".into(), max_tokens.into());
    let system: Vec<&str> = system_parts
        .iter()
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .collect();
    if !system.is_empty() {
        out.insert("system".into(), system.join("\n\n").into());
    }
    for key in ["temperature", "top_p"] {
        if let Some(v) = body.get(key).filter(|v| !v.is_null()) {
            out.insert(key.into(), v.clone());
        }
    }
    match body.get("stop") {
        Some(Value::String(s)) if !s.is_empty() => {
            out.insert("stop_sequences".into(), json!([s]));
        }
        Some(Value::Array(items)) if !items.is_empty() => {
            out.insert("stop_sequences".into(), Value::Array(items.clone()));
        }
        _ => {}
    }
    if let Some(tools) = body
        .get("tools")
        .and_then(Value::as_array)
        .filter(|t| !t.is_empty())
    {
        out.insert("tools".into(), convert_tools(tools)?.into());
    }
    if let Some(choice) =
        convert_tool_choice(body.get("tool_choice"), body.get("parallel_tool_calls"))?
    {
        out.insert("tool_choice".into(), choice);
    }
    if let Some(user) = body.get("user").and_then(Value::as_str) {
        out.insert("metadata".into(), json!({"user_id": user}));
    }
    if body.get("stream").and_then(Value::as_bool) == Some(true) {
        out.insert("stream".into(), true.into());
    }
    Ok(Value::Object(out))
}

// ---------------------------------------------------------------- 非流式响应

pub fn finish_reason(stop_reason: &str) -> &'static str {
    match stop_reason {
        "max_tokens" | "model_context_window_exceeded" => "length",
        "tool_use" => "tool_calls",
        "refusal" => "content_filter",
        // end_turn、stop_sequence、pause_turn 及未知值
        _ => "stop",
    }
}

fn openai_usage(usage: &Value) -> Value {
    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    let cached = n("cache_read_input_tokens");
    let prompt = n("input_tokens") + cached + n("cache_creation_input_tokens");
    let completion = n("output_tokens");
    let mut out = json!({
        "prompt_tokens": prompt,
        "completion_tokens": completion,
        "total_tokens": prompt + completion,
    });
    if cached > 0 {
        out["prompt_tokens_details"] = json!({"cached_tokens": cached});
    }
    out
}

pub fn response_to_openai(msg: &Value, model: &str) -> Value {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = Vec::new();
    for block in msg
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => text.push_str(block.get("text").and_then(Value::as_str).unwrap_or("")),
            Some("thinking") => {
                reasoning.push_str(block.get("thinking").and_then(Value::as_str).unwrap_or(""));
            }
            Some("tool_use") => tool_calls.push(json!({
                "id": block.get("id"),
                "type": "function",
                "function": {
                    "name": block.get("name"),
                    "arguments": block.get("input").cloned().unwrap_or_else(|| json!({})).to_string(),
                },
            })),
            _ => {}
        }
    }
    let mut message = json!({
        "role": "assistant",
        "content": if text.is_empty() { Value::Null } else { text.into() },
    });
    if !reasoning.is_empty() {
        message["reasoning_content"] = reasoning.into();
    }
    if !tool_calls.is_empty() {
        message["tool_calls"] = tool_calls.into();
    }
    json!({
        "id": msg.get("id").and_then(Value::as_str).unwrap_or("chatcmpl-vertexswitch"),
        "object": "chat.completion",
        "created": now_secs(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": msg.get("stop_reason").and_then(Value::as_str).map(finish_reason),
        }],
        "usage": openai_usage(msg.get("usage").unwrap_or(&Value::Null)),
    })
}

// ---------------------------------------------------------------- 流式

/// 把任意切分的字节流解析成 (event, data) 对。
///
/// 在字节层面找事件边界（完整的事件不会切开多字节 UTF-8 字符）；每次对整个缓冲区把 CRLF
/// 归一为 LF，这样被切开的 CRLF 也能正确识别。
#[derive(Default)]
pub struct SseParser {
    buffer: Vec<u8>,
}

impl SseParser {
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<(String, String)> {
        self.buffer.extend_from_slice(chunk);
        if self.buffer.windows(2).any(|w| w == b"\r\n") {
            let mut normalized = Vec::with_capacity(self.buffer.len());
            let mut i = 0;
            while i < self.buffer.len() {
                if self.buffer[i] == b'\r' && self.buffer.get(i + 1) == Some(&b'\n') {
                    i += 1;
                    continue;
                }
                normalized.push(self.buffer[i]);
                i += 1;
            }
            self.buffer = normalized;
        }
        let mut events = Vec::new();
        while let Some(pos) = self.buffer.windows(2).position(|w| w == b"\n\n") {
            let raw: Vec<u8> = self.buffer.drain(..pos + 2).collect();
            let raw = String::from_utf8_lossy(&raw[..pos]);
            let mut event = "message".to_owned();
            let mut data: Vec<&str> = Vec::new();
            for line in raw.split('\n') {
                if let Some(rest) = line.strip_prefix("event:") {
                    event = rest.trim().to_owned();
                } else if let Some(rest) = line.strip_prefix("data:") {
                    data.push(rest.trim_start());
                }
            }
            if !data.is_empty() {
                events.push((event, data.join("\n")));
            }
        }
        events
    }
}

/// Anthropic 流式事件 → OpenAI `chat.completion.chunk`。
///
/// 工具调用参数以 `input_json_delta` 分段到达，按 content block 下标映射到 OpenAI 的
/// `tool_calls[].index` 后逐段转发。
pub struct StreamTranslator {
    model: String,
    include_usage: bool,
    id: String,
    created: u64,
    tool_index: HashMap<u64, usize>,
    usage: Map<String, Value>,
    pub finished: bool,
}

impl StreamTranslator {
    pub fn new(model: &str, include_usage: bool) -> Self {
        Self {
            model: model.to_owned(),
            include_usage,
            id: "chatcmpl-vertexswitch".into(),
            created: now_secs(),
            tool_index: HashMap::new(),
            usage: Map::new(),
            finished: false,
        }
    }

    fn chunk(&self, delta: Value, finish: Option<&str>) -> Value {
        json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
        })
    }

    fn merge_usage(&mut self, usage: Option<&Value>) {
        if let Some(Value::Object(u)) = usage {
            for (k, v) in u {
                self.usage.insert(k.clone(), v.clone());
            }
        }
    }

    pub fn handle(&mut self, event: &str, data: &Value) -> Vec<Value> {
        let kind = data.get("type").and_then(Value::as_str).unwrap_or(event);
        let block_index = data.get("index").and_then(Value::as_u64).unwrap_or(0);
        match kind {
            "message_start" => {
                let msg = data.get("message").cloned().unwrap_or_default();
                if let Some(id) = msg.get("id").and_then(Value::as_str) {
                    self.id = id.to_owned();
                }
                self.merge_usage(msg.get("usage"));
                vec![self.chunk(json!({"role": "assistant", "content": ""}), None)]
            }
            "content_block_start" => {
                let block = data.get("content_block").cloned().unwrap_or_default();
                if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                    return vec![];
                }
                let index = self.tool_index.len();
                self.tool_index.insert(block_index, index);
                let call = json!({
                    "index": index,
                    "id": block.get("id"),
                    "type": "function",
                    "function": {"name": block.get("name"), "arguments": ""},
                });
                vec![self.chunk(json!({"tool_calls": [call]}), None)]
            }
            "content_block_delta" => {
                let delta = data.get("delta").cloned().unwrap_or_default();
                let field = |k: &str| {
                    delta
                        .get(k)
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned()
                };
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        vec![self.chunk(json!({"content": field("text")}), None)]
                    }
                    Some("thinking_delta") => {
                        vec![self.chunk(json!({"reasoning_content": field("thinking")}), None)]
                    }
                    Some("input_json_delta") => match self.tool_index.get(&block_index) {
                        Some(&index) => {
                            let call = json!({
                                "index": index,
                                "function": {"arguments": field("partial_json")},
                            });
                            vec![self.chunk(json!({"tool_calls": [call]}), None)]
                        }
                        None => vec![],
                    },
                    _ => vec![],
                }
            }
            "message_delta" => {
                self.merge_usage(data.get("usage"));
                let reason = data
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(Value::as_str)
                    .map(finish_reason);
                reason
                    .map(|r| vec![self.chunk(json!({}), Some(r))])
                    .unwrap_or_default()
            }
            "message_stop" => {
                self.finished = true;
                if !self.include_usage {
                    return vec![];
                }
                let mut last = self.chunk(json!({}), None);
                last["choices"] = json!([]);
                last["usage"] = openai_usage(&Value::Object(self.usage.clone()));
                vec![last]
            }
            // ping 与未知事件忽略
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests;
