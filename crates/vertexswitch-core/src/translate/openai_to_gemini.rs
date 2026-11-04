//! OpenAI Chat Completions ⇄ Gemini 原生 generateContent 转换（SPEC §13）：纯函数 + 流式状态机。
//!
//! Vertex 的 OpenAI 兼容接口不接受服务档位头，需要档位时就把请求转成原生格式发出。
//! 字段对应照 Vertex 自己的兼容接口文档，客户端换到这条路上时行为尽量不变：
//! - `reasoning_effort` low / medium / high → `thinkingBudget` 1024 / 8192 / 24576
//! - 工具参数本来就是 JSON Schema，放进 `parametersJsonSchema`
//! - 思考签名放在 `tool_calls[].extra_content.google.thought_signature` 里往返；Gemini 3 起
//!   函数调用缺签名会被拒（400），客户端没带回来时按文档填 `skip_thought_signature_validator`
//! - `extra_body.google` 里的 `thinking_config` / `safety_settings` / `cached_content` 照转
//!
//! 参考：
//! - <https://docs.cloud.google.com/vertex-ai/generative-ai/docs/migrate/openai/overview>
//! - <https://docs.cloud.google.com/vertex-ai/generative-ai/docs/thought-signatures>
//! - v1 discovery 文档里的 GenerateContentRequest / Part / FunctionDeclaration / Candidate

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

use super::openai_to_anthropic::{DATA_URL, bad, now_secs, text_of};
use crate::errors::ProxyResult;

/// 文档给出的占位签名：历史里的函数调用不是这个 API 生成的（或签名丢了）时使用
pub const SKIP_SIGNATURE: &str = "skip_thought_signature_validator";

/// 与 Vertex OpenAI 兼容接口相同的映射
const REASONING_BUDGETS: &[(&str, u64)] = &[("low", 1024), ("medium", 8192), ("high", 24576)];

/// Gemini 3 起函数调用必须带回思考签名；更早的模型不强制，也不填占位值。
pub fn requires_signatures(model: &str) -> bool {
    model
        .strip_prefix("gemini-")
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|major| major.parse::<u32>().ok())
        .is_some_and(|major| major >= 3)
}

/// 客户端会按 id 把工具结果对回调用；Gemini 在 v1 上不一定给 id，就自己生成一个不重复的。
fn new_call_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("call_{:x}{n:x}", nanos & 0xffff_ffff_ffff)
}

// ---------------------------------------------------------------- 请求

fn guess_mime(url: &str) -> Option<&'static str> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "pdf" => "application/pdf",
        "mp3" => "audio/mp3",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "txt" => "text/plain",
        _ => return None,
    })
}

/// data URL 内嵌为 inlineData；其他地址（https、gs）按 fileData 交给 Vertex 去取。
fn media_part(url: &str, fallback_mime: &str) -> Value {
    match DATA_URL.captures(url) {
        Some(caps) => json!({"inlineData": {"mimeType": &caps["media"], "data": &caps["data"]}}),
        None => json!({"fileData": {
            "mimeType": guess_mime(url).unwrap_or(fallback_mime),
            "fileUri": url,
        }}),
    }
}

fn content_part(part: &Value) -> ProxyResult<Option<Value>> {
    let field = |k: &str| part.get(k);
    Ok(match part.get("type").and_then(Value::as_str) {
        Some("text") => field("text")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(|t| json!({"text": t})),
        Some("image_url") => {
            let image = field("image_url");
            let url = image
                .and_then(|i| i.get("url"))
                .or(image)
                .and_then(Value::as_str)
                .ok_or_else(|| bad("image_url.url 缺失"))?;
            Some(media_part(url, "image/jpeg"))
        }
        Some("input_audio") => {
            let audio = field("input_audio").ok_or_else(|| bad("input_audio 缺失"))?;
            let data = audio
                .get("data")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("input_audio.data 缺失"))?;
            let format = audio.get("format").and_then(Value::as_str).unwrap_or("wav");
            Some(json!({"inlineData": {"mimeType": format!("audio/{format}"), "data": data}}))
        }
        Some("file") => {
            let data = field("file")
                .and_then(|f| f.get("file_data"))
                .and_then(Value::as_str)
                .ok_or_else(|| bad("file 只支持 file_data（data URL），不支持 file_id"))?;
            Some(media_part(data, "application/pdf"))
        }
        other => return Err(bad(format!("不支持的消息内容类型：{other:?}"))),
    })
}

fn user_parts(content: Option<&Value>) -> ProxyResult<Vec<Value>> {
    match content {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::String(s)) if s.is_empty() => Ok(vec![]),
        Some(Value::String(s)) => Ok(vec![json!({"text": s})]),
        Some(Value::Array(parts)) => Ok(parts
            .iter()
            .map(content_part)
            .collect::<ProxyResult<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect()),
        Some(_) => Err(bad("user 消息的 content 必须是字符串或数组")),
    }
}

fn parse_arguments(raw: Option<&Value>) -> ProxyResult<Value> {
    match raw {
        None | Some(Value::Null) => Ok(json!({})),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(json!({})),
        Some(Value::String(s)) => serde_json::from_str(s)
            .map_err(|e| bad(format!("tool_calls 的 arguments 不是合法 JSON：{e}"))),
        Some(other) => Ok(other.clone()),
    }
}

/// 助手消息：文本 + 函数调用。记下每个调用 id 对应的函数名，tool 消息要靠它找回名字。
fn assistant_parts(
    msg: &Value,
    needs_signature: bool,
    names: &mut HashMap<String, String>,
) -> ProxyResult<Vec<Value>> {
    let mut parts = Vec::new();
    let text = text_of(msg.get("content"))?;
    if !text.is_empty() {
        parts.push(json!({"text": text}));
    }
    let mut calls: Vec<Value> = msg
        .get("tool_calls")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // 已弃用的单个 function_call
    if let Some(call) = msg.get("function_call").filter(|c| c.is_object()) {
        calls.push(json!({"function": call}));
    }
    let first_call = parts.len();
    let mut signed = false;
    for call in &calls {
        let function = call.get("function").cloned().unwrap_or_default();
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("tool_calls[].function.name 缺失"))?;
        if let Some(id) = call.get("id").and_then(Value::as_str) {
            names.insert(id.to_owned(), name.to_owned());
        }
        let mut part = json!({"functionCall": {
            "name": name,
            "args": parse_arguments(function.get("arguments"))?,
        }});
        if let Some(sig) = call
            .pointer("/extra_content/google/thought_signature")
            .and_then(Value::as_str)
        {
            part["thoughtSignature"] = sig.into();
            signed = true;
        }
        parts.push(part);
    }
    // 只有一组并行调用里的第一个带签名；客户端把签名丢了时给第一个补占位值
    if needs_signature && !signed && parts.len() > first_call {
        parts[first_call]["thoughtSignature"] = SKIP_SIGNATURE.into();
    }
    Ok(parts)
}

/// 工具结果：能解析成 JSON 就按 JSON 交回，否则按字符串；统一放在 `output` 键下（文档约定）。
fn function_response(name: &str, content: Option<&Value>) -> ProxyResult<Value> {
    let text = text_of(content)?;
    let output = serde_json::from_str::<Value>(&text)
        .ok()
        .filter(|v| !v.is_null())
        .unwrap_or(Value::String(text));
    Ok(json!({"functionResponse": {"name": name, "response": {"output": output}}}))
}

fn convert_tools(body: &Value) -> ProxyResult<Vec<Value>> {
    let mut declarations = Vec::new();
    let mut extra = Vec::new();
    let functions = body
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|tool| match tool.get("type").and_then(Value::as_str) {
            Some("function") => Ok(tool.get("function").cloned().unwrap_or_default()),
            other => Err(bad(format!("不支持的工具类型：{other:?}"))),
        })
        // 已弃用的 functions 字段
        .chain(
            body.get("functions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|f| Ok(f.clone())),
        );
    for function in functions {
        let function = function?;
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("tools[].function.name 缺失"))?;
        let mut decl = Map::new();
        decl.insert("name".into(), name.into());
        if let Some(desc) = function
            .get("description")
            .and_then(Value::as_str)
            .filter(|d| !d.is_empty())
        {
            decl.insert("description".into(), desc.into());
        }
        if let Some(params) = function.get("parameters").filter(|p| !p.is_null()) {
            decl.insert("parametersJsonSchema".into(), params.clone());
        }
        declarations.push(Value::Object(decl));
    }
    if !declarations.is_empty() {
        extra.push(json!({"functionDeclarations": declarations}));
    }
    if body.get("web_search_options").is_some_and(|w| !w.is_null()) {
        extra.push(json!({"googleSearch": {}}));
    }
    Ok(extra)
}

fn tool_config(body: &Value) -> ProxyResult<Option<Value>> {
    let choice = body
        .get("tool_choice")
        .filter(|c| !c.is_null())
        .or_else(|| body.get("function_call").filter(|c| !c.is_null()));
    let config = match choice {
        None => return Ok(None),
        Some(Value::String(s)) => match s.as_str() {
            "auto" => json!({"mode": "AUTO"}),
            "none" => json!({"mode": "NONE"}),
            "required" => json!({"mode": "ANY"}),
            "validated" => json!({"mode": "VALIDATED"}),
            other => return Err(bad(format!("不支持的 tool_choice：{other:?}"))),
        },
        Some(obj) => {
            let name = obj
                .pointer("/function/name")
                .or_else(|| obj.get("name"))
                .and_then(Value::as_str)
                .ok_or_else(|| bad(format!("不支持的 tool_choice：{obj}")))?;
            json!({"mode": "ANY", "allowedFunctionNames": [name]})
        }
    };
    Ok(Some(json!({"functionCallingConfig": config})))
}

/// `extra_body.google`（OpenAI SDK 的 extra_body 原样并入请求体，常见两种写法都认）
fn google_extras(body: &Value) -> Option<&Map<String, Value>> {
    body.pointer("/extra_body/google")
        .or_else(|| body.get("google"))
        .and_then(Value::as_object)
}

/// 顶层键从 snake_case 换成 Vertex JSON 用的 camelCase（内层原样）
fn camel_keys(value: &Value) -> Value {
    let Value::Object(map) = value else {
        return value.clone();
    };
    map.iter()
        .map(|(k, v)| {
            let mut key = String::with_capacity(k.len());
            let mut upper = false;
            for c in k.chars() {
                if c == '_' {
                    upper = true;
                } else if upper {
                    key.extend(c.to_uppercase());
                    upper = false;
                } else {
                    key.push(c);
                }
            }
            (key, v.clone())
        })
        .collect::<Map<_, _>>()
        .into()
}

fn generation_config(body: &Value) -> ProxyResult<Map<String, Value>> {
    let mut config = Map::new();
    for (from, to) in [
        ("temperature", "temperature"),
        ("top_p", "topP"),
        ("n", "candidateCount"),
        ("presence_penalty", "presencePenalty"),
        ("frequency_penalty", "frequencyPenalty"),
        ("seed", "seed"),
    ] {
        if let Some(v) = body.get(from).filter(|v| !v.is_null()) {
            config.insert(to.into(), v.clone());
        }
    }
    if let Some(max) = ["max_completion_tokens", "max_tokens"]
        .iter()
        .find_map(|k| body.get(*k).and_then(Value::as_u64).filter(|v| *v > 0))
    {
        config.insert("maxOutputTokens".into(), max.into());
    }
    match body.get("stop") {
        Some(Value::String(s)) if !s.is_empty() => {
            config.insert("stopSequences".into(), json!([s]));
        }
        Some(Value::Array(items)) if !items.is_empty() => {
            config.insert("stopSequences".into(), Value::Array(items.clone()));
        }
        _ => {}
    }
    if let Some(format) = body.get("response_format").filter(|f| !f.is_null()) {
        match format.get("type").and_then(Value::as_str) {
            Some("json_object") => {
                config.insert("responseMimeType".into(), "application/json".into());
            }
            Some("json_schema") => {
                config.insert("responseMimeType".into(), "application/json".into());
                if let Some(schema) = format.pointer("/json_schema/schema") {
                    config.insert("responseJsonSchema".into(), schema.clone());
                }
            }
            Some("text") | None => {}
            Some(other) => return Err(bad(format!("不支持的 response_format：{other:?}"))),
        }
    }
    let effort = body.get("reasoning_effort").and_then(Value::as_str);
    let thinking = google_extras(body).and_then(|g| g.get("thinking_config"));
    if effort.is_some() && thinking.is_some() {
        return Err(bad(
            "reasoning_effort 与 extra_body.google.thinking_config 只能二选一",
        ));
    }
    if let Some(budget) = effort.and_then(|e| {
        REASONING_BUDGETS
            .iter()
            .find(|(name, _)| *name == e)
            .map(|(_, b)| *b)
    }) {
        config.insert("thinkingConfig".into(), json!({"thinkingBudget": budget}));
    }
    if let Some(thinking) = thinking {
        config.insert("thinkingConfig".into(), camel_keys(thinking));
    }
    Ok(config)
}

/// OpenAI 请求体 → Gemini GenerateContentRequest（模型名放在 URL 里，不在请求体）。
pub fn request_to_gemini(body: &Value, model: &str) -> ProxyResult<Value> {
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .filter(|m| !m.is_empty())
        .ok_or_else(|| bad("messages 不能为空"))?;
    let needs_signature = requires_signatures(model);

    let mut system: Vec<Value> = Vec::new();
    let mut contents: Vec<Value> = Vec::new();
    let mut names: HashMap<String, String> = HashMap::new();
    // 连续的工具结果合进同一条 user 内容（文档：先给出全部调用，再给出全部结果）
    let mut pending_results: Vec<Value> = Vec::new();
    let flush = |contents: &mut Vec<Value>, pending: &mut Vec<Value>| {
        if !pending.is_empty() {
            contents.push(json!({"role": "user", "parts": std::mem::take(pending)}));
        }
    };

    for msg in messages {
        if !msg.is_object() {
            return Err(bad("messages[] 必须是对象"));
        }
        let role = msg.get("role").and_then(Value::as_str);
        if let Some("tool" | "function") = role {
            let name = msg
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    msg.get("tool_call_id")
                        .and_then(Value::as_str)
                        .and_then(|id| names.get(id).cloned())
                })
                .ok_or_else(|| bad("tool 消息找不到对应的工具调用（tool_call_id 不匹配）"))?;
            pending_results.push(function_response(&name, msg.get("content"))?);
            continue;
        }
        flush(&mut contents, &mut pending_results);
        match role {
            Some("system" | "developer") => {
                let text = text_of(msg.get("content"))?;
                if !text.is_empty() {
                    system.push(json!({"text": text}));
                }
            }
            Some("user") => {
                let parts = user_parts(msg.get("content"))?;
                if !parts.is_empty() {
                    contents.push(json!({"role": "user", "parts": parts}));
                }
            }
            Some("assistant") => {
                let parts = assistant_parts(msg, needs_signature, &mut names)?;
                if !parts.is_empty() {
                    contents.push(json!({"role": "model", "parts": parts}));
                }
            }
            other => return Err(bad(format!("不支持的消息角色：{other:?}"))),
        }
    }
    flush(&mut contents, &mut pending_results);
    if contents.is_empty() {
        return Err(bad("messages 里没有可发送的内容"));
    }

    let mut out = Map::new();
    out.insert("contents".into(), contents.into());
    if !system.is_empty() {
        out.insert("systemInstruction".into(), json!({"parts": system}));
    }
    let tools = convert_tools(body)?;
    if !tools.is_empty() {
        out.insert("tools".into(), tools.into());
    }
    if let Some(config) = tool_config(body)? {
        out.insert("toolConfig".into(), config);
    }
    let generation = generation_config(body)?;
    if !generation.is_empty() {
        out.insert("generationConfig".into(), generation.into());
    }
    if let Some(google) = google_extras(body) {
        if let Some(safety) = google.get("safety_settings") {
            out.insert("safetySettings".into(), safety.clone());
        }
        if let Some(cached) = google.get("cached_content") {
            out.insert("cachedContent".into(), cached.clone());
        }
    }
    Ok(Value::Object(out))
}

// ---------------------------------------------------------------- 响应

pub fn finish_reason(reason: &str) -> &'static str {
    match reason {
        "MAX_TOKENS" => "length",
        "SAFETY"
        | "RECITATION"
        | "BLOCKLIST"
        | "PROHIBITED_CONTENT"
        | "SPII"
        | "MODEL_ARMOR"
        | "IMAGE_SAFETY"
        | "IMAGE_PROHIBITED_CONTENT"
        | "IMAGE_RECITATION" => "content_filter",
        // STOP、MALFORMED_FUNCTION_CALL、OTHER 及未知值
        _ => "stop",
    }
}

fn openai_usage(meta: Option<&Value>) -> Value {
    let n = |k: &str| {
        meta.and_then(|m| m.get(k))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    let prompt = n("promptTokenCount") + n("toolUsePromptTokenCount");
    let thoughts = n("thoughtsTokenCount");
    let completion = n("candidatesTokenCount") + thoughts;
    let total = match n("totalTokenCount") {
        0 => prompt + completion,
        t => t,
    };
    let mut usage = json!({
        "prompt_tokens": prompt,
        "completion_tokens": completion,
        "total_tokens": total,
    });
    if thoughts > 0 {
        usage["completion_tokens_details"] = json!({"reasoning_tokens": thoughts});
    }
    let cached = n("cachedContentTokenCount");
    if cached > 0 {
        usage["prompt_tokens_details"] = json!({"cached_tokens": cached});
    }
    usage
}

/// functionCall 部分 → OpenAI tool_call；签名放进 extra_content 供客户端原样带回。
fn tool_call(part: &Value, call: &Value) -> Value {
    let mut out = json!({
        "id": call.get("id").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(new_call_id),
        "type": "function",
        "function": {
            "name": call.get("name"),
            "arguments": call.get("args").cloned().unwrap_or_else(|| json!({})).to_string(),
        },
    });
    if let Some(sig) = part.get("thoughtSignature").and_then(Value::as_str) {
        out["extra_content"] = json!({"google": {"thought_signature": sig}});
    }
    out
}

fn parts_of(candidate: &Value) -> &[Value] {
    candidate
        .pointer("/content/parts")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn response_id(resp: &Value) -> Option<String> {
    resp.get("responseId")
        .and_then(Value::as_str)
        .map(|id| format!("chatcmpl-{id}"))
}

pub fn response_to_openai(resp: &Value, model: &str) -> Value {
    let candidates = resp
        .get("candidates")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut choices: Vec<Value> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let mut text = String::new();
            let mut reasoning = String::new();
            let mut calls = Vec::new();
            for part in parts_of(c) {
                if let Some(call) = part.get("functionCall") {
                    calls.push(tool_call(part, call));
                } else if let Some(t) = part.get("text").and_then(Value::as_str) {
                    if part.get("thought").and_then(Value::as_bool) == Some(true) {
                        reasoning.push_str(t);
                    } else {
                        text.push_str(t);
                    }
                }
            }
            let mut message = json!({
                "role": "assistant",
                "content": if text.is_empty() { Value::Null } else { text.into() },
            });
            if !reasoning.is_empty() {
                message["reasoning_content"] = reasoning.into();
            }
            let finish = if calls.is_empty() {
                c.get("finishReason")
                    .and_then(Value::as_str)
                    .map(finish_reason)
            } else {
                message["tool_calls"] = calls.into();
                Some("tool_calls")
            };
            json!({
                "index": c.get("index").and_then(Value::as_u64).unwrap_or(i as u64),
                "message": message,
                "finish_reason": finish,
            })
        })
        .collect();
    // 提示词被拦截时没有候选
    if choices.is_empty() {
        let blocked = resp.pointer("/promptFeedback/blockReason").is_some();
        choices.push(json!({
            "index": 0,
            "message": {"role": "assistant", "content": null},
            "finish_reason": if blocked { "content_filter" } else { "stop" },
        }));
    }
    json!({
        "id": response_id(resp).unwrap_or_else(|| "chatcmpl-vertexswitch".into()),
        "object": "chat.completion",
        "created": now_secs(),
        "model": model,
        "choices": choices,
        "usage": openai_usage(resp.get("usageMetadata")),
    })
}

// ---------------------------------------------------------------- 流式

/// Gemini 流（`alt=sse`，每个事件是一段 GenerateContentResponse）→ OpenAI `chat.completion.chunk`。
///
/// Gemini 的函数调用总是整段到达，转成一次带完整 arguments 的 tool_calls 增量。
pub struct StreamTranslator {
    model: String,
    include_usage: bool,
    id: Option<String>,
    created: u64,
    started: HashSet<u64>,
    tool_counts: HashMap<u64, usize>,
    usage: Option<Value>,
    pub finished: bool,
}

impl StreamTranslator {
    pub fn new(model: &str, include_usage: bool) -> Self {
        Self {
            model: model.to_owned(),
            include_usage,
            id: None,
            created: now_secs(),
            started: HashSet::new(),
            tool_counts: HashMap::new(),
            usage: None,
            finished: false,
        }
    }

    fn chunk(&self, index: u64, delta: Value, finish: Option<&str>) -> Value {
        json!({
            "id": self.id.as_deref().unwrap_or("chatcmpl-vertexswitch"),
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [{"index": index, "delta": delta, "finish_reason": finish}],
        })
    }

    /// 每个候选的第一段先发 role
    fn start(&mut self, index: u64, out: &mut Vec<Value>) {
        if self.started.insert(index) {
            out.push(self.chunk(index, json!({"role": "assistant", "content": ""}), None));
        }
    }

    pub fn handle(&mut self, data: &Value) -> Vec<Value> {
        if self.id.is_none() {
            self.id = response_id(data);
        }
        if let Some(meta) = data.get("usageMetadata") {
            self.usage = Some(meta.clone());
        }
        let mut out = Vec::new();
        let candidates = data
            .get("candidates")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if candidates.is_empty() && data.pointer("/promptFeedback/blockReason").is_some() {
            self.start(0, &mut out);
            out.push(self.chunk(0, json!({}), Some("content_filter")));
            self.finished = true;
            return out;
        }
        for (i, c) in candidates.iter().enumerate() {
            let index = c.get("index").and_then(Value::as_u64).unwrap_or(i as u64);
            self.start(index, &mut out);
            for part in parts_of(c) {
                if let Some(call) = part.get("functionCall") {
                    let n = self.tool_counts.entry(index).or_default();
                    let mut delta = tool_call(part, call);
                    delta["index"] = (*n).into();
                    *n += 1;
                    out.push(self.chunk(index, json!({"tool_calls": [delta]}), None));
                } else if let Some(t) = part
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty())
                {
                    let key = if part.get("thought").and_then(Value::as_bool) == Some(true) {
                        "reasoning_content"
                    } else {
                        "content"
                    };
                    out.push(self.chunk(index, json!({ key: t }), None));
                }
            }
            if let Some(reason) = c.get("finishReason").and_then(Value::as_str) {
                let finish = if self.tool_counts.get(&index).is_some_and(|n| *n > 0) {
                    "tool_calls"
                } else {
                    finish_reason(reason)
                };
                out.push(self.chunk(index, json!({}), Some(finish)));
                self.finished = true;
            }
        }
        out
    }

    /// 流正常结束后：客户端要了用量就补一段只有 usage 的块。
    pub fn finish(&self) -> Option<Value> {
        if !self.include_usage {
            return None;
        }
        let mut last = self.chunk(0, json!({}), None);
        last["choices"] = json!([]);
        last["usage"] = openai_usage(self.usage.as_ref());
        Some(last)
    }
}

#[cfg(test)]
mod tests;
