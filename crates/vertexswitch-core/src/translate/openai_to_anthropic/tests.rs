//! 移植自 Python 原型的 tests/test_translate.py。

use serde_json::{Value, json};

use super::*;

fn tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "get_weather",
            "description": "查天气",
            "parameters": {"type": "object", "properties": {"city": {"type": "string"}}},
        },
    })
}

fn convert(body: Value) -> Value {
    request_to_anthropic(&body, 100).unwrap()
}

#[test]
fn basic_request() {
    let out = request_to_anthropic(
        &json!({
            "model": "x",
            "messages": [
                {"role": "system", "content": "be brief"},
                {"role": "developer", "content": [{"type": "text", "text": "really"}]},
                {"role": "user", "content": "hi"},
            ],
            "max_completion_tokens": 50,
            "temperature": 0.2,
            "stop": "END",
            "user": "u1",
            "stream": true,
        }),
        8192,
    )
    .unwrap();
    assert_eq!(
        out,
        json!({
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
            "system": "be brief\n\nreally",
            "max_tokens": 50,
            "temperature": 0.2,
            "stop_sequences": ["END"],
            "metadata": {"user_id": "u1"},
            "stream": true,
        })
    );
}

#[test]
fn default_max_tokens() {
    let out = request_to_anthropic(
        &json!({"messages": [{"role": "user", "content": "hi"}]}),
        1234,
    )
    .unwrap();
    assert_eq!(out["max_tokens"], 1234);
}

#[test]
fn tools_and_tool_choice() {
    let out = convert(json!({
        "messages": [{"role": "user", "content": "weather?"}],
        "tools": [tool()],
        "tool_choice": {"type": "function", "function": {"name": "get_weather"}},
        "parallel_tool_calls": false,
    }));
    assert_eq!(
        out["tools"],
        json!([{
            "name": "get_weather",
            "description": "查天气",
            "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}},
        }])
    );
    assert_eq!(
        out["tool_choice"],
        json!({"type": "tool", "name": "get_weather", "disable_parallel_tool_use": true})
    );
}

#[test]
fn tool_choice_strings() {
    for (choice, expected) in [
        ("auto", json!({"type": "auto"})),
        ("none", json!({"type": "none"})),
        ("required", json!({"type": "any"})),
    ] {
        let out = convert(json!({
            "messages": [{"role": "user", "content": "x"}],
            "tools": [tool()],
            "tool_choice": choice,
        }));
        assert_eq!(out["tool_choice"], expected, "{choice}");
    }
}

#[test]
fn parallel_false_without_choice_means_auto() {
    let out = convert(json!({
        "messages": [{"role": "user", "content": "x"}],
        "tools": [tool()],
        "parallel_tool_calls": false,
    }));
    assert_eq!(
        out["tool_choice"],
        json!({"type": "auto", "disable_parallel_tool_use": true})
    );
    let out = convert(json!({
        "messages": [{"role": "user", "content": "x"}],
        "tool_choice": "none",
        "parallel_tool_calls": false,
    }));
    assert_eq!(out["tool_choice"], json!({"type": "none"}));
}

#[test]
fn tool_call_roundtrip_history() {
    let out = convert(json!({
        "messages": [
            {"role": "user", "content": "weather in Paris and Rome?"},
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"Paris\"}"}},
                    {"id": "call_2", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"Rome\"}"}},
                ],
            },
            {"role": "tool", "tool_call_id": "call_1", "content": "sunny"},
            {"role": "tool", "tool_call_id": "call_2", "content": "rain"},
            {"role": "user", "content": "thanks"},
        ]
    }));
    assert_eq!(
        out["messages"],
        json!([
            {"role": "user", "content": [{"type": "text", "text": "weather in Paris and Rome?"}]},
            {
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {"city": "Paris"}},
                    {"type": "tool_use", "id": "call_2", "name": "get_weather", "input": {"city": "Rome"}},
                ],
            },
            {
                "role": "user",
                "content": [
                    {"type": "tool_result", "tool_use_id": "call_1", "content": "sunny"},
                    {"type": "tool_result", "tool_use_id": "call_2", "content": "rain"},
                    {"type": "text", "text": "thanks"},
                ],
            },
        ])
    );
}

#[test]
fn base64_image() {
    let out = convert(json!({
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "what is this"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
            ],
        }]
    }));
    assert_eq!(
        out["messages"][0]["content"][1],
        json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"}})
    );
}

#[test]
fn invalid_requests() {
    let cases = [
        json!({"messages": []}),
        json!({"messages": [{"role": "user", "content": "x"}], "n": 2}),
        json!({"messages": [{"role": "user", "content": [{"type": "image_url", "image_url": {"url": "https://x/a.png"}}]}]}),
        json!({"messages": [{"role": "user", "content": "x"}], "tools": [{"type": "retrieval"}]}),
        json!({"messages": [{"role": "wizard", "content": "x"}]}),
        json!({"messages": [{"role": "assistant", "tool_calls": [{"id": "c", "function": {"name": "f", "arguments": "{oops"}}]}]}),
        json!({"messages": [{"role": "user", "content": "x"}], "tool_choice": "sometimes"}),
    ];
    for body in cases {
        let err = request_to_anthropic(&body, 100).unwrap_err();
        assert_eq!(err.status, 400, "{body}");
    }
}

#[test]
fn response_with_text_thinking_and_tools() {
    let msg = json!({
        "id": "msg_1",
        "content": [
            {"type": "thinking", "thinking": "hmm"},
            {"type": "text", "text": "Checking."},
            {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"city": "巴黎"}},
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 10, "cache_read_input_tokens": 5, "output_tokens": 7},
    });
    let out = response_to_openai(&msg, "claude-x");
    assert_eq!(out["object"], "chat.completion");
    assert_eq!(out["model"], "claude-x");
    let choice = &out["choices"][0];
    assert_eq!(choice["finish_reason"], "tool_calls");
    assert_eq!(choice["message"]["content"], "Checking.");
    assert_eq!(choice["message"]["reasoning_content"], "hmm");
    let call = &choice["message"]["tool_calls"][0];
    assert_eq!(call["id"], "toolu_1");
    let args: Value =
        serde_json::from_str(call["function"]["arguments"].as_str().unwrap()).unwrap();
    assert_eq!(args, json!({"city": "巴黎"}));
    assert_eq!(
        out["usage"],
        json!({
            "prompt_tokens": 15,
            "completion_tokens": 7,
            "total_tokens": 22,
            "prompt_tokens_details": {"cached_tokens": 5},
        })
    );
}

#[test]
fn finish_reasons() {
    for (stop, finish) in [
        ("end_turn", "stop"),
        ("max_tokens", "length"),
        ("stop_sequence", "stop"),
        ("refusal", "content_filter"),
        ("model_context_window_exceeded", "length"),
    ] {
        let out = response_to_openai(&json!({"content": [], "stop_reason": stop}), "m");
        assert_eq!(out["choices"][0]["finish_reason"], finish, "{stop}");
        assert!(out["choices"][0]["message"]["content"].is_null());
    }
}

fn events() -> Vec<(&'static str, Value)> {
    vec![
        (
            "message_start",
            json!({"type": "message_start", "message": {"id": "msg_9", "usage": {"input_tokens": 12}}}),
        ),
        (
            "content_block_start",
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        ),
        ("ping", json!({"type": "ping"})),
        (
            "content_block_delta",
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Let me check."}}),
        ),
        (
            "content_block_stop",
            json!({"type": "content_block_stop", "index": 0}),
        ),
        (
            "content_block_start",
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "toolu_a", "name": "get_weather", "input": {}}}),
        ),
        (
            "content_block_delta",
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"city\": \"Pa"}}),
        ),
        (
            "content_block_delta",
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "ris\"}"}}),
        ),
        (
            "content_block_stop",
            json!({"type": "content_block_stop", "index": 1}),
        ),
        (
            "content_block_start",
            json!({"type": "content_block_start", "index": 2, "content_block": {"type": "tool_use", "id": "toolu_b", "name": "get_weather", "input": {}}}),
        ),
        (
            "content_block_delta",
            json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "{\"city\": \"Rome\"}"}}),
        ),
        (
            "content_block_stop",
            json!({"type": "content_block_stop", "index": 2}),
        ),
        (
            "message_delta",
            json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 30}}),
        ),
        ("message_stop", json!({"type": "message_stop"})),
    ]
}

fn run(include_usage: bool) -> (StreamTranslator, Vec<Value>) {
    let mut t = StreamTranslator::new("claude-x", include_usage);
    let chunks = events()
        .iter()
        .flat_map(|(ev, data)| t.handle(ev, data))
        .collect();
    (t, chunks)
}

#[test]
fn stream_translator_tool_calls() {
    let (t, chunks) = run(true);
    assert!(t.finished);
    assert!(chunks.iter().all(|c| c["id"] == "msg_9"));
    let deltas: Vec<&Value> = chunks
        .iter()
        .filter_map(|c| c["choices"].get(0).map(|ch| &ch["delta"]))
        .collect();
    assert_eq!(*deltas[0], json!({"role": "assistant", "content": ""}));
    let text: String = deltas
        .iter()
        .filter_map(|d| d.get("content").and_then(Value::as_str))
        .collect();
    assert_eq!(text, "Let me check.");

    // 按 index 重新拼出每个工具调用
    let mut calls: Vec<(String, String, String)> = vec![Default::default(); 2];
    for d in &deltas {
        for tc in d
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let entry = &mut calls[tc["index"].as_u64().unwrap() as usize];
            if let Some(id) = tc.get("id").and_then(Value::as_str) {
                entry.0 = id.into();
            }
            if let Some(name) = tc["function"].get("name").and_then(Value::as_str) {
                entry.1 = name.into();
            }
            entry.2 += tc["function"]["arguments"].as_str().unwrap_or("");
        }
    }
    assert_eq!(
        calls,
        vec![
            (
                "toolu_a".into(),
                "get_weather".into(),
                "{\"city\": \"Paris\"}".into()
            ),
            (
                "toolu_b".into(),
                "get_weather".into(),
                "{\"city\": \"Rome\"}".into()
            ),
        ]
    );
    let finishes: Vec<&Value> = chunks
        .iter()
        .filter_map(|c| c["choices"].get(0).map(|ch| &ch["finish_reason"]))
        .filter(|f| !f.is_null())
        .collect();
    assert_eq!(finishes, vec![&json!("tool_calls")]);
    let last = chunks.last().unwrap();
    assert_eq!(last["choices"], json!([]));
    assert_eq!(last["usage"]["completion_tokens"], 30);
    assert_eq!(last["usage"]["prompt_tokens"], 12);
}

#[test]
fn stream_translator_without_usage_chunk() {
    let (_, chunks) = run(false);
    assert!(
        chunks
            .iter()
            .all(|c| !c["choices"].as_array().unwrap().is_empty())
    );
}

#[test]
fn sse_parser_handles_arbitrary_splits() {
    let raw = "event: message_start\r\ndata: {\"a\": 1}\r\n\r\nevent: ping\ndata: {}\n\nevent: content_block_delta\ndata: {\"t\": \"你\"}\n\n".as_bytes();
    for size in [1, 3, 7, raw.len()] {
        let mut p = SseParser::default();
        let events: Vec<(String, String)> = raw.chunks(size).flat_map(|c| p.feed(c)).collect();
        assert_eq!(
            events,
            vec![
                ("message_start".into(), "{\"a\": 1}".into()),
                ("ping".into(), "{}".into()),
                ("content_block_delta".into(), "{\"t\": \"你\"}".into()),
            ],
            "size {size}"
        );
    }
}
