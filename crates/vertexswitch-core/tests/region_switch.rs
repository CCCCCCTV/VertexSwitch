//! 区域切换（SPEC §9a）：经完整代理走一遍，假上游按地址里的 `/locations/{区域}/` 区分区域。
//! 状态机的细节（记录、停留、重置）见 `regions` 的单元测试。

mod common;

use std::time::Duration;

use reqwest::Method;
use serde_json::{Value, json};
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, ResponseTemplate};

const GEN: &str = "/v1beta/models/gemini-3.5-flash:generateContent";
const STREAM: &str = "/v1beta/models/gemini-3.5-flash:streamGenerateContent?alt=sse";

fn settings() -> vertexswitch_core::config::Settings {
    let mut s = common::settings();
    let rs = &mut s.gemini.region_switch;
    rs.enabled = true;
    rs.regions = vec!["global".into(), "us".into(), "eu".into()];
    rs.slow_first_byte_seconds = 0.0;
    rs.slow_tokens_per_second = 0.0;
    s
}

fn body() -> Value {
    json!({"contents": [{"role": "user", "parts": [{"text": "hi"}]}]})
}

fn ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "candidates": [{"content": {"role": "model", "parts": [{"text": "pong"}]}}],
        "usageMetadata": {"promptTokenCount": 4, "candidatesTokenCount": 2, "trafficType": "ON_DEMAND"},
    }))
}

fn stream_ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(
        "data: {\"candidates\": [{\"content\": {\"parts\": [{\"text\": \"pong\"}]}}]}\r\n\r\n",
        "text/event-stream",
    )
}

fn status(code: u16) -> ResponseTemplate {
    ResponseTemplate::new(code).set_body_json(json!({
        "error": {"code": code, "message": "nope", "status": "RESOURCE_EXHAUSTED"}
    }))
}

/// `region` 区域先返回 `first` 共 `times` 次（0 = 不用），之后返回 `then`。
async fn region(
    h: &common::Harness,
    region: &str,
    first: ResponseTemplate,
    times: u64,
    then: ResponseTemplate,
) {
    let path = format!("/locations/{region}/");
    if times > 0 {
        Mock::given(method("POST"))
            .and(path_regex(path.clone()))
            .respond_with(first)
            .up_to_n_times(times)
            .with_priority(1)
            .mount(&h.upstream)
            .await;
    }
    Mock::given(method("POST"))
        .and(path_regex(path))
        .respond_with(then)
        .mount(&h.upstream)
        .await;
}

/// 假上游依次收到的请求各发往哪个区域。
async fn regions_seen(h: &common::Harness) -> Vec<String> {
    h.upstream
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| {
            let path = r.url.path();
            let rest = path.split_once("/locations/").unwrap().1;
            rest.split('/').next().unwrap().to_owned()
        })
        .collect()
}

async fn send(h: &common::Harness, path: &str) -> (u16, String, Option<String>) {
    let r = h
        .client
        .post(h.url(path))
        .json(&body())
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    let tier = r
        .headers()
        .get("x-vertexswitch-tier")
        .map(|v| v.to_str().unwrap().to_owned());
    (status, r.text().await.unwrap(), tier)
}

async fn admin_json(h: &common::Harness, path: &str) -> Value {
    h.admin(Method::GET, path)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn busy_region_moves_this_request_and_later_ones() {
    let h = common::start(settings()).await;
    region(&h, "global", status(429), 100, ok()).await;
    region(&h, "us", ok(), 0, ok()).await;

    assert_eq!(send(&h, GEN).await.0, 200);
    assert_eq!(regions_seen(&h).await, ["global", "us"]);
    // 之后的请求直接发往 us
    assert_eq!(send(&h, GEN).await.0, 200);
    assert_eq!(regions_seen(&h).await, ["global", "us", "us"]);

    let status = admin_json(&h, "/admin/api/status").await;
    let switch = &status["gemini_region_switch"];
    assert_eq!(switch["home"], "global");
    assert_eq!(switch["models"][0]["model"], "gemini-3.5-flash");
    assert_eq!(switch["models"][0]["region"], "us");
    let records = admin_json(&h, "/admin/api/requests").await;
    assert_eq!(records[0]["region"], "us");
}

#[tokio::test]
async fn flex_outside_global_goes_out_as_standard() {
    let mut s = settings();
    s.gemini.tier = "flex".into();
    let h = common::start(s).await;
    region(&h, "global", status(429), 100, ok()).await;
    region(&h, "us", ok(), 0, ok()).await;

    let (code, _, tier) = send(&h, GEN).await;
    assert_eq!(code, 200);
    assert_eq!(tier.as_deref(), Some("standard"));
    let sent = h.upstream.received_requests().await.unwrap();
    let shared = |i: usize| {
        sent[i]
            .headers
            .get("x-vertex-ai-llm-shared-request-type")
            .map(|v| v.to_str().unwrap().to_owned())
    };
    assert_eq!(shared(0).as_deref(), Some("flex"));
    assert_eq!(shared(1), None);
}

#[tokio::test]
async fn every_region_busy_falls_back_to_busy_retry() {
    let mut s = settings();
    s.retry.busy_attempts = 1;
    let h = common::start(s).await;
    for r in ["global", "us", "eu"] {
        region(&h, r, status(429), 0, status(429)).await;
    }
    assert_eq!(send(&h, GEN).await.0, 429);
    // 一圈都繁忙 → 按繁忙重试等一次、原地重发 → 再来一圈直到都试过
    assert_eq!(
        regions_seen(&h).await,
        ["global", "us", "eu", "eu", "global", "us"]
    );
}

#[tokio::test]
async fn missing_model_in_another_region_is_skipped() {
    let h = common::start(settings()).await;
    region(&h, "global", status(429), 100, ok()).await;
    region(&h, "us", status(404), 0, status(404)).await;
    region(&h, "eu", ok(), 0, ok()).await;
    assert_eq!(send(&h, GEN).await.0, 200);
    assert_eq!(regions_seen(&h).await, ["global", "us", "eu"]);
}

#[tokio::test]
async fn missing_model_at_home_goes_to_the_client() {
    let h = common::start(settings()).await;
    region(&h, "global", status(404), 0, status(404)).await;
    assert_eq!(send(&h, GEN).await.0, 404);
    assert_eq!(regions_seen(&h).await, ["global"]);
}

#[tokio::test]
async fn disabled_switch_stays_home() {
    let mut s = settings();
    s.gemini.region_switch.enabled = false;
    s.retry.busy_attempts = 1;
    let h = common::start(s).await;
    region(&h, "global", status(429), 0, status(429)).await;
    region(&h, "us", ok(), 0, ok()).await;
    assert_eq!(send(&h, GEN).await.0, 429);
    assert_eq!(regions_seen(&h).await, ["global", "global"]);
}

#[tokio::test]
async fn slow_stream_moves_later_requests_and_a_slower_region_switches_back() {
    let mut s = settings();
    s.gemini.region_switch.slow_first_byte_seconds = 0.2;
    let h = common::start(s).await;
    let slow = |ms| stream_ok().set_delay(Duration::from_millis(ms));
    region(&h, "global", stream_ok(), 0, slow(400)).await;
    region(&h, "us", stream_ok(), 0, slow(800)).await;

    // global 首字约 0.4 秒，超过 0.2 秒：本次照常完成，之后的请求改发 us
    let (code, text, _) = send(&h, STREAM).await;
    assert_eq!((code, text.contains("pong")), (200, true));
    tokio::time::sleep(Duration::from_millis(50)).await;
    send(&h, STREAM).await;
    // us 更慢：回到记录的 global，并停在那里
    tokio::time::sleep(Duration::from_millis(50)).await;
    send(&h, STREAM).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    send(&h, STREAM).await;
    assert_eq!(regions_seen(&h).await, ["global", "us", "global", "global"]);
}

#[tokio::test]
async fn non_stream_requests_do_not_judge_speed() {
    let mut s = settings();
    s.gemini.region_switch.slow_first_byte_seconds = 0.2;
    let h = common::start(s).await;
    region(
        &h,
        "global",
        ok(),
        0,
        ok().set_delay(Duration::from_millis(400)),
    )
    .await;
    send(&h, GEN).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    send(&h, GEN).await;
    assert_eq!(regions_seen(&h).await, ["global", "global"]);
}

#[tokio::test]
async fn returns_home_after_the_reset_interval() {
    let mut s = settings();
    s.gemini.region_switch.reset_minutes = 0.003; // 约 0.18 秒
    let h = common::start(s).await;
    region(&h, "global", status(429), 1, ok()).await;
    region(&h, "us", ok(), 0, ok()).await;
    send(&h, GEN).await;
    send(&h, GEN).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    send(&h, GEN).await;
    assert_eq!(regions_seen(&h).await, ["global", "us", "us", "global"]);
}

#[tokio::test]
async fn openai_format_requests_switch_too() {
    let h = common::start(settings()).await;
    let reply = ResponseTemplate::new(200).set_body_json(json!({
        "id": "x", "object": "chat.completion", "model": "google/gemini-3.5-flash",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": "pong"}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
    }));
    region(&h, "global", status(429), 100, reply.clone()).await;
    region(&h, "us", reply.clone(), 0, reply).await;
    let r = h
        .client
        .post(h.url("/v1/chat/completions"))
        .json(
            &json!({"model": "gemini-3.5-flash", "messages": [{"role": "user", "content": "hi"}]}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(regions_seen(&h).await, ["global", "us"]);
    let sent = h.upstream.received_requests().await.unwrap();
    assert!(
        sent[1]
            .url
            .path()
            .ends_with("/endpoints/openapi/chat/completions")
    );
}
