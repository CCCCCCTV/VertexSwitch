//! Anthropic Messages ⇄ Gemini 原生 generateContent 转换（SPEC §14）：让只会说 Claude 格式的客户端
//! （如 Claude Code）调用 Gemini 模型。纯函数 + 流式状态机。
//!
//! - `system` / `messages` → `systemInstruction` / `contents`；text、image、document（PDF）、tool_use、
//!   tool_result 照转，`cache_control` 忽略
//! - 工具：`input_schema` 放进 `parametersJsonSchema`；`tool_choice` auto / any / tool / none →
//!   AUTO / ANY（指定函数）/ NONE。服务端工具（web_search 等）Gemini 这边没有对应物，跳过并记日志
//! - `thinking`：enabled 的 `budget_tokens` → `thinkingBudget`，并要回思考摘要（`includeThoughts`），
//!   转成 thinking 块
//! - 思考签名：Gemini 3 起函数调用必须带回签名。签名放在 tool_use 前面的 thinking 块的 `signature` 里，
//!   加 [`SIGNATURE_PREFIX`] 区分 Claude 自己的签名；客户端原样带回时挂回函数调用，没带回时填文档给的占位值
//! - Gemini 不给工具调用 ID，自己生成 `toolu_…`；工具结果按 ID 在历史里找回函数名
//!
//! 参考：
//! - <https://platform.claude.com/docs/en/api/messages/create>、<https://platform.claude.com/docs/en/build-with-claude/streaming>
//! - <https://docs.cloud.google.com/vertex-ai/generative-ai/docs/thought-signatures>
//! - v1 discovery 文档里的 GenerateContentRequest / Part / FunctionDeclaration / Candidate / CountTokensRequest

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use serde_json::{Map, Value, json};

use super::openai_to_anthropic::bad;
use super::openai_to_gemini::{SKIP_SIGNATURE, requires_signatures};
use crate::errors::ProxyResult;
use crate::tr;

/// 我们放进 thinking 块的 Gemini 思考签名的前缀
pub const SIGNATURE_PREFIX: &str = "vsg1:";

/// Gemini 不给调用 ID，自己生成一个不重复的（与 Anthropic 的 `toolu_` 同形）。
fn new_tool_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("toolu_vs{:x}{n:x}", nanos & 0xffff_ffff_ffff)
}

fn new_message_id() -> String {
    new_tool_id().replacen("toolu_", "msg_", 1)
}

// ---------------------------------------------------------------- 请求

/// content 可以是字符串或块数组；统一成块。
fn blocks(content: Option<&Value>) -> Vec<Value> {
    match content {
        Some(Value::String(s)) => vec![json!({"type": "text", "text": s})],
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    }
}

fn block_type(block: &Value) -> Option<&str> {
    block.get("type").and_then(Value::as_str)
}

fn guess_mime(url: &str) -> &'static str {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    match path
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("pdf") => "application/pdf",
        _ => "image/jpeg",
    }
}

/// image / document 的 `source` → inlineData / fileData / 纯文本。
fn source_part(source: Option<&Value>) -> ProxyResult<Value> {
    let source = source.ok_or_else(|| bad(tr!("source 缺失", "source is missing")))?;
    let field = |k: &str| source.get(k).and_then(Value::as_str);
    match field("type") {
        Some("base64") => {
            let data = field("data")
                .ok_or_else(|| bad(tr!("source.data 缺失", "source.data is missing")))?;
            let mime = field("media_type").unwrap_or("image/jpeg");
            Ok(json!({"inlineData": {"mimeType": mime, "data": data}}))
        }
        Some("url") => {
            let url =
                field("url").ok_or_else(|| bad(tr!("source.url 缺失", "source.url is missing")))?;
            Ok(json!({"fileData": {"mimeType": guess_mime(url), "fileUri": url}}))
        }
        Some("text") => Ok(json!({"text": field("data").unwrap_or("")})),
        other => Err(bad(tr!(
            "不支持的 source 类型：{other:?}（支持 base64、url、text）",
            "Unsupported source type: {other:?} (base64, url and text are supported)"
        ))),
    }
}

/// tool_result 的内容：文字合成一段交给 functionResponse，图片等另作 part 跟在后面。
fn tool_result_parts(name: &str, block: &Value) -> ProxyResult<Vec<Value>> {
    let mut text = String::new();
    let mut extra = Vec::new();
    for item in blocks(block.get("content")) {
        match block_type(&item) {
            Some("text") => {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(item.get("text").and_then(Value::as_str).unwrap_or(""));
            }
            Some("image" | "document") => extra.push(source_part(item.get("source"))?),
            _ => {}
        }
    }
    let key = if block.get("is_error").and_then(Value::as_bool) == Some(true) {
        "error"
    } else {
        "output"
    };
    // 能解析成 JSON 就按 JSON 交回，否则按字符串（与 OpenAI 那条路一致）
    let value = serde_json::from_str::<Value>(&text)
        .ok()
        .filter(|v| !v.is_null())
        .unwrap_or(Value::String(text));
    let mut parts = vec![json!({"functionResponse": {"name": name, "response": {key: value}}})];
    parts.extend(extra);
    Ok(parts)
}

fn user_parts(content: Option<&Value>, names: &HashMap<String, String>) -> ProxyResult<Vec<Value>> {
    let mut parts = Vec::new();
    for block in blocks(content) {
        match block_type(&block) {
            Some("text") => {
                if let Some(t) = block
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty())
                {
                    parts.push(json!({"text": t}));
                }
            }
            Some("image" | "document") => parts.push(source_part(block.get("source"))?),
            Some("tool_result") => {
                let id = block
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let name = names.get(id).ok_or_else(|| {
                    bad(tr!(
                        "tool_result 找不到对应的 tool_use（tool_use_id {id:?} 不在历史里）",
                        "tool_result has no matching tool_use (tool_use_id {id:?} isn't in the history)"
                    ))
                })?;
                parts.extend(tool_result_parts(name, &block)?);
            }
            // 历史里 Claude 自己的思考、服务端工具结果等，Gemini 用不上
            Some(
                "thinking" | "redacted_thinking" | "web_search_tool_result" | "server_tool_use",
            ) => {}
            other => {
                return Err(bad(tr!(
                    "不支持的消息内容类型：{other:?}",
                    "Unsupported message content type: {other:?}"
                )));
            }
        }
    }
    Ok(parts)
}

/// 助手消息：文本 + 函数调用。我们放进 thinking 块的签名挂回它后面的函数调用。
fn assistant_parts(content: Option<&Value>, needs_signature: bool) -> Vec<Value> {
    let mut parts = Vec::new();
    let mut pending: Option<String> = None;
    let mut first_call = None;
    let mut signed = false;
    for block in blocks(content) {
        match block_type(&block) {
            Some("text") => {
                if let Some(t) = block
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty())
                {
                    parts.push(json!({"text": t}));
                }
            }
            Some("thinking") => {
                if let Some(sig) = block
                    .get("signature")
                    .and_then(Value::as_str)
                    .and_then(|s| s.strip_prefix(SIGNATURE_PREFIX))
                    .filter(|s| !s.is_empty())
                {
                    pending = Some(sig.to_owned());
                }
            }
            Some("tool_use") => {
                let mut part = json!({"functionCall": {
                    "name": block.get("name").cloned().unwrap_or_default(),
                    "args": block.get("input").filter(|i| i.is_object()).cloned().unwrap_or_else(|| json!({})),
                }});
                if let Some(sig) = pending.take() {
                    part["thoughtSignature"] = sig.into();
                    signed = true;
                }
                first_call.get_or_insert(parts.len());
                parts.push(part);
            }
            // redacted_thinking、服务端工具调用等：Gemini 用不上
            _ => {}
        }
    }
    // 只有一组并行调用里的第一个带签名；客户端把签名丢了时给第一个补占位值
    if let Some(first) = first_call.filter(|_| needs_signature && !signed) {
        parts[first]["thoughtSignature"] = SKIP_SIGNATURE.into();
    }
    parts
}

fn system_parts(system: Option<&Value>) -> Vec<Value> {
    blocks(system)
        .iter()
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .filter(|t| !t.is_empty())
        .map(|t| json!({"text": t}))
        .collect()
}

/// 工具定义 → functionDeclarations；服务端工具（带版本号的 type）跳过并记日志。
fn convert_tools(body: &Value) -> Vec<Value> {
    let mut declarations = Vec::new();
    let mut skipped = Vec::new();
    for tool in body
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match block_type(tool) {
            None | Some("custom") => {
                let mut decl = Map::new();
                decl.insert("name".into(), tool.get("name").cloned().unwrap_or_default());
                if let Some(desc) = tool
                    .get("description")
                    .and_then(Value::as_str)
                    .filter(|d| !d.is_empty())
                {
                    decl.insert("description".into(), desc.into());
                }
                if let Some(schema) = tool.get("input_schema").filter(|s| s.is_object()) {
                    let mut schema = schema.clone();
                    // 元信息字段对 Gemini 没有意义
                    if let Some(obj) = schema.as_object_mut() {
                        obj.remove("$schema");
                    }
                    decl.insert("parametersJsonSchema".into(), schema);
                }
                declarations.push(Value::Object(decl));
            }
            Some(other) => skipped.push(other.to_owned()),
        }
    }
    if !skipped.is_empty() {
        tracing::warn!(
            "{}",
            tr!(
                "Gemini 不支持这些 Claude 服务端工具，已跳过：{}",
                "Gemini doesn't support these Claude server tools; skipped: {}",
                skipped.join(", ")
            )
        );
    }
    if declarations.is_empty() {
        Vec::new()
    } else {
        vec![json!({"functionDeclarations": declarations})]
    }
}

fn tool_config(body: &Value) -> Option<Value> {
    let choice = body.get("tool_choice")?;
    let config = match block_type(choice)? {
        "auto" => json!({"mode": "AUTO"}),
        "any" => json!({"mode": "ANY"}),
        "none" => json!({"mode": "NONE"}),
        "tool" => json!({"mode": "ANY", "allowedFunctionNames": [choice.get("name")?]}),
        _ => return None,
    };
    Some(json!({"functionCallingConfig": config}))
}

fn generation_config(body: &Value) -> Map<String, Value> {
    let mut config = Map::new();
    if let Some(max) = body
        .get("max_tokens")
        .and_then(Value::as_u64)
        .filter(|v| *v > 0)
    {
        config.insert("maxOutputTokens".into(), max.into());
    }
    for (from, to) in [
        ("temperature", "temperature"),
        ("top_p", "topP"),
        ("top_k", "topK"),
    ] {
        if let Some(v) = body.get(from).filter(|v| v.is_number()) {
            config.insert(to.into(), v.clone());
        }
    }
    if let Some(stops) = body
        .get("stop_sequences")
        .and_then(Value::as_array)
        .filter(|s| !s.is_empty())
    {
        config.insert("stopSequences".into(), Value::Array(stops.clone()));
    }
    match body.pointer("/thinking/type").and_then(Value::as_str) {
        Some("enabled") => {
            let mut thinking = json!({"includeThoughts": true});
            if let Some(budget) = body
                .pointer("/thinking/budget_tokens")
                .and_then(Value::as_u64)
            {
                thinking["thinkingBudget"] = budget.into();
            }
            config.insert("thinkingConfig".into(), thinking);
        }
        Some("adaptive") => {
            config.insert("thinkingConfig".into(), json!({"includeThoughts": true}));
        }
        // disabled 或没写：用模型默认（Gemini 3 关不掉思考，只是不要回思考摘要）
        _ => {}
    }
    config
}

/// Anthropic Messages 请求体 → Gemini GenerateContentRequest（模型名放在 URL 里，不在请求体）。
pub fn request_to_gemini(body: &Value, model: &str) -> ProxyResult<Value> {
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .filter(|m| !m.is_empty())
        .ok_or_else(|| bad(tr!("messages 不能为空", "messages must not be empty")))?;
    let needs_signature = requires_signatures(model);
    // tool_result 要靠 tool_use 的 ID 找回函数名
    let names: HashMap<String, String> = messages
        .iter()
        .flat_map(|m| blocks(m.get("content")))
        .filter(|b| block_type(b) == Some("tool_use"))
        .filter_map(|b| {
            Some((
                b.get("id")?.as_str()?.to_owned(),
                b.get("name")?.as_str()?.to_owned(),
            ))
        })
        .collect();

    let mut contents = Vec::new();
    for msg in messages {
        let (role, parts) = match msg.get("role").and_then(Value::as_str) {
            Some("user") => ("user", user_parts(msg.get("content"), &names)?),
            Some("assistant") => (
                "model",
                assistant_parts(msg.get("content"), needs_signature),
            ),
            other => {
                return Err(bad(tr!(
                    "不支持的消息角色：{other:?}",
                    "Unsupported message role: {other:?}"
                )));
            }
        };
        if !parts.is_empty() {
            contents.push(json!({"role": role, "parts": parts}));
        }
    }
    if contents.is_empty() {
        return Err(bad(tr!(
            "messages 里没有可发送的内容",
            "messages has nothing to send"
        )));
    }

    let mut out = Map::new();
    out.insert("contents".into(), contents.into());
    let system = system_parts(body.get("system"));
    if !system.is_empty() {
        out.insert("systemInstruction".into(), json!({"parts": system}));
    }
    let tools = convert_tools(body);
    if !tools.is_empty() {
        out.insert("tools".into(), tools.into());
        if let Some(config) = tool_config(body) {
            out.insert("toolConfig".into(), config);
        }
    }
    let generation = generation_config(body);
    if !generation.is_empty() {
        out.insert("generationConfig".into(), generation.into());
    }
    Ok(Value::Object(out))
}

/// count_tokens：Vertex 的 CountTokensRequest 只认 contents / systemInstruction / tools。
pub fn count_tokens_request(body: &Value, model: &str) -> ProxyResult<Value> {
    let full = request_to_gemini(body, model)?;
    let mut out = Map::new();
    for key in ["contents", "systemInstruction", "tools"] {
        if let Some(v) = full.get(key) {
            out.insert(key.into(), v.clone());
        }
    }
    Ok(Value::Object(out))
}

pub fn count_tokens_response(resp: &Value) -> Value {
    json!({"input_tokens": resp.get("totalTokens").and_then(Value::as_u64).unwrap_or(0)})
}

// ---------------------------------------------------------------- 响应

pub fn stop_reason(finish: &str) -> &'static str {
    match finish {
        "MAX_TOKENS" => "max_tokens",
        "SAFETY"
        | "RECITATION"
        | "BLOCKLIST"
        | "PROHIBITED_CONTENT"
        | "SPII"
        | "MODEL_ARMOR"
        | "IMAGE_SAFETY"
        | "IMAGE_PROHIBITED_CONTENT"
        | "IMAGE_RECITATION" => "refusal",
        // STOP、MALFORMED_FUNCTION_CALL、OTHER 及未知值
        _ => "end_turn",
    }
}

/// Anthropic 的 input_tokens 不含缓存命中的部分，命中的单列 cache_read_input_tokens。
fn anthropic_usage(meta: Option<&Value>) -> Value {
    let n = |k: &str| {
        meta.and_then(|m| m.get(k))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    let prompt = n("promptTokenCount") + n("toolUsePromptTokenCount");
    let cached = n("cachedContentTokenCount").min(prompt);
    let mut usage = json!({
        "input_tokens": prompt - cached,
        "output_tokens": n("candidatesTokenCount") + n("thoughtsTokenCount"),
    });
    if cached > 0 {
        usage["cache_read_input_tokens"] = cached.into();
    }
    usage
}

fn first_parts(resp: &Value) -> &[Value] {
    resp.pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn signature_block(sig: &str) -> Value {
    json!({"type": "thinking", "thinking": "", "signature": format!("{SIGNATURE_PREFIX}{sig}")})
}

fn tool_use_block(call: &Value) -> Value {
    json!({
        "type": "tool_use",
        "id": new_tool_id(),
        "name": call.get("name").cloned().unwrap_or_default(),
        "input": call.get("args").filter(|a| a.is_object()).cloned().unwrap_or_else(|| json!({})),
    })
}

fn is_thought(part: &Value) -> bool {
    part.get("thought").and_then(Value::as_bool) == Some(true)
}

/// 非流式：Gemini GenerateContentResponse → Anthropic Message。只取第一个候选。
pub fn response_to_anthropic(resp: &Value, model: &str) -> Value {
    let mut content: Vec<Value> = Vec::new();
    let mut used_tools = false;
    for part in first_parts(resp) {
        if let Some(call) = part.get("functionCall") {
            if let Some(sig) = part.get("thoughtSignature").and_then(Value::as_str) {
                content.push(signature_block(sig));
            }
            content.push(tool_use_block(call));
            used_tools = true;
        } else if let Some(t) = part
            .get("text")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
        {
            let (kind, key) = if is_thought(part) {
                ("thinking", "thinking")
            } else {
                ("text", "text")
            };
            // 连续的同类片段合成一块（签名块不合并）
            let mergeable = content.last().is_some_and(|b| {
                block_type(b) == Some(kind)
                    && b.get("signature")
                        .and_then(Value::as_str)
                        .is_none_or(str::is_empty)
            });
            if mergeable {
                let last = content.last_mut().expect("刚检查过");
                let joined = format!("{}{t}", last[key].as_str().unwrap_or(""));
                last[key] = joined.into();
            } else if kind == "thinking" {
                content.push(json!({"type": "thinking", "thinking": t, "signature": ""}));
            } else {
                content.push(json!({"type": "text", "text": t}));
            }
        }
    }
    let finish = resp
        .pointer("/candidates/0/finishReason")
        .and_then(Value::as_str);
    let stop = if used_tools {
        "tool_use"
    } else if finish.is_none() && resp.pointer("/promptFeedback/blockReason").is_some() {
        "refusal"
    } else {
        stop_reason(finish.unwrap_or("STOP"))
    };
    json!({
        "id": resp.get("responseId").and_then(Value::as_str).map(|id| format!("msg_{id}")).unwrap_or_else(new_message_id),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content,
        "stop_reason": stop,
        "stop_sequence": null,
        "usage": anthropic_usage(resp.get("usageMetadata")),
    })
}

// ---------------------------------------------------------------- 流式

/// Anthropic 流式的一帧：`event: <type>` + `data: …` + 空行。
fn frame(event: &Value) -> Bytes {
    let name = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    format!("event: {name}\ndata: {event}\n\n").into()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Open {
    Text,
    Thinking,
}

/// Gemini 流（`alt=sse`，每个事件是一段 GenerateContentResponse）→ Anthropic 流式事件：
/// message_start → 各内容块的 content_block_start / delta / stop → message_delta → message_stop。
/// 文本与思考按片段增量发出；函数调用整段到达，转成一个 tool_use 块（参数一次给完）。
pub struct StreamTranslator {
    model: String,
    started: bool,
    open: Option<Open>,
    next_index: usize,
    used_tools: bool,
    usage: Option<Value>,
    pub finished: bool,
}

impl StreamTranslator {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_owned(),
            started: false,
            open: None,
            next_index: 0,
            used_tools: false,
            usage: None,
            finished: false,
        }
    }

    fn start_block(&mut self, block: Value, out: &mut Vec<Value>) -> usize {
        let index = self.next_index;
        self.next_index += 1;
        out.push(json!({"type": "content_block_start", "index": index, "content_block": block}));
        index
    }

    fn close(&mut self, out: &mut Vec<Value>) {
        if self.open.take().is_some() {
            out.push(json!({"type": "content_block_stop", "index": self.next_index - 1}));
        }
    }

    /// 文本或思考片段：类型变了就换一个块。
    fn text(&mut self, kind: Open, t: &str, out: &mut Vec<Value>) {
        if self.open != Some(kind) {
            self.close(out);
            let block = match kind {
                Open::Text => json!({"type": "text", "text": ""}),
                Open::Thinking => json!({"type": "thinking", "thinking": ""}),
            };
            self.start_block(block, out);
            self.open = Some(kind);
        }
        let delta = match kind {
            Open::Text => json!({"type": "text_delta", "text": t}),
            Open::Thinking => json!({"type": "thinking_delta", "thinking": t}),
        };
        out.push(
            json!({"type": "content_block_delta", "index": self.next_index - 1, "delta": delta}),
        );
    }

    /// 一个完整的块：开始、（可选）一段增量、结束。
    fn whole_block(&mut self, block: Value, delta: Option<Value>, out: &mut Vec<Value>) {
        self.close(out);
        let index = self.start_block(block, out);
        if let Some(delta) = delta {
            out.push(json!({"type": "content_block_delta", "index": index, "delta": delta}));
        }
        out.push(json!({"type": "content_block_stop", "index": index}));
    }

    pub fn handle(&mut self, data: &Value) -> Vec<Value> {
        let mut out = Vec::new();
        if let Some(meta) = data.get("usageMetadata") {
            self.usage = Some(meta.clone());
        }
        if !self.started {
            self.started = true;
            let mut usage = anthropic_usage(self.usage.as_ref());
            usage["output_tokens"] = 0.into();
            out.push(json!({"type": "message_start", "message": {
                "id": data.get("responseId").and_then(Value::as_str).map(|id| format!("msg_{id}")).unwrap_or_else(new_message_id),
                "type": "message",
                "role": "assistant",
                "model": self.model,
                "content": [],
                "stop_reason": null,
                "stop_sequence": null,
                "usage": usage,
            }}));
        }
        for part in first_parts(data) {
            if let Some(call) = part.get("functionCall") {
                if let Some(sig) = part.get("thoughtSignature").and_then(Value::as_str) {
                    self.whole_block(
                        json!({"type": "thinking", "thinking": ""}),
                        Some(json!({"type": "signature_delta", "signature": format!("{SIGNATURE_PREFIX}{sig}")})),
                        &mut out,
                    );
                }
                let mut block = tool_use_block(call);
                let args = std::mem::replace(&mut block["input"], json!({}));
                self.whole_block(
                    block,
                    Some(json!({"type": "input_json_delta", "partial_json": args.to_string()})),
                    &mut out,
                );
                self.used_tools = true;
            } else if let Some(t) = part
                .get("text")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
            {
                let kind = if is_thought(part) {
                    Open::Thinking
                } else {
                    Open::Text
                };
                self.text(kind, t, &mut out);
            }
        }
        let finish = data
            .pointer("/candidates/0/finishReason")
            .and_then(Value::as_str);
        let blocked = data.get("candidates").is_none()
            && data.pointer("/promptFeedback/blockReason").is_some();
        if finish.is_some() || blocked {
            self.close(&mut out);
            let stop = if self.used_tools {
                "tool_use"
            } else if blocked {
                "refusal"
            } else {
                stop_reason(finish.unwrap_or("STOP"))
            };
            out.push(json!({
                "type": "message_delta",
                "delta": {"stop_reason": stop, "stop_sequence": null},
                "usage": anthropic_usage(self.usage.as_ref()),
            }));
            out.push(json!({"type": "message_stop"}));
            self.finished = true;
        }
        out
    }
}

impl super::StreamTranslate for StreamTranslator {
    fn on_event(&mut self, _event: &str, data: &Value) -> Vec<Bytes> {
        if self.finished {
            return Vec::new();
        }
        self.handle(data).iter().map(frame).collect()
    }

    fn finish(&mut self) -> Option<Vec<Bytes>> {
        self.finished.then(Vec::new)
    }
}

#[cfg(test)]
mod tests;
