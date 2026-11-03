//! 冒烟测试：健康检查、未知路由、鉴权。

mod common;

use serde_json::Value;

#[tokio::test]
async fn health_reports_token_and_project() {
    let h = common::start(common::settings()).await;
    let r = h.client.get(h.url("/health")).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], vertexswitch_core::VERSION);
    assert_eq!(body["project_id"], "proj");
    assert_eq!(body["gemini_tier"], "standard");
}

#[tokio::test]
async fn unknown_route_uses_protocol_error_format() {
    let h = common::start(common::settings()).await;
    let r = h.client.get(h.url("/v1/nope")).send().await.unwrap();
    assert_eq!(r.status(), 404);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["error"]["type"], "not_found_error");

    let r = h.client.get(h.url("/v1beta/nope")).send().await.unwrap();
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["error"]["status"], "NOT_FOUND");
}

#[tokio::test]
async fn wrong_method_is_json_too() {
    let h = common::start(common::settings()).await;
    // 只接受 POST 的路径用 GET 访问
    let r = h
        .client
        .get(h.url("/v1/chat/completions"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 405);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["error"]["code"], 405);

    let r = h.client.get(h.url("/v1/messages")).send().await.unwrap();
    assert_eq!(r.status(), 405);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["type"], "error");
}

#[tokio::test]
async fn api_key_variants() {
    let mut s = common::settings();
    s.server.api_key = "secret".into();
    let h = common::start(s).await;
    let list = h.url("/v1beta/models");
    assert_eq!(h.client.get(&list).send().await.unwrap().status(), 401);
    assert_eq!(
        h.client
            .get(&list)
            .header("x-api-key", "wrong")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    for (name, value) in [
        ("x-api-key", "secret"),
        ("x-goog-api-key", "secret"),
        ("authorization", "Bearer secret"),
    ] {
        let r = h
            .client
            .get(&list)
            .header(name, value)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "{name}");
    }
    let r = h
        .client
        .get(format!("{list}?key=secret"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    // /health 不鉴权
    assert_eq!(
        h.client
            .get(h.url("/health"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
}
