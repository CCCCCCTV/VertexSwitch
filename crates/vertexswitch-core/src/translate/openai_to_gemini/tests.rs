use serde_json::{Value, json};

use super::*;

const G3: &str = "gemini-3.5-flash";

fn tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "get_weather",
            "description": "查天气",
            "parameters": {"type": "object", "properties": {"city": {"type": "string"}}, "additionalProperties": false},
        },
    })
}

fn convert(body: Value) -> Value {
    request_to_gemini(&body, G3).unwrap()
}

#[test]
fn signature_rule_by_model_generation() {
    assert!(requires_signatures("gemini-3.5-flash"));
    assert!(requires_signatures("gemini-3-pro-preview"));
    assert!(requires_signatures("gemini-4-argon"));
    assert!(!requires_signatures("gemini-2.5-pro"));
    assert!(!requires_signatures("gemma-3"));
}

#[test]
fn messages_and_system() {
    let out = convert(json!({
        "messages": [
            {"role": "system", "content": "be brief"},
            {"role": "developer", "content": [{"type": "text", "text": "really"}]},
            {"role": "user", "content": "hi"},
            {"role": "assistant", "content": "hello"},
            {"role": "user", "content": [
                {"type": "text", "text": "what is this"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
                {"type": "image_url", "image_url": {"url": "https://example.com/cat.webp?x=1"}},
            ]},
        ],
    }));
    assert_eq!(
        out["systemInstruction"],
        json!({"parts": [{"text": "be brief"}, {"text": "really"}]})
    );
    assert_eq!(
        out["contents"],
        json!([
            {"role": "user", "parts": [{"text": "hi"}]},
            {"role": "model", "parts": [{"text": "hello"}]},
            {"role": "user", "parts": [
                {"text": "what is this"},
                {"inlineData": {"mimeType": "image/png", "data": "AAAA"}},
                {"fileData": {"mimeType": "image/webp", "fileUri": "https://example.com/cat.webp?x=1"}},
            ]},
        ])
    );
    assert!(out.get("generationConfig").is_none());
}

#[test]
fn generation_parameters() {
    let out = convert(json!({
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.2, "top_p": 0.9, "n": 2, "seed": 7,
        "presence_penalty": 0.5, "frequency_penalty": -0.5,
        "max_tokens": 50, "max_completion_tokens": 64,
        "stop": "END",
        "response_format": {"type": "json_schema", "json_schema": {"name": "x", "schema": {"type": "object"}}},
        "reasoning_effort": "medium",
        "user": "ignored",
    }));
    assert_eq!(
        out["generationConfig"],
        json!({
            "temperature": 0.2, "topP": 0.9, "candidateCount": 2, "seed": 7,
            "presencePenalty": 0.5, "frequencyPenalty": -0.5,
            "maxOutputTokens": 64,
            "stopSequences": ["END"],
            "responseMimeType": "application/json",
            "responseJsonSchema": {"type": "object"},
            "thinkingConfig": {"thinkingBudget": 8192},
        })
    );
}

#[test]
fn google_extras_and_effort_conflict() {
    let out = convert(json!({
        "messages": [{"role": "user", "content": "hi"}],
        "extra_body": {"google": {
            "thinking_config": {"thinking_level": "LOW", "include_thoughts": true},
            "safety_settings": [{"category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_NONE"}],
            "cached_content": "projects/p/locations/global/cachedContents/c",
        }},
    }));
    assert_eq!(
        out["generationConfig"]["thinkingConfig"],
        json!({"thinkingLevel": "LOW", "includeThoughts": true})
    );
    assert_eq!(out["safetySettings"][0]["threshold"], "BLOCK_NONE");
    assert_eq!(
        out["cachedContent"],
        "projects/p/locations/global/cachedContents/c"
    );

    let err = request_to_gemini(
        &json!({
            "messages": [{"role": "user", "content": "hi"}],
            "reasoning_effort": "high",
            "google": {"thinking_config": {"thinking_budget": 10}},
        }),
        G3,
    )
    .unwrap_err();
    assert_eq!(err.status, 400);
}

#[test]
fn tools_and_choice() {
    let out = convert(json!({
        "messages": [{"role": "user", "content": "weather?"}],
        "tools": [tool()],
        "tool_choice": {"type": "function", "function": {"name": "get_weather"}},
    }));
    assert_eq!(
        out["tools"],
        json!([{"functionDeclarations": [{
            "name": "get_weather",
            "description": "查天气",
            "parametersJsonSchema": {"type": "object", "properties": {"city": {"type": "string"}}, "additionalProperties": false},
        }]}])
    );
    assert_eq!(
        out["toolConfig"],
        json!({"functionCallingConfig": {"mode": "ANY", "allowedFunctionNames": ["get_weather"]}})
    );
    for (choice, mode) in [("auto", "AUTO"), ("none", "NONE"), ("required", "ANY")] {
        let out = convert(json!({
            "messages": [{"role": "user", "content": "x"}],
            "tools": [tool()],
            "tool_choice": choice,
        }));
        assert_eq!(out["toolConfig"]["functionCallingConfig"]["mode"], mode);
    }
    let err = request_to_gemini(
        &json!({"messages": [{"role": "user", "content": "x"}], "tools": [{"type": "code_interpreter"}]}),
        G3,
    )
    .unwrap_err();
    assert_eq!(err.status, 400);
}

fn tool_round(signature: Option<&str>, model: &str) -> Value {
    let mut first = json!({
        "id": "call_a", "type": "function",
        "function": {"name": "get_weather", "arguments": "{\"city\":\"Paris\"}"},
    });
    if let Some(sig) = signature {
        first["extra_content"] = json!({"google": {"thought_signature": sig}});
    }
    request_to_gemini(
        &json!({"messages": [
            {"role": "user", "content": "weather in Paris and Rome?"},
            {"role": "assistant", "content": null, "tool_calls": [
                first,
                {"id": "call_b", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"Rome\"}"}},
            ]},
            {"role": "tool", "tool_call_id": "call_a", "content": "{\"temp\": 20}"},
            {"role": "tool", "tool_call_id": "call_b", "content": "sunny"},
            {"role": "user", "content": "thanks"},
        ]}),
        model,
    )
    .unwrap()
}

#[test]
fn tool_history_round_trip_keeps_signature() {
    let out = tool_round(Some("sig-1"), G3);
    assert_eq!(
        out["contents"][1],
        json!({"role": "model", "parts": [
            {"functionCall": {"name": "get_weather", "args": {"city": "Paris"}}, "thoughtSignature": "sig-1"},
            {"functionCall": {"name": "get_weather", "args": {"city": "Rome"}}},
        ]})
    );
    // 两个结果合在同一条 user 内容里，名字从 tool_call_id 找回
    assert_eq!(
        out["contents"][2],
        json!({"role": "user", "parts": [
            {"functionResponse": {"name": "get_weather", "response": {"output": {"temp": 20}}}},
            {"functionResponse": {"name": "get_weather", "response": {"output": "sunny"}}},
        ]})
    );
    assert_eq!(
        out["contents"][3],
        json!({"role": "user", "parts": [{"text": "thanks"}]})
    );
}

#[test]
fn missing_signature_gets_placeholder_on_gemini_3_only() {
    let out = tool_round(None, G3);
    assert_eq!(
        out["contents"][1]["parts"][0]["thoughtSignature"],
        SKIP_SIGNATURE
    );
    assert!(
        out["contents"][1]["parts"][1]
            .get("thoughtSignature")
            .is_none()
    );

    let out = tool_round(None, "gemini-2.5-flash");
    assert!(
        out["contents"][1]["parts"][0]
            .get("thoughtSignature")
            .is_none()
    );
}

#[test]
fn unknown_tool_call_id_is_rejected() {
    let err = request_to_gemini(
        &json!({"messages": [
            {"role": "user", "content": "x"},
            {"role": "tool", "tool_call_id": "nope", "content": "1"},
        ]}),
        G3,
    )
    .unwrap_err();
    assert_eq!(err.status, 400);
}

#[test]
fn empty_messages_rejected() {
    for body in [
        json!({"messages": []}),
        json!({"messages": [{"role": "system", "content": "only system"}]}),
    ] {
        assert_eq!(request_to_gemini(&body, G3).unwrap_err().status, 400);
    }
}

fn gemini_response() -> Value {
    json!({
        "responseId": "r1",
        "candidates": [{
            "index": 0,
            "content": {"role": "model", "parts": [
                {"text": "thinking...", "thought": true},
                {"text": "Let me check."},
                {"functionCall": {"name": "get_weather", "args": {"city": "Paris"}}, "thoughtSignature": "sig-9"},
            ]},
            "finishReason": "STOP",
        }],
        "usageMetadata": {
            "promptTokenCount": 10, "candidatesTokenCount": 5, "thoughtsTokenCount": 7,
            "cachedContentTokenCount": 4, "totalTokenCount": 22, "trafficType": "ON_DEMAND_FLEX",
        },
    })
}

#[test]
fn non_stream_response() {
    let out = response_to_openai(&gemini_response(), "gemini-3.5-flash");
    assert_eq!(out["id"], "chatcmpl-r1");
    assert_eq!(out["object"], "chat.completion");
    assert_eq!(out["model"], "gemini-3.5-flash");
    let choice = &out["choices"][0];
    assert_eq!(choice["finish_reason"], "tool_calls");
    assert_eq!(choice["message"]["content"], "Let me check.");
    assert_eq!(choice["message"]["reasoning_content"], "thinking...");
    let call = &choice["message"]["tool_calls"][0];
    assert!(call["id"].as_str().unwrap().starts_with("call_"));
    assert_eq!(call["function"]["name"], "get_weather");
    assert_eq!(call["function"]["arguments"], "{\"city\":\"Paris\"}");
    assert_eq!(
        call["extra_content"]["google"]["thought_signature"],
        "sig-9"
    );
    assert_eq!(
        out["usage"],
        json!({
            "prompt_tokens": 10, "completion_tokens": 12, "total_tokens": 22,
            "completion_tokens_details": {"reasoning_tokens": 7},
            "prompt_tokens_details": {"cached_tokens": 4},
        })
    );
}

#[test]
fn finish_reasons_and_blocked_prompt() {
    assert_eq!(finish_reason("STOP"), "stop");
    assert_eq!(finish_reason("MAX_TOKENS"), "length");
    assert_eq!(finish_reason("SAFETY"), "content_filter");
    assert_eq!(finish_reason("MALFORMED_FUNCTION_CALL"), "stop");
    let out = response_to_openai(&json!({"promptFeedback": {"blockReason": "SAFETY"}}), "m");
    assert_eq!(out["choices"][0]["finish_reason"], "content_filter");
    assert_eq!(out["choices"][0]["message"]["content"], Value::Null);
}

#[test]
fn stream_translation() {
    let mut t = StreamTranslator::new("gemini-3.5-flash", true);
    let mut out = Vec::new();
    out.extend(t.handle(&json!({
        "responseId": "r2",
        "candidates": [{"content": {"role": "model", "parts": [{"text": "Hel"}]}}],
    })));
    out.extend(t.handle(&json!({
        "candidates": [{"content": {"role": "model", "parts": [
            {"text": "lo"},
            {"functionCall": {"name": "get_weather", "args": {}}, "thoughtSignature": "s"},
            {"functionCall": {"name": "get_time", "args": {"tz": "UTC"}}},
        ]}, "finishReason": "STOP"}],
        "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 4, "totalTokenCount": 7},
    })));
    assert!(t.finished);
    out.extend(t.usage_chunk());

    assert!(out.iter().all(|c| c["id"] == "chatcmpl-r2"));
    assert_eq!(
        out[0]["choices"][0]["delta"],
        json!({"role": "assistant", "content": ""})
    );
    assert_eq!(out[1]["choices"][0]["delta"]["content"], "Hel");
    assert_eq!(out[2]["choices"][0]["delta"]["content"], "lo");
    let first = &out[3]["choices"][0]["delta"]["tool_calls"][0];
    assert_eq!(first["index"], 0);
    assert_eq!(first["function"]["arguments"], "{}");
    assert_eq!(first["extra_content"]["google"]["thought_signature"], "s");
    let second = &out[4]["choices"][0]["delta"]["tool_calls"][0];
    assert_eq!(second["index"], 1);
    assert_eq!(second["function"]["name"], "get_time");
    assert_eq!(out[5]["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(out[6]["choices"], json!([]));
    assert_eq!(out[6]["usage"]["total_tokens"], 7);
    assert_eq!(out.len(), 7);
}

#[test]
fn stream_without_usage_or_finish() {
    let mut t = StreamTranslator::new("m", false);
    let out = t.handle(&json!({"candidates": [{"content": {"parts": [{"text": "x"}]}}]}));
    assert_eq!(out.len(), 2);
    assert!(!t.finished);
    assert!(t.usage_chunk().is_none());
}
