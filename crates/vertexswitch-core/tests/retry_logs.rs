//! 上游繁忙重试的日志：每次重试一条 WARN（kind=upstream_retry），重试后成功一条 INFO（kind=upstream_retry_ok）。
//! 单独一个测试进程：tracing 的线程内订阅器在多个测试并行命中同一处日志时会漏收事件。

mod common;

use serde_json::{Value, json};
use tracing_subscriber::layer::SubscriberExt;
use vertexswitch_core::logs::LogLayer;
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
async fn gemini_429_is_retried_and_logged() {
    let h = common::start(common::settings()).await;
    // 测试运行时是单线程的，代理的任务也在这个线程上，日志都会进 h.state.logs
    let _guard = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(LogLayer::new(h.state.logs.clone())),
    );
    let ok = json!({
        "candidates": [{"content": {"role": "model", "parts": [{"text": "yo"}]}}],
        "usageMetadata": {"promptTokenCount": 1, "trafficType": "ON_DEMAND"},
    });
    mount_sequence(
        &h,
        busy(429),
        2,
        ResponseTemplate::new(200).set_body_json(ok.clone()),
    )
    .await;

    let r = h
        .client
        .post(h.url("/v1beta/models/gemini-3.5-flash:generateContent"))
        .json(&gemini_body())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.json::<Value>().await.unwrap(), ok);
    assert_eq!(h.upstream.received_requests().await.unwrap().len(), 3);

    let logs = h.state.logs.recent(20, |_| true);
    let retries: Vec<_> = logs
        .iter()
        .filter(|e| e.fields.get("kind").map(String::as_str) == Some("upstream_retry"))
        .collect();
    assert_eq!(retries.len(), 2, "{logs:?}");
    assert!(retries.iter().all(|e| e.level == "WARN"
        && e.fields["model"] == "gemini-3.5-flash"
        && e.fields["status"] == "429"));
    assert!(
        retries[0].message.contains("返回 429（繁忙）"),
        "{}",
        retries[0].message
    );
    let ok_log = logs
        .iter()
        .find(|e| e.fields.get("kind").map(String::as_str) == Some("upstream_retry_ok"))
        .unwrap_or_else(|| panic!("缺少 upstream_retry_ok：{logs:?}"));
    assert_eq!(ok_log.fields["retries"], "2");
    assert_eq!(ok_log.fields["model"], "gemini-3.5-flash");
}
