//! OpenAI Chat Completions 入口与模型列表（SPEC §8.3、§8.4、§10）。
//! 移植自 Python 原型的 tests/test_openai_api.py 与 test_anthropic_api.py 中的模型列表部分。

mod common;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const OPENAPI: &str = "/projects/proj/locations/global/endpoints/openapi/chat/completions";

fn completion() -> Value {
    json!({
        "id": "c1",
        "object": "chat.completion",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "pong"}}],
    })
}

fn chat(model: &str) -> Value {
    json!({"model": model, "messages": [{"role": "user", "content": "ping"}]})
}

fn with(mut body: Value, key: &str, value: Value) -> Value {
    body[key] = value;
    body
}

async fn mount(h: &common::Harness, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .respond_with(response)
        .mount(&h.upstream)
        .await;
}

async fn requests(h: &common::Harness) -> Vec<Request> {
    h.upstream.received_requests().await.unwrap()
}

async fn post(h: &common::Harness, path: &str, body: &Value) -> reqwest::Response {
    h.client.post(h.url(path)).json(body).send().await.unwrap()
}

#[tokio::test]
async fn gemini_via_openai_compat_on_all_paths() {
    let mut s = common::settings();
    s.gemini.tier = "flex".into(); // 兼容接口不支持档位头，必须不发
    let h = common::start(s).await;
    mount(&h, ResponseTemplate::new(200).set_body_json(completion())).await;
    for p in [
        "/v1/chat/completions",
        "/chat/completions",
        "/openai/v1/chat/completions",
        "/api/v1/chat/completions",
    ] {
        let r = post(
            &h,
            p,
            &with(chat("gemini-3-flash"), "temperature", json!(0.1)),
        )
        .await;
        assert_eq!(r.status(), 200, "{p}");
        assert_eq!(r.json::<Value>().await.unwrap(), completion());
    }
    for up in requests(&h).await {
        assert_eq!(up.url.path(), format!("/v1beta1{OPENAPI}"));
        assert_eq!(up.headers["authorization"], "Bearer tok");
        assert!(
            !up.headers
                .keys()
                .any(|k| k.as_str().starts_with("x-vertex-ai-llm") || k == "x-server-timeout")
        );
        let sent: Value = up.body_json().unwrap();
        assert_eq!(sent["model"], "google/gemini-3-flash-preview");
        assert_eq!(sent["temperature"], 0.1);
    }
}

#[tokio::test]
async fn google_prefixed_model_name() {
    let h = common::start(common::settings()).await;
    mount(&h, ResponseTemplate::new(200).set_body_json(completion())).await;
    post(&h, "/v1/chat/completions", &chat("google/gemini-3.5-flash")).await;
    let sent: Value = requests(&h).await[0].body_json().unwrap();
    assert_eq!(sent["model"], "google/gemini-3.5-flash");
}

#[tokio::test]
async fn maas_uses_v1_and_maas_region() {
    let mut s = common::settings();
    s.maas.region = "us-central1".into();
    let h = common::start(s).await;
    mount(&h, ResponseTemplate::new(200).set_body_json(completion())).await;
    post(
        &h,
        "/v1/chat/completions",
        &chat("deepseek-ai/deepseek-v3.1-maas"),
    )
    .await;
    let up = &requests(&h).await[0];
    assert_eq!(
        up.url.path(),
        "/v1/projects/proj/locations/us-central1/endpoints/openapi/chat/completions"
    );
    let sent: Value = up.body_json().unwrap();
    assert_eq!(sent["model"], "deepseek-ai/deepseek-v3.1-maas");
}

#[tokio::test]
async fn openai_stream_passthrough() {
    let h = common::start(common::settings()).await;
    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"po\"}}]}\n\ndata: [DONE]\n\n";
    mount(
        &h,
        ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"),
    )
    .await;
    let r = post(
        &h,
        "/v1/chat/completions",
        &with(chat("gemini-3.5-flash"), "stream", json!(true)),
    )
    .await;
    assert!(
        r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    assert_eq!(r.text().await.unwrap(), sse);
}

#[tokio::test]
async fn ollama_needs_no_google_token() {
    let mut s = common::settings();
    s.ollama.enabled = true;
    let upstream = MockServer::start().await;
    Mock::given(path("/api/tags"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(completion()))
        .mount(&upstream)
        .await;
    // 让 Google token 必然失败：Ollama 请求若去取 token 就会得到 500
    let h = {
        let mut s = s.clone();
        s.google.api_base_override = Some(upstream.uri());
        s.ollama.base_url = upstream.uri();
        let state = std::sync::Arc::new(vertexswitch_core::AppState::from_settings(
            s,
            vertexswitch_core::registry::ModelRegistry::builtin(),
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(vertexswitch_core::serve(listener, state));
        base
    };
    let r = reqwest::Client::new()
        .post(format!("{h}/v1/chat/completions"))
        .json(&chat("ollama/qwen3:8b"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let up = upstream
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.url.path() == "/v1/chat/completions")
        .unwrap();
    assert!(up.headers.get("authorization").is_none());
    let sent: Value = up.body_json().unwrap();
    assert_eq!(sent["model"], "qwen3:8b");
}

#[tokio::test]
async fn ollama_disabled() {
    let h = common::start(common::settings()).await;
    let r = post(&h, "/v1/chat/completions", &chat("ollama/qwen3:8b")).await;
    assert_eq!(r.status(), 400);
    assert!(r.json::<Value>().await.unwrap()["error"].is_object());
    assert!(requests(&h).await.is_empty());
}

#[tokio::test]
async fn ollama_discovery_on_startup() {
    let mut s = common::settings();
    s.ollama.enabled = true;
    let upstream = MockServer::start().await;
    Mock::given(path("/api/tags"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"models": [{"name": "llama3.2:3b"}]})),
        )
        .mount(&upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(completion()))
        .mount(&upstream)
        .await;
    let h = common::start_on(s, upstream).await;
    // serve 先完成发现再接受连接；这里轮询到服务就绪
    let mut models = Value::Null;
    for _ in 0..50 {
        if let Ok(r) = h.client.get(h.url("/v1/models")).send().await {
            models = r.json().await.unwrap();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let ids: Vec<&str> = models["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"ollama/llama3.2:3b"));
    let r = post(&h, "/v1/chat/completions", &chat("llama3.2:3b")).await;
    assert_eq!(r.status(), 200);
}

#[tokio::test]
async fn ollama_connection_error_is_not_quota() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let mut s = common::settings();
    s.ollama.enabled = true;
    s.ollama.base_url = dead;
    s.retry.quota_hang_seconds = 0.0;
    let h = common::start_with_override(s).await;
    let r = post(&h, "/v1/chat/completions", &chat("ollama/x")).await;
    assert_eq!(r.status(), 502);
    let msg = r.json::<Value>().await.unwrap()["error"]["message"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(msg.contains("Ollama"), "{msg}");
}

#[tokio::test]
async fn unknown_model_and_bad_body() {
    let h = common::start(common::settings()).await;
    let r = post(&h, "/v1/chat/completions", &chat("gpt-4o")).await;
    assert_eq!(r.status(), 404);
    assert_eq!(
        r.json::<Value>().await.unwrap()["error"]["type"],
        "not_found_error"
    );
    let r = post(&h, "/v1/chat/completions", &json!({"messages": []})).await;
    assert_eq!(r.status(), 400);
    assert!(requests(&h).await.is_empty());
}

#[tokio::test]
async fn upstream_error_status_and_message() {
    let h = common::start(common::settings()).await;
    let err =
        json!([{"error": {"code": 400, "message": "bad thing", "status": "INVALID_ARGUMENT"}}]);
    mount(&h, ResponseTemplate::new(400).set_body_json(err)).await;
    let r = post(&h, "/v1/chat/completions", &chat("gemini-3.5-flash")).await;
    assert_eq!(r.status(), 400);
    assert_eq!(
        r.json::<Value>().await.unwrap()["error"]["message"],
        "bad thing"
    );
}

// ---------------------------------------------------------------- Claude 转换

fn claude_tool_message() -> Value {
    json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"city": "Paris"}}],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 9, "output_tokens": 4},
    })
}

fn tools() -> Value {
    json!([{
        "type": "function",
        "function": {
            "name": "get_weather",
            "parameters": {"type": "object", "properties": {"city": {"type": "string"}}},
        },
    }])
}

#[tokio::test]
async fn claude_non_stream_tool_call() {
    let h = common::start(common::settings()).await;
    mount(
        &h,
        ResponseTemplate::new(200).set_body_json(claude_tool_message()),
    )
    .await;
    let mut body = chat("claude-haiku-4-5");
    body["tools"] = tools();
    body["tool_choice"] = "required".into();
    body["max_tokens"] = 20.into();
    let r = post(&h, "/v1/chat/completions", &body).await;
    assert_eq!(r.status(), 200);
    let up = &requests(&h).await[0];
    assert!(
        up.url
            .path()
            .ends_with("/publishers/anthropic/models/claude-haiku-4-5@20251001:rawPredict")
    );
    let sent: Value = up.body_json().unwrap();
    assert_eq!(sent["anthropic_version"], "vertex-2023-10-16");
    assert!(sent.get("model").is_none());
    assert_eq!(sent["tool_choice"], json!({"type": "any"}));
    assert_eq!(sent["max_tokens"], 20);
    assert_eq!(
        sent["tools"][0]["input_schema"]["properties"]["city"],
        json!({"type": "string"})
    );

    let out: Value = r.json().await.unwrap();
    assert_eq!(out["model"], "claude-haiku-4-5");
    let choice = &out["choices"][0];
    assert_eq!(choice["finish_reason"], "tool_calls");
    let args: Value = serde_json::from_str(
        choice["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(args, json!({"city": "Paris"}));
}

fn anthropic_sse(events: &[Value]) -> String {
    events
        .iter()
        .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()))
        .collect()
}

fn stream_events() -> Vec<Value> {
    vec![
        json!({"type": "message_start", "message": {"id": "msg_s", "usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "toolu_s", "name": "get_weather", "input": {}}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"city\":"}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": " \"Oslo\"}"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 8}}),
        json!({"type": "message_stop"}),
    ]
}

fn data_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn claude_stream_tool_call() {
    let h = common::start(common::settings()).await;
    mount(
        &h,
        ResponseTemplate::new(200)
            .set_body_raw(anthropic_sse(&stream_events()), "text/event-stream"),
    )
    .await;
    let mut body = chat("claude-opus-4-6");
    body["tools"] = tools();
    body["stream"] = true.into();
    body["stream_options"] = json!({"include_usage": true});
    let r = post(&h, "/v1/chat/completions", &body).await;
    assert_eq!(r.status(), 200);
    assert!(
        requests(&h).await[0]
            .url
            .path()
            .ends_with(":streamRawPredict")
    );
    let lines = data_lines(&r.text().await.unwrap());
    assert_eq!(lines.last().unwrap(), "[DONE]");
    let chunks: Vec<Value> = lines[..lines.len() - 1]
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let args: String = chunks
        .iter()
        .filter_map(|c| c["choices"].get(0))
        .flat_map(|ch| {
            ch["delta"]["tool_calls"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .map(|tc| {
            tc["function"]["arguments"]
                .as_str()
                .unwrap_or("")
                .to_owned()
        })
        .collect();
    assert_eq!(
        serde_json::from_str::<Value>(&args).unwrap(),
        json!({"city": "Oslo"})
    );
    let finishes: Vec<&Value> = chunks
        .iter()
        .filter_map(|c| c["choices"].get(0).map(|ch| &ch["finish_reason"]))
        .filter(|f| !f.is_null())
        .collect();
    assert_eq!(finishes, vec![&json!("tool_calls")]);
    assert_eq!(chunks.last().unwrap()["usage"]["completion_tokens"], 8);
}

#[tokio::test]
async fn claude_stream_error_event_becomes_openai_error() {
    let h = common::start(common::settings()).await;
    let events = vec![
        stream_events()[0].clone(),
        json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}),
    ];
    mount(
        &h,
        ResponseTemplate::new(200).set_body_raw(anthropic_sse(&events), "text/event-stream"),
    )
    .await;
    let r = post(
        &h,
        "/v1/chat/completions",
        &with(chat("claude-opus-4-6"), "stream", json!(true)),
    )
    .await;
    let lines = data_lines(&r.text().await.unwrap());
    let last: Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert_eq!(last["error"]["message"], "Overloaded");
    assert!(!lines.contains(&"[DONE]".to_owned()));
}

#[tokio::test]
async fn claude_stream_truncated_reports_error() {
    let h = common::start(common::settings()).await;
    mount(
        &h,
        ResponseTemplate::new(200)
            .set_body_raw(anthropic_sse(&stream_events()[..3]), "text/event-stream"),
    )
    .await;
    let r = post(
        &h,
        "/v1/chat/completions",
        &with(chat("claude-opus-4-6"), "stream", json!(true)),
    )
    .await;
    let text = r.text().await.unwrap();
    assert!(!text.contains("[DONE]"));
    assert!(text.contains("提前结束"));
}

#[tokio::test]
async fn claude_stream_broken_connection_reports_openai_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let broken = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 16384];
        let _ = socket.read(&mut buf).await;
        let part = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m\"}}\n\n";
        let head = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n{:x}\r\n{part}\r\n",
            part.len()
        );
        let _ = socket.write_all(head.as_bytes()).await;
    });
    let mut s = common::settings();
    s.google.api_base_override = Some(broken);
    let h = common::start_with_override(s).await;
    let r = post(
        &h,
        "/v1/chat/completions",
        &with(chat("claude-opus-4-6"), "stream", json!(true)),
    )
    .await;
    let lines = data_lines(&r.text().await.unwrap());
    let last: Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert_eq!(last["error"]["code"], 502);
}

#[tokio::test]
async fn claude_upstream_error_openai_format() {
    let h = common::start(common::settings()).await;
    let err =
        json!({"type": "error", "error": {"type": "rate_limit_error", "message": "slow down"}});
    mount(&h, ResponseTemplate::new(429).set_body_json(err)).await;
    let r = post(&h, "/v1/chat/completions", &chat("claude-opus-4-6")).await;
    assert_eq!(r.status(), 429);
    assert_eq!(
        r.json::<Value>().await.unwrap()["error"],
        json!({"message": "slow down", "type": "rate_limit_error", "param": null, "code": 429})
    );
}

#[tokio::test]
async fn claude_url_image_rejected_before_upstream() {
    let h = common::start(common::settings()).await;
    let body = json!({
        "model": "claude-opus-4-6",
        "messages": [{"role": "user", "content": [{"type": "image_url", "image_url": {"url": "https://x/a.png"}}]}],
    });
    let r = post(&h, "/v1/chat/completions", &body).await;
    assert_eq!(r.status(), 400);
    assert!(requests(&h).await.is_empty());
}

// ---------------------------------------------------------------- 模型列表

#[tokio::test]
async fn models_list_formats() {
    let h = common::start(common::settings()).await;
    let anth: Value = h
        .client
        .get(h.url("/v1/models"))
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(anth["data"][0]["type"], "model");
    assert_eq!(anth["first_id"], "claude-fable-5-1");
    assert_eq!(anth["has_more"], false);
    assert!(
        anth["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["id"].as_str().unwrap().starts_with("claude-"))
    );

    let prefixed: Value = h
        .client
        .get(h.url("/anthropic/v1/models"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(prefixed["data"][0]["type"], "model");

    let oai: Value = h
        .client
        .get(h.url("/v1/models"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(oai["object"], "list");
    let ids: Vec<&str> = oai["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"claude-opus-4-6") && ids.contains(&"gemini-3.5-flash"));

    let one: Value = h
        .client
        .get(h.url("/v1/models/claude-opus-4-6"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        one,
        json!({"id": "claude-opus-4-6", "object": "model", "created": 0, "owned_by": "vertexswitch"})
    );
    let r = h.client.get(h.url("/v1/models/nope")).send().await.unwrap();
    assert_eq!(r.status(), 404);
}
