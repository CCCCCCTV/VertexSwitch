//! 界面切到英文后，核心的错误信息与日志也是英文；档位降级日志带 `kind` / `model` 字段。
//! 本文件单独一个测试进程，整份都在英文下运行，不影响其他测试的中文断言。

mod common;

use serde_json::{Value, json};
use tracing_subscriber::layer::SubscriberExt;
use vertexswitch_core::logs::{LogEntry, LogLayer};
use wiremock::matchers::method;
use wiremock::{Mock, ResponseTemplate};

fn english() {
    vertexswitch_core::i18n::use_english(true);
}

async fn post(h: &common::Harness, path: &str, body: &Value) -> (u16, Value) {
    let r = h.client.post(h.url(path)).json(body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap())
}

#[tokio::test]
async fn client_errors_are_english() {
    english();
    let h = common::start(common::settings()).await;
    let (status, body) = post(
        &h,
        "/v1/chat/completions",
        &json!({"model": "nope", "messages": [{"role": "user", "content": "hi"}]}),
    )
    .await;
    assert_eq!(status, 404);
    assert_eq!(body["error"]["message"], "Unknown model \"nope\"");

    let (status, body) = post(&h, "/v1/messages", &json!({"messages": []})).await;
    assert_eq!(status, 400);
    assert_eq!(body["error"]["message"], "model: missing model name");
}

#[tokio::test]
async fn tier_warnings_carry_kind_and_model() {
    english();
    let mut s = common::settings();
    s.gemini.tier = "flex".into();
    let h = common::start(s).await;
    // 测试运行时是单线程的，代理的任务也在这个线程上，日志都会进 h.state.logs
    let _guard = tracing::subscriber::set_default(
        tracing_subscriber::registry().with(LogLayer::new(h.state.logs.clone())),
    );
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{"content": {"role": "model", "parts": [{"text": "yo"}]}}],
            "usageMetadata": {"promptTokenCount": 1, "trafficType": "ON_DEMAND"},
        })))
        .mount(&h.upstream)
        .await;
    let body = json!({"contents": [{"role": "user", "parts": [{"text": "hi"}]}]});
    // 不支持 Flex 的模型：发请求前就改用 Standard
    post(&h, "/v1beta/models/gemini-4-argon:generateContent", &body).await;
    // 支持 Flex，但上游实际按 ON_DEMAND 处理
    post(&h, "/v1beta/models/gemini-3.5-flash:generateContent", &body).await;

    let warns = h.state.logs.recent(10, |e| e.level == "WARN");
    let of_kind = |kind: &str| -> &LogEntry {
        warns
            .iter()
            .find(|e| e.fields.get("kind").map(String::as_str) == Some(kind))
            .unwrap_or_else(|| panic!("缺少 {kind} 日志：{warns:?}"))
    };
    let fallback = of_kind("tier_fallback");
    assert_eq!(
        fallback.message,
        format!(
            "Model {} doesn't support Flex; using Standard instead",
            fallback.fields["model"]
        )
    );
    let mismatch = of_kind("tier_mismatch");
    assert!(mismatch.fields["model"].starts_with("gemini-3.5-flash"));
    assert_eq!(
        mismatch.message,
        "Requested the flex tier, but upstream served it as ON_DEMAND (expected ON_DEMAND_FLEX)"
    );
}
