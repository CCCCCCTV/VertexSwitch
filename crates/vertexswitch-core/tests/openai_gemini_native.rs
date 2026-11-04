//! OpenAI 入口上的 Gemini 请求需要服务档位时转成原生 generateContent（SPEC §13）。

mod common;

use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, Request, ResponseTemplate};

const OPENAPI: &str = "/projects/proj/locations/global/endpoints/openapi/chat/completions";
const GEMINI_FLASH: &str = "gemini-3-flash-preview";

fn native_path(action: &str) -> String {
    format!(
        "{}/{GEMINI_FLASH}:{action}",
        common::Harness::publisher_path("google", "global")
    )
}

fn chat() -> Value {
    json!({"model": "gemini-3-flash", "messages": [{"role": "user", "content": "ping"}]})
}

fn with(mut body: Value, key: &str, value: Value) -> Value {
    body[key] = value;
    body
}

fn gemini_reply() -> Value {
    json!({
        "responseId": "resp-1",
        "candidates": [{
            "content": {"role": "model", "parts": [
                {"text": "Paris is sunny."},
                {"functionCall": {"name": "get_weather", "args": {"city": "Paris"}}, "thoughtSignature": "sig"},
            ]},
            "finishReason": "STOP",
        }],
        "usageMetadata": {"promptTokenCount": 9, "candidatesTokenCount": 6, "totalTokenCount": 15, "trafficType": "ON_DEMAND_FLEX"},
    })
}

fn gemini_sse(events: &[Value]) -> String {
    events
        .iter()
        .map(|e| format!("data: {e}\r\n\r\n"))
        .collect()
}

fn data_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .map(str::to_owned)
        .collect()
}

async fn mount_native(h: &common::Harness, action: &str, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path(native_path(action)))
        .respond_with(response)
        .mount(&h.upstream)
        .await;
}

async fn requests(h: &common::Harness) -> Vec<Request> {
    h.upstream.received_requests().await.unwrap()
}

async fn post(h: &common::Harness, body: &Value) -> reqwest::Response {
    h.client
        .post(h.url("/v1/chat/completions"))
        .json(body)
        .send()
        .await
        .unwrap()
}

fn tiered(tier: &str) -> vertexswitch_core::config::Settings {
    let mut s = common::settings();
    s.gemini.tier = tier.into();
    s
}

#[tokio::test]
async fn model_tier_sends_request_through_native_api() {
    let mut s = common::settings();
    s.gemini
        .model_tiers
        .insert(GEMINI_FLASH.into(), "flex".into());
    let h = common::start(s).await;
    mount_native(
        &h,
        "generateContent",
        ResponseTemplate::new(200).set_body_json(gemini_reply()),
    )
    .await;

    let body = json!({
        "model": "gemini-3-flash",
        "messages": [
            {"role": "system", "content": "be brief"},
            {"role": "user", "content": "weather?"},
        ],
        "tools": [{
            "type": "function",
            "function": {"name": "get_weather", "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}},
        }],
        "max_tokens": 20,
        "reasoning_effort": "low",
    });
    let r = post(&h, &body).await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["x-vertexswitch-tier"], "flex");
    let out: Value = r.json().await.unwrap();
    assert_eq!(out["id"], "chatcmpl-resp-1");
    assert_eq!(out["model"], "gemini-3-flash");
    let choice = &out["choices"][0];
    assert_eq!(choice["finish_reason"], "tool_calls");
    assert_eq!(choice["message"]["content"], "Paris is sunny.");
    assert_eq!(
        choice["message"]["tool_calls"][0]["extra_content"]["google"]["thought_signature"],
        "sig"
    );
    assert_eq!(out["usage"]["total_tokens"], 15);

    let sent = requests(&h).await;
    assert_eq!(sent.len(), 1);
    let up = &sent[0];
    assert_eq!(up.headers["authorization"], "Bearer tok");
    assert_eq!(up.headers["x-vertex-ai-llm-shared-request-type"], "flex");
    assert_eq!(up.headers["x-server-timeout"], "1800");
    let gbody: Value = up.body_json().unwrap();
    assert_eq!(gbody["systemInstruction"]["parts"][0]["text"], "be brief");
    assert_eq!(gbody["contents"][0]["parts"][0]["text"], "weather?");
    assert_eq!(gbody["generationConfig"]["maxOutputTokens"], 20);
    assert_eq!(
        gbody["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        1024
    );
    assert_eq!(
        gbody["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"]["type"],
        "object"
    );
    assert!(gbody.get("model").is_none());
}

#[tokio::test]
async fn other_models_keep_default_tier() {
    // 只给另一个模型设了 Flex：这个模型仍是默认 Standard，走兼容接口
    let mut s = common::settings();
    s.gemini
        .model_tiers
        .insert("gemini-2.5-pro".into(), "flex".into());
    let h = common::start(s).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "c1", "choices": []})))
        .mount(&h.upstream)
        .await;
    assert_eq!(post(&h, &chat()).await.status(), 200);
    assert_eq!(
        requests(&h).await[0].url.path(),
        format!("/v1beta1{OPENAPI}")
    );
}

#[tokio::test]
async fn tier_override_header_works_on_openai_route() {
    let h = common::start(common::settings()).await;
    mount_native(
        &h,
        "generateContent",
        ResponseTemplate::new(200).set_body_json(gemini_reply()),
    )
    .await;
    let r = h
        .client
        .post(h.url("/v1/chat/completions"))
        .header("X-VertexSwitch-Tier", "priority")
        .json(&chat())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["x-vertexswitch-tier"], "priority");
    let up = &requests(&h).await[0];
    assert_eq!(
        up.headers["x-vertex-ai-llm-shared-request-type"],
        "priority"
    );
    assert!(!up.headers.contains_key("x-vertexswitch-tier"));
}

#[tokio::test]
async fn openai_native_never_keeps_compat_without_tier_headers() {
    let mut s = tiered("flex");
    s.gemini.openai_native = "never".into();
    let h = common::start(s).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "c1", "choices": []})))
        .mount(&h.upstream)
        .await;
    assert_eq!(post(&h, &chat()).await.status(), 200);
    let up = &requests(&h).await[0];
    assert_eq!(up.url.path(), format!("/v1beta1{OPENAPI}"));
    assert!(
        !up.headers
            .keys()
            .any(|k| k.as_str().starts_with("x-vertex-ai-llm") || k == "x-server-timeout")
    );
}

#[tokio::test]
async fn openai_native_always_converts_even_for_standard() {
    let mut s = common::settings();
    s.gemini.openai_native = "always".into();
    let h = common::start(s).await;
    mount_native(
        &h,
        "generateContent",
        ResponseTemplate::new(200).set_body_json(gemini_reply()),
    )
    .await;
    let r = post(&h, &chat()).await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["x-vertexswitch-tier"], "standard");
    let up = &requests(&h).await[0];
    assert!(
        !up.headers
            .contains_key("x-vertex-ai-llm-shared-request-type")
    );
}

#[tokio::test]
async fn native_stream_converts_to_openai_chunks() {
    let h = common::start(tiered("priority")).await;
    let events = [
        json!({"responseId": "rs", "candidates": [{"content": {"role": "model", "parts": [{"text": "Hel"}]}}]}),
        json!({"candidates": [{"content": {"role": "model", "parts": [{"text": "lo"}]}, "finishReason": "STOP"}],
               "usageMetadata": {"promptTokenCount": 2, "candidatesTokenCount": 3, "totalTokenCount": 5, "trafficType": "ON_DEMAND_PRIORITY"}}),
    ];
    Mock::given(method("POST"))
        .and(path(native_path("streamGenerateContent")))
        .and(query_param("alt", "sse"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(gemini_sse(&events)),
        )
        .mount(&h.upstream)
        .await;
    let body = with(
        with(chat(), "stream", json!(true)),
        "stream_options",
        json!({"include_usage": true}),
    );
    let r = post(&h, &body).await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "text/event-stream");
    assert_eq!(r.headers()["x-vertexswitch-tier"], "priority");
    let lines = data_lines(&r.text().await.unwrap());
    assert_eq!(lines.last().unwrap(), "[DONE]");
    let chunks: Vec<Value> = lines[..lines.len() - 1]
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let text: String = chunks
        .iter()
        .filter_map(|c| c["choices"][0]["delta"]["content"].as_str())
        .collect();
    assert_eq!(text, "Hello");
    assert!(chunks.iter().all(|c| c["id"] == "chatcmpl-rs"));
    assert!(
        chunks
            .iter()
            .any(|c| c["choices"][0]["finish_reason"] == "stop")
    );
    assert_eq!(chunks.last().unwrap()["usage"]["total_tokens"], 5);
}

#[tokio::test]
async fn native_stream_cut_short_reports_error() {
    let h = common::start(tiered("priority")).await;
    let events = [json!({"candidates": [{"content": {"parts": [{"text": "partial"}]}}]})];
    mount_native(
        &h,
        "streamGenerateContent",
        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_string(gemini_sse(&events)),
    )
    .await;
    let r = post(&h, &with(chat(), "stream", json!(true))).await;
    let lines = data_lines(&r.text().await.unwrap());
    assert!(!lines.contains(&"[DONE]".to_owned()));
    let last: Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert!(
        last["error"]["message"]
            .as_str()
            .unwrap()
            .contains("提前结束")
    );
}

#[tokio::test]
async fn native_upstream_error_uses_openai_format() {
    let h = common::start(tiered("priority")).await;
    mount_native(
        &h,
        "generateContent",
        ResponseTemplate::new(429).set_body_json(json!({
            "error": {"code": 429, "message": "Resource exhausted", "status": "RESOURCE_EXHAUSTED"},
        })),
    )
    .await;
    let r = post(&h, &chat()).await;
    assert_eq!(r.status(), 429);
    let out: Value = r.json().await.unwrap();
    assert!(
        out["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Resource exhausted")
    );
}

#[tokio::test]
async fn bad_tool_history_is_400_before_upstream() {
    let h = common::start(tiered("flex")).await;
    let body = json!({
        "model": "gemini-3-flash",
        "messages": [
            {"role": "user", "content": "x"},
            {"role": "tool", "tool_call_id": "missing", "content": "1"},
        ],
    });
    let r = post(&h, &body).await;
    assert_eq!(r.status(), 400);
    assert!(requests(&h).await.is_empty());
}

#[tokio::test]
async fn invalid_model_tier_rejected_by_admin_api() {
    let h = common::start(common::settings()).await;
    let r = h
        .admin(reqwest::Method::PATCH, "/admin/api/config")
        .json(&json!({"gemini": {"model_tiers": {GEMINI_FLASH: "turbo"}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let r = h
        .admin(reqwest::Method::PATCH, "/admin/api/config")
        .json(&json!({"gemini": {"model_tiers": {GEMINI_FLASH: "flex"}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let out: Value = r.json().await.unwrap();
    assert_eq!(
        out["settings"]["gemini"]["model_tiers"][GEMINI_FLASH],
        "flex"
    );
    // null 删掉这一项，恢复跟随默认
    let r = h
        .admin(reqwest::Method::PATCH, "/admin/api/config")
        .json(&json!({"gemini": {"model_tiers": {GEMINI_FLASH: null}}}))
        .send()
        .await
        .unwrap();
    let out: Value = r.json().await.unwrap();
    assert_eq!(out["settings"]["gemini"]["model_tiers"], json!({}));
}
