//! R0 冒烟测试：核心能起服务；wiremock 假上游可用（后续所有上游测试都基于它，不连真实 Vertex）。

use serde_json::{Value, json};
use tokio::net::TcpListener;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn spawn_core() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(vertexswitch_core::serve(listener));
    format!("http://{addr}")
}

#[tokio::test]
async fn health_reports_version() {
    let base = spawn_core().await;
    let body: Value = reqwest::get(format!("{base}/health"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["version"], vertexswitch_core::VERSION);
}

#[tokio::test]
async fn wiremock_fake_upstream_works() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/x:generateContent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .expect(1)
        .mount(&upstream)
        .await;

    let body: Value = reqwest::Client::new()
        .post(format!("{}/v1/x:generateContent", upstream.uri()))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body, json!({ "ok": true }));
}
