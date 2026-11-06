use serde_json::{Value, json};

use super::*;
use crate::translate::StreamTranslate;

fn weather_tool() -> Value {
    json!({
        "name": "get_weather",
        "description": "查天气",
        "input_schema": {
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"],
            "additionalProperties": false,
        },
        "cache_control": {"type": "ephemeral"},
    })
}

#[test]
fn system_text_images_and_generation_config() {
    let body = json!({
        "model": "gemini-3.5-flash",
        "max_tokens": 1024,
        "temperature": 0.2,
        "top_p": 0.9,
        "top_k": 40,
        "stop_sequences": ["END"],
        "system": [{"type": "text", "text": "你是助手", "cache_control": {"type": "ephemeral"}}],
        "messages": [{"role": "user", "content": [
            {"type": "text", "text": "看图"},
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"}},
            {"type": "image", "source": {"type": "url", "url": "https://x/a.webp"}},
            {"type": "document", "source": {"type": "base64", "media_type": "application/pdf", "data": "JVBE"}},
        ]}],
    });
    let out = request_to_gemini(&body, "gemini-3.5-flash").unwrap();
    assert_eq!(
        out["systemInstruction"],
        json!({"parts": [{"text": "你是助手"}]})
    );
    assert_eq!(
        out["contents"],
        json!([{"role": "user", "parts": [
            {"text": "看图"},
            {"inlineData": {"mimeType": "image/png", "data": "AAAA"}},
            {"fileData": {"mimeType": "image/webp", "fileUri": "https://x/a.webp"}},
            {"inlineData": {"mimeType": "application/pdf", "data": "JVBE"}},
        ]}])
    );
    assert_eq!(
        out["generationConfig"],
        json!({"maxOutputTokens": 1024, "temperature": 0.2, "topP": 0.9, "topK": 40, "stopSequences": ["END"]})
    );
    assert!(out.get("tools").is_none());
}

#[test]
fn string_content_and_roles() {
    let body = json!({"max_tokens": 8, "messages": [
        {"role": "user", "content": "你好"},
        {"role": "assistant", "content": "你好！"},
        {"role": "user", "content": "再见"},
    ]});
    let out = request_to_gemini(&body, "gemini-3.5-flash").unwrap();
    assert_eq!(
        out["contents"],
        json!([
            {"role": "user", "parts": [{"text": "你好"}]},
            {"role": "model", "parts": [{"text": "你好！"}]},
            {"role": "user", "parts": [{"text": "再见"}]},
        ])
    );
    let err = request_to_gemini(
        &json!({"messages": [{"role": "system", "content": "x"}]}),
        "m",
    )
    .unwrap_err();
    assert_eq!(err.status, 400);
    assert_eq!(
        request_to_gemini(&json!({"messages": []}), "m")
            .unwrap_err()
            .status,
        400
    );
}

#[test]
fn tools_and_tool_choice() {
    let body = json!({
        "max_tokens": 8,
        "tools": [weather_tool(), {"type": "web_search_20250305", "name": "web_search"}],
        "tool_choice": {"type": "tool", "name": "get_weather"},
        "messages": [{"role": "user", "content": "北京天气"}],
    });
    let out = request_to_gemini(&body, "gemini-3.5-flash").unwrap();
    assert_eq!(
        out["tools"],
        json!([{"functionDeclarations": [{
            "name": "get_weather",
            "description": "查天气",
            "parametersJsonSchema": {
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"],
                "additionalProperties": false,
            },
        }]}])
    );
    assert_eq!(
        out["toolConfig"],
        json!({"functionCallingConfig": {"mode": "ANY", "allowedFunctionNames": ["get_weather"]}})
    );
    for (choice, mode) in [("auto", "AUTO"), ("any", "ANY"), ("none", "NONE")] {
        let mut b = body.clone();
        b["tool_choice"] = json!({"type": choice});
        let out = request_to_gemini(&b, "m").unwrap();
        assert_eq!(out["toolConfig"]["functionCallingConfig"]["mode"], mode);
    }
}

#[test]
fn tool_round_trip_with_signatures() {
    let history = |sig: Option<&str>| {
        let mut assistant = vec![json!({"type": "text", "text": "我查一下"})];
        if let Some(sig) = sig {
            assistant.push(json!({"type": "thinking", "thinking": "", "signature": sig}));
        }
        assistant.push(json!({"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"city": "北京"}}));
        assistant.push(json!({"type": "tool_use", "id": "toolu_2", "name": "get_weather", "input": {"city": "上海"}}));
        json!({"max_tokens": 8, "tools": [weather_tool()], "messages": [
            {"role": "user", "content": "北京和上海天气"},
            {"role": "assistant", "content": assistant},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": "{\"temp\": 20}"},
                {"type": "tool_result", "tool_use_id": "toolu_2", "content": [
                    {"type": "text", "text": "下雨"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "QUJD"}},
                ], "is_error": true},
            ]},
        ]})
    };

    // 我们发出的签名原样挂回第一个函数调用
    let out = request_to_gemini(&history(Some("vsg1:SIG")), "gemini-3.5-flash").unwrap();
    let model_parts = &out["contents"][1]["parts"];
    assert_eq!(model_parts[0], json!({"text": "我查一下"}));
    assert_eq!(
        model_parts[1],
        json!({"functionCall": {"name": "get_weather", "args": {"city": "北京"}}, "thoughtSignature": "SIG"})
    );
    assert!(model_parts[2].get("thoughtSignature").is_none());
    assert_eq!(
        out["contents"][2]["parts"],
        json!([
            {"functionResponse": {"name": "get_weather", "response": {"output": {"temp": 20}}}},
            {"functionResponse": {"name": "get_weather", "response": {"error": "下雨"}}},
            {"inlineData": {"mimeType": "image/png", "data": "QUJD"}},
        ])
    );

    // 签名丢了（或是 Claude 自己的签名）：Gemini 3 补占位值，更早的模型不补
    for sig in [None, Some("claude-native-signature")] {
        let out = request_to_gemini(&history(sig), "gemini-3.5-flash").unwrap();
        assert_eq!(
            out["contents"][1]["parts"][1]["thoughtSignature"],
            SKIP_SIGNATURE
        );
        let out = request_to_gemini(&history(sig), "gemini-2.5-flash").unwrap();
        assert!(
            out["contents"][1]["parts"][1]
                .get("thoughtSignature")
                .is_none()
        );
    }

    // tool_result 对不上 tool_use：400
    let mut bad_history = history(None);
    bad_history["messages"][2]["content"][0]["tool_use_id"] = "toolu_missing".into();
    let err = request_to_gemini(&bad_history, "gemini-3.5-flash").unwrap_err();
    assert_eq!(err.status, 400);
    assert!(err.message.contains("toolu_missing"), "{}", err.message);
}

#[test]
fn thinking_config() {
    let base = json!({"max_tokens": 4096, "messages": [{"role": "user", "content": "x"}]});
    let with = |thinking: Value| {
        let mut b = base.clone();
        b["thinking"] = thinking;
        request_to_gemini(&b, "m").unwrap()["generationConfig"]["thinkingConfig"].clone()
    };
    assert_eq!(
        with(json!({"type": "enabled", "budget_tokens": 2048})),
        json!({"includeThoughts": true, "thinkingBudget": 2048})
    );
    assert_eq!(
        with(json!({"type": "adaptive"})),
        json!({"includeThoughts": true})
    );
    assert!(with(json!({"type": "disabled"})).is_null());
}

#[test]
fn count_tokens_keeps_only_supported_fields() {
    let body = json!({
        "max_tokens": 8,
        "system": "s",
        "tools": [weather_tool()],
        "tool_choice": {"type": "auto"},
        "messages": [{"role": "user", "content": "x"}],
    });
    let out = count_tokens_request(&body, "m").unwrap();
    let keys: Vec<&String> = out.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["contents", "systemInstruction", "tools"]);
    assert_eq!(
        count_tokens_response(&json!({"totalTokens": 31, "totalBillableCharacters": 90})),
        json!({"input_tokens": 31})
    );
}

fn gemini_response() -> Value {
    json!({
        "responseId": "r1",
        "candidates": [{
            "content": {"role": "model", "parts": [
                {"text": "先想想", "thought": true},
                {"text": "再想想", "thought": true},
                {"text": "北京"},
                {"text": "晴"},
                {"functionCall": {"name": "get_weather", "args": {"city": "北京"}}, "thoughtSignature": "SIG"},
            ]},
            "finishReason": "STOP",
        }],
        "usageMetadata": {"promptTokenCount": 100, "cachedContentTokenCount": 40, "candidatesTokenCount": 7, "thoughtsTokenCount": 5},
    })
}

#[test]
fn response_blocks_stop_reason_and_usage() {
    let out = response_to_anthropic(&gemini_response(), "gemini-3.5-flash");
    assert_eq!(out["id"], "msg_r1");
    assert_eq!(out["type"], "message");
    assert_eq!(out["model"], "gemini-3.5-flash");
    assert_eq!(out["stop_reason"], "tool_use");
    let content = out["content"].as_array().unwrap();
    assert_eq!(
        content[0],
        json!({"type": "thinking", "thinking": "先想想再想想", "signature": ""})
    );
    assert_eq!(content[1], json!({"type": "text", "text": "北京晴"}));
    assert_eq!(
        content[2],
        json!({"type": "thinking", "thinking": "", "signature": "vsg1:SIG"})
    );
    assert_eq!(content[3]["type"], "tool_use");
    assert!(content[3]["id"].as_str().unwrap().starts_with("toolu_"));
    assert_eq!(content[3]["input"], json!({"city": "北京"}));
    assert_eq!(
        out["usage"],
        json!({"input_tokens": 60, "output_tokens": 12, "cache_read_input_tokens": 40})
    );

    // 往返：把这条回复原样放回历史，签名能挂回函数调用
    let history = json!({"max_tokens": 8, "messages": [
        {"role": "user", "content": "天气"},
        {"role": "assistant", "content": out["content"]},
        {"role": "user", "content": [{"type": "tool_result", "tool_use_id": content[3]["id"], "content": "晴"}]},
    ]});
    let req = request_to_gemini(&history, "gemini-3.5-flash").unwrap();
    assert_eq!(req["contents"][1]["parts"][1]["thoughtSignature"], "SIG");
    assert_eq!(
        req["contents"][2]["parts"][0]["functionResponse"]["name"],
        "get_weather"
    );
}

#[test]
fn response_stop_reasons() {
    let with = |finish: &str| {
        let r = json!({"candidates": [{"content": {"parts": [{"text": "x"}]}, "finishReason": finish}]});
        response_to_anthropic(&r, "m")["stop_reason"].clone()
    };
    assert_eq!(with("STOP"), "end_turn");
    assert_eq!(with("MAX_TOKENS"), "max_tokens");
    assert_eq!(with("SAFETY"), "refusal");
    let blocked = json!({"promptFeedback": {"blockReason": "SAFETY"}});
    let out = response_to_anthropic(&blocked, "m");
    assert_eq!(out["stop_reason"], "refusal");
    assert_eq!(out["content"], json!([]));
}

/// 把翻译器吐出的帧解析回 (event, data) 方便断言。
fn events(t: &mut StreamTranslator, chunks: &[Value]) -> Vec<(String, Value)> {
    chunks
        .iter()
        .flat_map(|c| t.on_event("message", c))
        .map(|frame| {
            let text = String::from_utf8(frame.to_vec()).unwrap();
            let (event, data) = text
                .trim_end()
                .split_once('\n')
                .expect("event 行 + data 行");
            (
                event.strip_prefix("event: ").unwrap().to_owned(),
                serde_json::from_str(data.strip_prefix("data: ").unwrap()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn stream_event_sequence() {
    let chunks = [
        json!({"responseId": "r9", "candidates": [{"content": {"parts": [{"text": "想", "thought": true}]}}], "usageMetadata": {"promptTokenCount": 50}}),
        json!({"candidates": [{"content": {"parts": [{"text": "你"}]}}]}),
        json!({"candidates": [{"content": {"parts": [{"text": "好"}]}}]}),
        json!({"candidates": [{"content": {"parts": [
            {"functionCall": {"name": "get_weather", "args": {"city": "北京"}}, "thoughtSignature": "SIG"},
        ]}, "finishReason": "STOP"}], "usageMetadata": {"promptTokenCount": 50, "candidatesTokenCount": 9}}),
    ];
    let mut t = StreamTranslator::new("gemini-3.5-flash");
    let got = events(&mut t, &chunks);
    let names: Vec<&str> = got.iter().map(|(e, _)| e.as_str()).collect();
    assert_eq!(
        names,
        [
            "message_start",
            "content_block_start",
            "content_block_delta",
            "content_block_stop", // 思考
            "content_block_start",
            "content_block_delta",
            "content_block_delta",
            "content_block_stop", // 文本
            "content_block_start",
            "content_block_delta",
            "content_block_stop", // 签名
            "content_block_start",
            "content_block_delta",
            "content_block_stop", // tool_use
            "message_delta",
            "message_stop",
        ]
    );
    // 每帧的 event 名与 data.type 一致
    assert!(got.iter().all(|(e, d)| d["type"] == e.as_str()));
    let start = &got[0].1["message"];
    assert_eq!(start["id"], "msg_r9");
    assert_eq!(start["model"], "gemini-3.5-flash");
    assert_eq!(
        start["usage"],
        json!({"input_tokens": 50, "output_tokens": 0})
    );
    assert_eq!(
        got[2].1["delta"],
        json!({"type": "thinking_delta", "thinking": "想"})
    );
    assert_eq!(
        got[4].1["content_block"],
        json!({"type": "text", "text": ""})
    );
    assert_eq!(
        got[6].1["delta"],
        json!({"type": "text_delta", "text": "好"})
    );
    assert_eq!(
        got[9].1["delta"],
        json!({"type": "signature_delta", "signature": "vsg1:SIG"})
    );
    let tool = &got[11].1;
    assert_eq!(tool["index"], 3);
    assert_eq!(tool["content_block"]["type"], "tool_use");
    assert_eq!(tool["content_block"]["input"], json!({}));
    assert_eq!(
        got[12].1["delta"],
        json!({"type": "input_json_delta", "partial_json": "{\"city\":\"北京\"}"})
    );
    let delta = &got[14].1;
    assert_eq!(delta["delta"]["stop_reason"], "tool_use");
    assert_eq!(
        delta["usage"],
        json!({"input_tokens": 50, "output_tokens": 9})
    );
    assert_eq!(t.finish(), Some(vec![]));
}

#[test]
fn stream_without_finish_reports_incomplete() {
    let mut t = StreamTranslator::new("m");
    let got = events(
        &mut t,
        &[json!({"candidates": [{"content": {"parts": [{"text": "半"}]}}]})],
    );
    assert_eq!(got.last().unwrap().0, "content_block_delta");
    assert_eq!(t.finish(), None);
}

#[test]
fn stream_blocked_prompt() {
    let mut t = StreamTranslator::new("m");
    let got = events(
        &mut t,
        &[json!({"promptFeedback": {"blockReason": "SAFETY"}})],
    );
    let names: Vec<&str> = got.iter().map(|(e, _)| e.as_str()).collect();
    assert_eq!(names, ["message_start", "message_delta", "message_stop"]);
    assert_eq!(got[1].1["delta"]["stop_reason"], "refusal");
    assert!(t.finish().is_some());
}
