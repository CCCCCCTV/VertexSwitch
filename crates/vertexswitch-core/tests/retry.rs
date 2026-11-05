//! 上游繁忙时自动重试（SPEC §4）：经完整代理走一遍，含流式请求与客户端断开。日志字段见 retry_logs.rs。

mod common;

use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use wiremock::matchers::method;
use wiremock::{Mock, ResponseTemplate};

fn gemini_body() -> Value {
    json!({"contents": [{"role": "user", "parts": [{"text": "hi"}]}]})
}

fn busy(status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({
        "error": {"code": status, "message": "Resource exhausted", "status": "RESOURCE_EXHAUSTED"}
    }))
}

/// 前 `times` 次返回 `first`，之后返回 `then`。
async fn mount_sequence(
    h: &common::Harness,
    first: ResponseTemplate,
    times: u64,
    then: ResponseTemplate,
) {
    Mock::given(method("POST"))
        .respond_with(first)
        .up_to_n_times(times)
        .with_priority(1)
        .mount(&h.upstream)
        .await;
    Mock::given(method("POST"))
        .respond_with(then)
        .mount(&h.upstream)
        .await;
}

#[tokio::test]
async fn stream_request_is_retried_before_anything_is_sent() {
    let h = common::start(common::settings()).await;
    let sse = "data: {\"usageMetadata\": {\"trafficType\": \"ON_DEMAND\"}}\r\n\r\n";
    mount_sequence(
        &h,
        busy(503),
        1,
        ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"),
    )
    .await;
    let r = h
        .client
        .post(h.url("/v1beta/models/gemini-3.5-flash:streamGenerateContent?alt=sse"))
        .json(&gemini_body())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.text().await.unwrap(), sse);
    assert_eq!(h.upstream.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn claude_overloaded_gives_the_client_the_last_error() {
    let mut s = common::settings();
    s.retry.busy_attempts = 2;
    let h = common::start(s).await;
    let overloaded =
        json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}});
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(529).set_body_json(overloaded.clone()))
        .mount(&h.upstream)
        .await;
    let r = h
        .client
        .post(h.url("/v1/messages"))
        .json(&json!({
            "model": "claude-haiku-4-5",
            "max_tokens": 16,
            "messages": [{"role": "user", "content": "hi"}],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 529);
    assert_eq!(r.json::<Value>().await.unwrap(), overloaded);
    assert_eq!(h.upstream.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn disabled_busy_retry_passes_429_straight_through() {
    let mut s = common::settings();
    s.retry.busy = false;
    let h = common::start(s).await;
    Mock::given(method("POST"))
        .respond_with(busy(429))
        .mount(&h.upstream)
        .await;
    let r = h
        .client
        .post(h.url("/v1beta/models/gemini-3.5-flash:generateContent"))
        .json(&gemini_body())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 429);
    assert_eq!(h.upstream.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn client_disconnect_stops_retrying() {
    let mut s = common::settings();
    s.retry.busy_attempts = 10;
    s.retry.busy_wait = "0.5 * 2^(n-1)".into();
    let h = common::start(s).await;
    Mock::given(method("POST"))
        .respond_with(busy(429))
        .mount(&h.upstream)
        .await;

    // 用原始 TCP 发请求，不等响应就断开
    let addr = h.base.trim_start_matches("http://").to_string();
    let mut socket = tokio::net::TcpStream::connect(&addr).await.unwrap();
    let body = gemini_body().to_string();
    let request = format!(
        "POST /v1beta/models/gemini-3.5-flash:generateContent HTTP/1.1\r\nhost: {addr}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(request.as_bytes()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    drop(socket);

    // 没有停的话：第 2 次约在 0.4~0.6 秒、第 3 次约在 1.1~1.9 秒发出
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let sent = h.upstream.received_requests().await.unwrap().len();
    assert!(sent <= 2, "客户端断开后仍在重试：上游收到 {sent} 次");
}
