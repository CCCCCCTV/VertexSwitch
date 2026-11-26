//! 换凭证（SPEC §9b）：经完整代理走一遍，假上游按地址里的 `/projects/{项目}/` 区分凭证。
//! 第一个凭证是 tok / proj，备用凭证是 tok-b / proj-b、tok-c / proj-c。状态机细节见 `credentials` 的单元测试。

mod common;

use std::time::Duration;

use reqwest::Method;
use serde_json::{Value, json};
use vertexswitch_core::auth::{ServiceAccountAuth, TokenSource};
use wiremock::matchers::{header, method, path_regex};
use wiremock::{Mock, ResponseTemplate};

const GEN: &str = "/v1beta/models/gemini-3.5-flash:generateContent";

fn backup(name: &str) -> TokenSource {
    TokenSource::fixed(&format!("tok-{name}"), &format!("proj-{name}"))
}

/// 密钥文件不存在：取 token 一定失败（不联网）。
fn broken() -> TokenSource {
    TokenSource::ServiceAccount(Box::new(ServiceAccountAuth::new(
        Some("no-such-key.json".into()),
        "",
        reqwest::Client::new(),
    )))
}

fn ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "candidates": [{"content": {"role": "model", "parts": [{"text": "pong"}]}}],
        "usageMetadata": {"promptTokenCount": 4, "candidatesTokenCount": 2},
    }))
}

fn status(code: u16) -> ResponseTemplate {
    ResponseTemplate::new(code).set_body_json(json!({
        "error": {"code": code, "message": "nope", "status": "RESOURCE_EXHAUSTED"}
    }))
}

/// `project` 项目先返回 `first` 共 `times` 次（0 = 不用），之后返回 `then`。
async fn project(
    h: &common::Harness,
    project: &str,
    first: ResponseTemplate,
    times: u64,
    then: ResponseTemplate,
) {
    let path = format!("/projects/{project}/");
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

/// 假上游依次收到的请求：(项目, 区域)。
async fn seen(h: &common::Harness) -> Vec<(String, String)> {
    h.upstream
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| {
            let path = r.url.path();
            let after = |key: &str| {
                path.split_once(key)
                    .unwrap()
                    .1
                    .split('/')
                    .next()
                    .unwrap()
                    .to_owned()
            };
            (after("/projects/"), after("/locations/"))
        })
        .collect()
}

fn projects(seen: &[(String, String)]) -> Vec<&str> {
    seen.iter().map(|(p, _)| p.as_str()).collect()
}

async fn send(h: &common::Harness) -> u16 {
    h.client
        .post(h.url(GEN))
        .json(&json!({"contents": [{"role": "user", "parts": [{"text": "hi"}]}]}))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
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
async fn busy_first_credential_moves_to_the_backup() {
    let h = common::start_with_backups(common::settings(), vec![backup("b")]).await;
    project(&h, "proj", status(429), 0, status(429)).await;
    Mock::given(method("POST"))
        .and(path_regex("/projects/proj-b/"))
        .and(header("authorization", "Bearer tok-b"))
        .respond_with(ok())
        .mount(&h.upstream)
        .await;

    assert_eq!(send(&h).await, 200);
    assert_eq!(projects(&seen(&h).await), ["proj", "proj-b"]);
    // 之后的请求直接用备用凭证
    assert_eq!(send(&h).await, 200);
    assert_eq!(projects(&seen(&h).await), ["proj", "proj-b", "proj-b"]);

    let status = admin_json(&h, "/admin/api/status").await;
    let creds = &status["credentials"];
    assert_eq!(creds["list"][0]["project"], "proj");
    assert_eq!(creds["list"][1]["project"], "proj-b");
    assert_eq!(creds["models"][0]["model"], "gemini-3.5-flash");
    assert_eq!(creds["models"][0]["credential"], 2);
    let records = admin_json(&h, "/admin/api/requests").await;
    assert_eq!(records[0]["credential"], 2);
}

#[tokio::test]
async fn denied_credential_switches_right_away() {
    let h = common::start_with_backups(common::settings(), vec![backup("b")]).await;
    project(&h, "proj", status(403), 0, status(403)).await;
    project(&h, "proj-b", ok(), 0, ok()).await;
    assert_eq!(send(&h).await, 200);
    assert_eq!(projects(&seen(&h).await), ["proj", "proj-b"]);
}

#[tokio::test]
async fn regions_are_tried_before_credentials() {
    let mut s = common::settings();
    s.gemini.region_switch.enabled = true;
    s.gemini.region_switch.slow_first_byte_seconds = 0.0;
    s.gemini.region_switch.slow_tokens_per_second = 0.0;
    let h = common::start_with_backups(s, vec![backup("b")]).await;
    project(&h, "proj", status(429), 0, status(429)).await;
    project(&h, "proj-b", ok(), 0, ok()).await;
    assert_eq!(send(&h).await, 200);
    let pair = |p: &str, r: &str| (p.to_owned(), r.to_owned());
    // 第一个凭证在 global / us / eu 都繁忙，再换备用凭证（从这个模型当前的区域 eu 开始）
    assert_eq!(
        seen(&h).await,
        [
            pair("proj", "global"),
            pair("proj", "us"),
            pair("proj", "eu"),
            pair("proj-b", "eu")
        ]
    );
}

#[tokio::test]
async fn every_credential_busy_falls_back_to_busy_retry() {
    let mut s = common::settings();
    s.retry.busy_attempts = 1;
    let h = common::start_with_backups(s, vec![backup("b")]).await;
    project(&h, "proj", status(429), 0, status(429)).await;
    project(&h, "proj-b", status(429), 0, status(429)).await;
    assert_eq!(send(&h).await, 429);
    // 一圈都繁忙 → 按繁忙重试等一次、原地重发 → 再换一圈直到都试过
    assert_eq!(
        projects(&seen(&h).await),
        ["proj", "proj-b", "proj-b", "proj"]
    );
}

#[tokio::test]
async fn broken_backup_is_skipped() {
    let h = common::start_with_backups(common::settings(), vec![broken(), backup("c")]).await;
    project(&h, "proj", status(403), 0, status(403)).await;
    project(&h, "proj-c", ok(), 0, ok()).await;
    assert_eq!(send(&h).await, 200);
    assert_eq!(projects(&seen(&h).await), ["proj", "proj-c"]);
    let status = admin_json(&h, "/admin/api/status").await;
    let creds = &status["credentials"];
    assert_eq!(creds["models"][0]["credential"], 3);
    assert_eq!(creds["models"][0]["broken"], json!([2]));
    assert!(creds["list"][1]["error"].as_str().is_some());
}

#[tokio::test]
async fn broken_backup_shows_up_before_any_request() {
    let h = common::start_with_backups(common::settings(), vec![broken(), backup("c")]).await;
    // 后台刷新也提前取备用凭证的 token：不用等请求换过去，状态里就能看到哪一个坏了
    h.state.start_refresher();
    let mut list = Value::Null;
    for _ in 0..100 {
        list = admin_json(&h, "/admin/api/status").await["credentials"]["list"].clone();
        if !list[1]["error"].is_null() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        list[1]["error"]
            .as_str()
            .unwrap()
            .contains("no-such-key.json"),
        "{list}"
    );
    assert!(list[0]["error"].is_null());
    assert!(list[2]["error"].is_null());
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn only_credential_failing_still_reports_the_error() {
    let h = common::start_with_backups(common::settings(), vec![broken()]).await;
    project(&h, "proj", status(403), 0, status(403)).await;
    // 备用凭证取不到 token：没有可换的，把第一个凭证的 403 交给客户端
    assert_eq!(send(&h).await, 403);
    assert_eq!(projects(&seen(&h).await), ["proj"]);
}

#[tokio::test]
async fn claude_request_moves_to_a_project_that_has_claude() {
    let h = common::start_with_backups(common::settings(), vec![backup("b")]).await;
    project(&h, "proj", status(404), 0, status(404)).await;
    project(
        &h,
        "proj-b",
        ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-sonnet-4-6",
            "content": [{"type": "text", "text": "pong"}], "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 1},
        })),
        0,
        ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-sonnet-4-6",
            "content": [{"type": "text", "text": "pong"}], "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 1},
        })),
    )
    .await;
    let r = h
        .client
        .post(h.url("/v1/messages"))
        .json(&json!({
            "model": "claude-sonnet-4-6", "max_tokens": 10,
            "messages": [{"role": "user", "content": "hi"}],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let sent = h.upstream.received_requests().await.unwrap();
    assert_eq!(sent.len(), 2);
    assert!(sent[1].url.path().contains("/projects/proj-b/"));
    assert!(sent[1].url.path().contains("/publishers/anthropic/"));
}

#[tokio::test]
async fn returns_to_the_first_credential_after_the_interval() {
    let mut s = common::settings();
    s.google.credential_reset_minutes = 0.003; // 约 0.18 秒
    let h = common::start_with_backups(s, vec![backup("b")]).await;
    project(&h, "proj", status(429), 1, ok()).await;
    project(&h, "proj-b", ok(), 0, ok()).await;
    send(&h).await;
    send(&h).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    send(&h).await;
    assert_eq!(
        projects(&seen(&h).await),
        ["proj", "proj-b", "proj-b", "proj"]
    );
}
