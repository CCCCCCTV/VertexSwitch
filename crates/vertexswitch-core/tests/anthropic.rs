//! Anthropic Messages 入口（SPEC §8.1）。移植自 Python 原型的 tests/test_anthropic_api.py。
//! 用户的密钥不能调用 Claude，这条路径只能用假上游验证。

mod common;

use common::Harness;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::matchers::{method, path};
use wiremock::{Mock, Request, ResponseTemplate};

fn message() -> Value {
    json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "hi"}],
        "model": "claude-opus-4-6",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 3, "output_tokens": 1},
    })
}

fn req() -> Value {
    json!({
        "model": "claude-haiku-4-5",
        "max_tokens": 10,
        "messages": [{"role": "user", "content": "hi"}],
    })
}

fn with(mut body: Value, key: &str, value: Value) -> Value {
    body[key] = value;
    body
}

fn cpath(model_action: &str) -> String {
    format!(
        "{}/{model_action}",
        Harness::publisher_path("anthropic", "global")
    )
}

async fn mount(h: &Harness, response: ResponseTemplate) {
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
async fn non_stream_rewrites_and_forwards() {
    let h = common::start(common::settings()).await;
    Mock::given(method("POST"))
        .and(path(cpath("claude-haiku-4-5@20251001:rawPredict")))
        .respond_with(ResponseTemplate::new(200).set_body_json(message()))
        .expect(1)
        .mount(&h.upstream)
        .await;
    let r = h
        .client
        .post(h.url("/v1/messages"))
        .header("anthropic-beta", "foo-2025")
        .header("anthropic-version", "2023-06-01")
        .json(&with(req(), "context_management", json!({"edits": []})))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.json::<Value>().await.unwrap(), message());

    let up = only_request(&h).await;
    assert_eq!(up.headers["authorization"], "Bearer tok");
    assert_eq!(up.headers["anthropic-beta"], "foo-2025");
    assert!(up.headers.get("anthropic-version").is_none());
    let sent: Value = up.body_json().unwrap();
    assert!(sent.get("model").is_none());
    assert!(sent.get("context_management").is_none());
    assert_eq!(sent["anthropic_version"], "vertex-2023-10-16");
    assert_eq!(sent["max_tokens"], 10);
}

#[tokio::test]
async fn anthropic_prefix_route() {
    let h = common::start(common::settings()).await;
    mount(&h, ResponseTemplate::new(200).set_body_json(message())).await;
    let r = post(&h, "/anthropic/v1/messages", &req()).await;
    assert_eq!(r.status(), 200);
}

#[tokio::test]
async fn regional_claude_uses_region_in_path() {
    let mut s = common::settings();
    s.google.region = "us-east5".into();
    let h = common::start(s).await;
    mount(&h, ResponseTemplate::new(200).set_body_json(message())).await;
    post(&h, "/v1/messages", &req()).await;
    assert_eq!(
        only_request(&h).await.url.path(),
        format!(
            "{}/claude-haiku-4-5@20251001:rawPredict",
            Harness::publisher_path("anthropic", "us-east5")
        )
    );
}

#[tokio::test]
async fn stream_passthrough() {
    let h = common::start(common::settings()).await;
    let sse = "event: message_start\ndata: {\"type\":\"message_start\"}\n\n\
               event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
    Mock::given(method("POST"))
        .and(path(cpath("claude-haiku-4-5@20251001:streamRawPredict")))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .mount(&h.upstream)
        .await;
    let r = post(&h, "/v1/messages", &with(req(), "stream", json!(true))).await;
    assert_eq!(r.status(), 200);
    assert!(
        r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    assert_eq!(r.headers()["cache-control"], "no-cache");
    assert_eq!(r.text().await.unwrap(), sse);
}

#[tokio::test]
async fn stream_false_is_not_stream() {
    let h = common::start(common::settings()).await;
    mount(&h, ResponseTemplate::new(200).set_body_json(message())).await;
    post(&h, "/v1/messages", &with(req(), "stream", json!(false))).await;
    assert!(only_request(&h).await.url.path().ends_with(":rawPredict"));
}

#[tokio::test]
async fn stream_error_after_start_emits_error_event() {
    // 发出一个完整事件和半个事件后断开连接
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let broken = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 16384];
        let _ = socket.read(&mut buf).await;
        let part = "event: message_start\ndata: {\"type\":\"message_start\"}\n\nevent: content_bl";
        let head = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n{:x}\r\n{part}\r\n",
            part.len()
        );
        let _ = socket.write_all(head.as_bytes()).await;
    });
    let mut s = common::settings();
    s.google.api_base_override = Some(broken);
    let h = common::start_with_override(s).await;
    let r = post(&h, "/v1/messages", &with(req(), "stream", json!(true))).await;
    assert_eq!(r.status(), 200);
    let text = r.text().await.unwrap();
    assert!(text.starts_with("event: message_start"));
    let tail = text.rsplit("event: error\n").next().unwrap();
    let payload: Value = serde_json::from_str(tail.trim().trim_start_matches("data: ")).unwrap();
    assert_eq!(payload["type"], "error");
    assert_eq!(payload["error"]["type"], "api_error");
}

#[tokio::test]
async fn upstream_anthropic_error_passthrough() {
    let h = common::start(common::settings()).await;
    let err =
        json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}});
    mount(&h, ResponseTemplate::new(529).set_body_json(err.clone())).await;
    let r = post(&h, "/v1/messages", &with(req(), "stream", json!(true))).await;
    assert_eq!(r.status(), 529);
    assert_eq!(r.json::<Value>().await.unwrap(), err);
}

#[tokio::test]
async fn upstream_google_error_converted() {
    let h = common::start(common::settings()).await;
    let google = json!([{"error": {"code": 429, "message": "Quota exceeded", "status": "RESOURCE_EXHAUSTED"}}]);
    mount(&h, ResponseTemplate::new(429).set_body_json(google)).await;
    let r = post(&h, "/v1/messages", &req()).await;
    assert_eq!(r.status(), 429);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({"type": "error", "error": {"type": "rate_limit_error", "message": "Quota exceeded"}})
    );
}

#[tokio::test]
async fn connection_failure_returns_anthropic_502() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let mut s = common::settings();
    s.retry.attempts = 0;
    s.google.api_base_override = Some(dead);
    let h = common::start_with_override(s).await;
    let r = post(&h, "/v1/messages", &req()).await;
    assert_eq!(r.status(), 502);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["type"], "error");
    assert_eq!(body["error"]["type"], "api_error");
}

#[tokio::test]
async fn bad_requests_use_anthropic_format() {
    let h = common::start(common::settings()).await;
    let cases: [(&str, u16); 6] = [
        ("not json", 400),
        ("[1,2]", 400),
        (r#"{"max_tokens": 1}"#, 400),
        (r#"{"model": "  ", "max_tokens": 1}"#, 400),
        // Gemini 模型可以走这个入口（翻译），但没有 messages 时是请求错误
        (r#"{"model": "gemini-3.5-flash", "max_tokens": 1}"#, 400),
        // MaaS 开放模型不走这个入口
        (
            r#"{"model": "deepseek-ai/deepseek-v3.1-maas", "max_tokens": 1}"#,
            404,
        ),
    ];
    for (payload, status) in cases {
        let r = h
            .client
            .post(h.url("/v1/messages"))
            .header("content-type", "application/json")
            .body(payload)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), status, "{payload}");
        let body: Value = r.json().await.unwrap();
        assert_eq!(body["type"], "error", "{payload}");
    }
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn count_tokens() {
    let h = common::start(common::settings()).await;
    Mock::given(method("POST"))
        .and(path(cpath("count-tokens:rawPredict")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"input_tokens": 7})))
        .mount(&h.upstream)
        .await;
    let r = post(
        &h,
        "/v1/messages/count_tokens",
        &json!({"model": "claude-opus-4-5", "messages": [{"role": "user", "content": "hi"}]}),
    )
    .await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.json::<Value>().await.unwrap(), json!({"input_tokens": 7}));
    let sent: Value = only_request(&h).await.body_json().unwrap();
    assert_eq!(sent["model"], "claude-opus-4-5@20251101");
    assert_eq!(sent["anthropic_version"], "vertex-2023-10-16");
}

#[tokio::test]
async fn count_tokens_upstream_error() {
    let h = common::start(common::settings()).await;
    mount(
        &h,
        ResponseTemplate::new(400).set_body_json(
            json!({"type": "error", "error": {"type": "invalid_request_error", "message": "bad"}}),
        ),
    )
    .await;
    let r = post(
        &h,
        "/anthropic/v1/messages/count_tokens",
        &json!({"model": "claude-opus-4-6", "messages": []}),
    )
    .await;
    assert_eq!(r.status(), 400);
    assert_eq!(r.json::<Value>().await.unwrap()["error"]["message"], "bad");
}

#[tokio::test]
async fn api_key_required_when_configured() {
    let mut s = common::settings();
    s.server.api_key = "secret".into();
    let h = common::start(s).await;
    mount(&h, ResponseTemplate::new(200).set_body_json(message())).await;
    let r = post(&h, "/v1/messages", &req()).await;
    assert_eq!(r.status(), 401);
    assert_eq!(
        r.json::<Value>().await.unwrap()["error"]["type"],
        "authentication_error"
    );
    for (name, value) in [("x-api-key", "secret"), ("authorization", "Bearer secret")] {
        let r = h
            .client
            .post(h.url("/v1/messages"))
            .header(name, value)
            .json(&req())
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "{name}");
    }
}
