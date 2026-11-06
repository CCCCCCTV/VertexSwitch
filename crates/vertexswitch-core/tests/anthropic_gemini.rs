//! Claude 格式调用 Gemini 模型（SPEC §14）：`/v1/messages` → Gemini 原生接口，响应翻译回 Claude 格式。

mod common;

use common::Harness;
use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, Request, ResponseTemplate};

const SHARED: &str = "x-vertex-ai-llm-shared-request-type";

fn gpath(model_action: &str) -> String {
    format!(
        "{}/{model_action}",
        Harness::publisher_path("google", "global")
    )
}

fn weather_tool() -> Value {
    json!({
        "name": "get_weather",
        "description": "查天气",
        "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]},
    })
}

fn messages(stream: bool) -> Value {
    json!({
        "model": "gemini-3.5-flash",
        "max_tokens": 256,
        "stream": stream,
        "system": "你是助手",
        "tools": [weather_tool()],
        "messages": [{"role": "user", "content": "北京天气怎么样"}],
    })
}

async fn only_request(h: &Harness) -> Request {
    let mut reqs = h.upstream.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1, "应恰好请求上游一次");
    reqs.remove(0)
}

async fn first_record(h: &Harness) -> Value {
    for _ in 0..60 {
        let r = h
            .admin(reqwest::Method::GET, "/admin/api/requests")
            .send()
            .await
            .unwrap();
        let records: Value = r.json().await.unwrap();
        if let Some(first) = records.as_array().and_then(|r| r.first()) {
            return first.clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("没有请求记录");
}

#[tokio::test]
async fn non_stream_message_with_tool_call() {
    let h = common::start(common::settings()).await;
    Mock::given(method("POST"))
        .and(path(gpath("gemini-3.5-flash:generateContent")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "responseId": "r1",
            "candidates": [{"content": {"role": "model", "parts": [
                {"text": "我查一下。"},
                {"functionCall": {"name": "get_weather", "args": {"city": "北京"}}, "thoughtSignature": "SIG"},
            ]}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 30, "candidatesTokenCount": 8, "trafficType": "ON_DEMAND"},
        })))
        .mount(&h.upstream)
        .await;
    let r = h
        .client
        .post(h.url("/v1/messages"))
        .header("anthropic-version", "2023-06-01")
        .json(&messages(false))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["x-vertexswitch-tier"], "standard");
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["type"], "message");
    assert_eq!(body["model"], "gemini-3.5-flash");
    assert_eq!(body["stop_reason"], "tool_use");
    assert_eq!(
        body["content"][0],
        json!({"type": "text", "text": "我查一下。"})
    );
    assert_eq!(body["content"][1]["signature"], "vsg1:SIG");
    assert_eq!(body["content"][2]["type"], "tool_use");
    assert_eq!(body["content"][2]["name"], "get_weather");
    assert_eq!(
        body["usage"],
        json!({"input_tokens": 30, "output_tokens": 8})
    );

    // 发给上游的是 Gemini 原生请求体
    let up: Value = only_request(&h).await.body_json().unwrap();
    assert_eq!(
        up["systemInstruction"],
        json!({"parts": [{"text": "你是助手"}]})
    );
    assert_eq!(
        up["contents"],
        json!([{"role": "user", "parts": [{"text": "北京天气怎么样"}]}])
    );
    assert_eq!(
        up["tools"][0]["functionDeclarations"][0]["name"],
        "get_weather"
    );
    assert_eq!(up["generationConfig"]["maxOutputTokens"], 256);
    assert!(up.get("model").is_none() && up.get("max_tokens").is_none());

    // 请求记录：入口是 Claude 格式，后端是 Gemini，用量按 Claude 格式的字段读到
    let record = first_record(&h).await;
    assert_eq!(record["protocol"], "anthropic");
    assert_eq!(record["backend"], "gemini");
    assert_eq!(record["model"], "gemini-3.5-flash");
    assert_eq!(record["tier"], "standard");
    assert_eq!(record["input_tokens"], 30);
    assert_eq!(record["output_tokens"], 8);
}

#[tokio::test]
async fn stream_translates_to_claude_events() {
    let h = common::start(common::settings()).await;
    let sse = [
        json!({"responseId": "r2", "candidates": [{"content": {"parts": [{"text": "北京"}]}}], "usageMetadata": {"promptTokenCount": 30}}),
        json!({"candidates": [{"content": {"parts": [{"text": "晴"}]}, "finishReason": "STOP"}], "usageMetadata": {"promptTokenCount": 30, "candidatesTokenCount": 2, "trafficType": "ON_DEMAND"}}),
    ]
    .iter()
    .map(|c| format!("data: {c}\r\n\r\n"))
    .collect::<String>();
    Mock::given(method("POST"))
        .and(path(gpath("gemini-3.5-flash:streamGenerateContent")))
        .and(query_param("alt", "sse"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .mount(&h.upstream)
        .await;
    let r = h
        .client
        .post(h.url("/v1/messages"))
        .json(&messages(true))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert!(
        r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    let text = r.text().await.unwrap();
    let events: Vec<&str> = text
        .lines()
        .filter_map(|l| l.strip_prefix("event: "))
        .collect();
    assert_eq!(
        events,
        [
            "message_start",
            "content_block_start",
            "content_block_delta",
            "content_block_delta",
            "content_block_stop",
            "message_delta",
            "message_stop",
        ]
    );
    assert!(text.contains(r#""text_delta","text":"晴""#), "{text}");
    assert!(text.contains(r#""stop_reason":"end_turn""#), "{text}");

    let record = first_record(&h).await;
    assert_eq!(record["outcome"], "ok");
    assert_eq!(record["stream"], true);
    assert_eq!(record["output_tokens"], 2);
}

#[tokio::test]
async fn stream_upstream_error_becomes_claude_error_event() {
    let h = common::start(common::settings()).await;
    let sse = "data: {\"error\":{\"code\":429,\"message\":\"Resource exhausted\",\"status\":\"RESOURCE_EXHAUSTED\"}}\n\n";
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .mount(&h.upstream)
        .await;
    let text = h
        .client
        .post(h.url("/v1/messages"))
        .json(&messages(true))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(text.contains("event: error"), "{text}");
    assert!(text.contains("Resource exhausted"), "{text}");
    let record = first_record(&h).await;
    assert_eq!(record["outcome"], "stream_error");
}

#[tokio::test]
async fn upstream_error_uses_claude_error_format() {
    let mut s = common::settings();
    s.retry.busy = false;
    let h = common::start(s).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "error": {"code": 429, "message": "Resource exhausted", "status": "RESOURCE_EXHAUSTED"}
        })))
        .mount(&h.upstream)
        .await;
    let r = h
        .client
        .post(h.url("/v1/messages"))
        .json(&messages(false))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 429);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["type"], "error");
    assert_eq!(body["error"]["type"], "rate_limit_error");
}

#[tokio::test]
async fn tiers_apply_like_the_gemini_endpoint() {
    let mut s = common::settings();
    s.gemini.tier = "flex".into();
    let h = common::start(s).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{"content": {"parts": [{"text": "ok"}]}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 1, "trafficType": "ON_DEMAND_FLEX"},
        })))
        .mount(&h.upstream)
        .await;
    let r = h
        .client
        .post(h.url("/v1/messages"))
        .json(&messages(false))
        .send()
        .await
        .unwrap();
    assert_eq!(r.headers()["x-vertexswitch-tier"], "flex");
    assert_eq!(only_request(&h).await.headers[SHARED], "flex");
}

#[tokio::test]
async fn count_tokens_goes_to_gemini() {
    let h = common::start(common::settings()).await;
    Mock::given(method("POST"))
        .and(path(gpath("gemini-3.5-flash:countTokens")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"totalTokens": 42})))
        .mount(&h.upstream)
        .await;
    let r = h
        .client
        .post(h.url("/v1/messages/count_tokens"))
        .json(&messages(false))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({"input_tokens": 42})
    );
    let up: Value = only_request(&h).await.body_json().unwrap();
    assert!(up.get("generationConfig").is_none() && up.get("contents").is_some());
}
