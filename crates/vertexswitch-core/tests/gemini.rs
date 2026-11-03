//! Gemini 原生入口与服务档位（SPEC §8.2、§9）。移植自 Python 原型的 tests/test_gemini_api.py。

mod common;

use common::Harness;
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, Request, ResponseTemplate};

const SHARED: &str = "x-vertex-ai-llm-shared-request-type";
const REQ_TYPE: &str = "x-vertex-ai-llm-request-type";
const TIMEOUT: &str = "x-server-timeout";

fn body() -> Value {
    json!({"contents": [{"role": "user", "parts": [{"text": "hi"}]}]})
}

fn reply(traffic: &str) -> Value {
    json!({
        "candidates": [{"content": {"role": "model", "parts": [{"text": "yo"}]}}],
        "usageMetadata": {"promptTokenCount": 1, "trafficType": traffic},
    })
}

fn gpath(model_action: &str) -> String {
    format!(
        "{}/{model_action}",
        Harness::publisher_path("google", "global")
    )
}

async fn mount_ok(h: &Harness, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .respond_with(response)
        .mount(&h.upstream)
        .await;
}

async fn only_request(h: &Harness) -> Request {
    let mut reqs = h.upstream.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1, "应恰好请求上游一次");
    reqs.remove(0)
}

async fn post(h: &Harness, path: &str, body: &Value) -> reqwest::Response {
    h.client.post(h.url(path)).json(body).send().await.unwrap()
}

#[tokio::test]
async fn generate_content_url_and_standard_headers() {
    let h = common::start(common::settings()).await;
    Mock::given(method("POST"))
        .and(path(gpath("gemini-3-flash-preview:generateContent")))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(reply("ON_DEMAND")))
        .expect(1)
        .mount(&h.upstream)
        .await;
    let r = post(&h, "/v1beta/models/gemini-3-flash:generateContent", &body()).await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["x-vertexswitch-tier"], "standard");
    assert_eq!(r.json::<Value>().await.unwrap(), reply("ON_DEMAND"));
    let up = only_request(&h).await;
    assert!(up.headers.get(SHARED).is_none());
    assert!(up.headers.get(REQ_TYPE).is_none());
}

#[tokio::test]
async fn gemini_prefix_and_models_prefix() {
    let h = common::start(common::settings()).await;
    mount_ok(
        &h,
        ResponseTemplate::new(200).set_body_json(reply("ON_DEMAND")),
    )
    .await;
    let r = post(
        &h,
        "/gemini/v1beta/models/models/gemini-3.8-flash:generateContent",
        &body(),
    )
    .await;
    assert_eq!(r.status(), 200);
    assert_eq!(
        only_request(&h).await.url.path(),
        gpath("gemini-3.8-flash:generateContent")
    );
}

#[tokio::test]
async fn v1_path_works() {
    let h = common::start(common::settings()).await;
    mount_ok(
        &h,
        ResponseTemplate::new(200).set_body_json(reply("ON_DEMAND")),
    )
    .await;
    let r = post(&h, "/v1/models/gemini-3.5-flash:generateContent", &body()).await;
    assert_eq!(r.status(), 200);
}

#[tokio::test]
async fn stream_passes_query_but_not_key() {
    let mut s = common::settings();
    s.server.api_key = "k".into();
    let h = common::start(s).await;
    let sse = "data: {\"usageMetadata\": {\"trafficType\": \"ON_DEMAND\"}}\r\n\r\n";
    Mock::given(method("POST"))
        .and(path(gpath("gemini-3.5-flash-lite:streamGenerateContent")))
        .and(query_param("alt", "sse"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .mount(&h.upstream)
        .await;
    let r = post(
        &h,
        "/v1beta/models/gemini-3.5-flash-lite:streamGenerateContent?alt=sse&key=k",
        &body(),
    )
    .await;
    assert_eq!(r.status(), 200);
    assert!(
        r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    assert_eq!(r.text().await.unwrap(), sse);
    let up = only_request(&h).await;
    assert!(!up.url.query_pairs().any(|(k, _)| k == "key"));
}

#[tokio::test]
async fn flex_headers_sent() {
    let mut s = common::settings();
    s.gemini.tier = "flex".into();
    let h = common::start(s).await;
    mount_ok(
        &h,
        ResponseTemplate::new(200).set_body_json(reply("ON_DEMAND_FLEX")),
    )
    .await;
    let r = post(
        &h,
        "/v1beta/models/gemini-3.5-flash:generateContent",
        &body(),
    )
    .await;
    assert_eq!(r.headers()["x-vertexswitch-tier"], "flex");
    let up = only_request(&h).await;
    assert_eq!(up.headers[SHARED], "flex");
    assert_eq!(up.headers[TIMEOUT], "1800");
}

#[tokio::test]
async fn flex_wait_outlasts_normal_read_timeout() {
    // 本地读超时 0.3 秒，但 Flex 排队时本地等待应不小于 X-Server-Timeout
    let mut s = common::settings();
    s.gemini.tier = "flex".into();
    s.gemini.flex_server_timeout = 5;
    s.timeouts.read = 0.3;
    let h = common::start(s).await;
    mount_ok(
        &h,
        ResponseTemplate::new(200)
            .set_body_json(reply("ON_DEMAND_FLEX"))
            .set_delay(std::time::Duration::from_millis(800)),
    )
    .await;
    let r = post(
        &h,
        "/v1beta/models/gemini-3.5-flash:generateContent",
        &body(),
    )
    .await;
    assert_eq!(r.status(), 200);
}

#[tokio::test]
async fn flex_on_argon_downgrades() {
    let mut s = common::settings();
    s.gemini.tier = "flex".into();
    let h = common::start(s).await;
    mount_ok(
        &h,
        ResponseTemplate::new(200).set_body_json(reply("ON_DEMAND")),
    )
    .await;
    let r = post(&h, "/v1beta/models/gemini-4-argon:generateContent", &body()).await;
    assert_eq!(r.headers()["x-vertexswitch-tier"], "standard");
    assert!(only_request(&h).await.headers.get(SHARED).is_none());
}

#[tokio::test]
async fn flex_non_global_region_downgrades() {
    let mut s = common::settings();
    s.gemini.tier = "flex".into();
    s.gemini.region = "us-central1".into();
    let h = common::start(s).await;
    mount_ok(
        &h,
        ResponseTemplate::new(200).set_body_json(reply("ON_DEMAND")),
    )
    .await;
    post(
        &h,
        "/v1beta/models/gemini-3.5-flash:generateContent",
        &body(),
    )
    .await;
    let up = only_request(&h).await;
    assert!(up.url.path().contains("/locations/us-central1/"));
    assert!(up.headers.get(SHARED).is_none());
}

#[tokio::test]
async fn priority_on_us_multiregion() {
    let mut s = common::settings();
    s.gemini.tier = "priority".into();
    s.gemini.region = "us".into();
    let h = common::start(s).await;
    mount_ok(
        &h,
        ResponseTemplate::new(200).set_body_json(reply("ON_DEMAND_PRIORITY")),
    )
    .await;
    post(
        &h,
        "/v1beta/models/gemini-3.5-flash:generateContent",
        &body(),
    )
    .await;
    let up = only_request(&h).await;
    assert!(up.url.path().contains("/locations/us/"));
    assert_eq!(up.headers[SHARED], "priority");
}

#[tokio::test]
async fn shared_only_header() {
    let mut s = common::settings();
    s.gemini.tier = "priority".into();
    s.gemini.shared_only = true;
    let h = common::start(s).await;
    mount_ok(
        &h,
        ResponseTemplate::new(200).set_body_json(reply("ON_DEMAND_PRIORITY")),
    )
    .await;
    post(
        &h,
        "/v1beta/models/gemini-3.5-flash:generateContent",
        &body(),
    )
    .await;
    let up = only_request(&h).await;
    assert_eq!(up.headers[REQ_TYPE], "shared");
    assert_eq!(up.headers[SHARED], "priority");
}

#[tokio::test]
async fn tier_override_header_not_forwarded() {
    let h = common::start(common::settings()).await;
    mount_ok(
        &h,
        ResponseTemplate::new(200).set_body_json(reply("ON_DEMAND_PRIORITY")),
    )
    .await;
    h.client
        .post(h.url("/v1beta/models/gemini-3.5-flash:generateContent"))
        .header("X-VertexSwitch-Tier", "priority")
        .json(&body())
        .send()
        .await
        .unwrap();
    let up = only_request(&h).await;
    assert_eq!(up.headers[SHARED], "priority");
    assert!(up.headers.get("x-vertexswitch-tier").is_none());
}

#[tokio::test]
async fn count_tokens_never_gets_tier_headers() {
    let mut s = common::settings();
    s.gemini.tier = "flex".into();
    let h = common::start(s).await;
    mount_ok(
        &h,
        ResponseTemplate::new(200).set_body_json(json!({"totalTokens": 3})),
    )
    .await;
    let r = post(&h, "/v1beta/models/gemini-3.5-flash:countTokens", &body()).await;
    assert!(r.headers().get("x-vertexswitch-tier").is_none());
    assert_eq!(r.json::<Value>().await.unwrap(), json!({"totalTokens": 3}));
    let up = only_request(&h).await;
    assert!(up.url.path().ends_with(":countTokens"));
    assert!(up.headers.get(SHARED).is_none());
}

#[tokio::test]
async fn missing_role_filled_with_user() {
    let h = common::start(common::settings()).await;
    mount_ok(
        &h,
        ResponseTemplate::new(200).set_body_json(reply("ON_DEMAND")),
    )
    .await;
    post(
        &h,
        "/v1beta/models/gemini-3.5-flash:generateContent",
        &json!({"contents": [{"parts": [{"text": "hi"}]}]}),
    )
    .await;
    let sent: Value = only_request(&h).await.body_json().unwrap();
    assert_eq!(sent["contents"][0]["role"], "user");
}

#[tokio::test]
async fn google_error_passthrough() {
    let h = common::start(common::settings()).await;
    let err = json!({"error": {"code": 429, "message": "Resource exhausted", "status": "RESOURCE_EXHAUSTED"}});
    mount_ok(
        &h,
        ResponseTemplate::new(429).set_body_json(json!([err.clone()])),
    )
    .await;
    let r = post(
        &h,
        "/v1beta/models/gemini-3.5-flash:generateContent",
        &body(),
    )
    .await;
    assert_eq!(r.status(), 429);
    assert_eq!(r.json::<Value>().await.unwrap(), err);
}

#[tokio::test]
async fn bad_targets_use_gemini_error_format() {
    let h = common::start(common::settings()).await;
    for target in [
        "/v1beta/models/gemini-3.5-flash:predict",
        "/v1beta/models/gemini-3.5-flash",
        "/v1beta/models/claude-opus-4-6:generateContent",
        "/v1/models/gemini-nope-x:predict",
    ] {
        let r = post(&h, target, &body()).await;
        assert_eq!(r.status(), 404, "{target}");
        let b: Value = r.json().await.unwrap();
        assert_eq!(b["error"]["status"], "NOT_FOUND", "{target}");
    }
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn invalid_json_is_400() {
    let h = common::start(common::settings()).await;
    for payload in ["not json", "[1,2]"] {
        let r = h
            .client
            .post(h.url("/v1beta/models/gemini-3.5-flash:generateContent"))
            .header("content-type", "application/json")
            .body(payload)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
        let b: Value = r.json().await.unwrap();
        assert_eq!(b["error"]["status"], "INVALID_ARGUMENT");
    }
}

#[tokio::test]
async fn connection_error_gemini_format() {
    // 指向一个已释放的端口：连接被拒绝
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let mut s = common::settings();
    s.retry.attempts = 0;
    s.google.api_base_override = Some(dead);
    let h = common::start_with_override(s).await;
    let r = post(
        &h,
        "/v1beta/models/gemini-3.5-flash:generateContent",
        &body(),
    )
    .await;
    assert_eq!(r.status(), 502);
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["error"]["status"], "UNAVAILABLE");
}

#[tokio::test]
async fn stream_error_after_start_emits_gemini_error_event() {
    // 上游声明的长度比实际发送的多，连接随即关闭 → 流中途出错
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_mid = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 16384];
        let _ = socket.read(&mut buf).await;
        let _ = socket
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n6\r\ndata: \r\n")
            .await;
    });
    let mut s = common::settings();
    s.google.api_base_override = Some(dead_mid);
    let h = common::start_with_override(s).await;
    let r = post(
        &h,
        "/v1beta/models/gemini-3.5-flash:streamGenerateContent?alt=sse",
        &body(),
    )
    .await;
    assert_eq!(r.status(), 200);
    let text = r.text().await.unwrap();
    let last = text.trim_end().rsplit("data: ").next().unwrap();
    let err: Value = serde_json::from_str(last).unwrap();
    assert_eq!(err["error"]["code"], 502);
}

#[tokio::test]
async fn traffic_mismatch_does_not_break_response() {
    // 档位不符只记告警，不影响响应本身
    let mut s = common::settings();
    s.gemini.tier = "flex".into();
    let h = common::start(s).await;
    let chunk = "data: {\"usageMetadata\": {\"trafficType\": \"ON_DEMAND\"}}\r\n\r\n".repeat(3);
    mount_ok(
        &h,
        ResponseTemplate::new(200).set_body_raw(chunk.clone(), "text/event-stream"),
    )
    .await;
    let r = post(
        &h,
        "/v1beta/models/gemini-3.5-flash:streamGenerateContent?alt=sse",
        &body(),
    )
    .await;
    assert_eq!(r.text().await.unwrap(), chunk);
}

#[tokio::test]
async fn gemini_models_list() {
    let h = common::start(common::settings()).await;
    let models: Value = h
        .client
        .get(h.url("/v1beta/models"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let names: Vec<&str> = models["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"models/gemini-3.8-flash"));
    let one: Value = h
        .client
        .get(h.url("/gemini/v1beta/models/models/gemini-3.8-flash"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(one["name"], "models/gemini-3.8-flash");
    let r = h
        .client
        .get(h.url("/v1beta/models/nope"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
}
