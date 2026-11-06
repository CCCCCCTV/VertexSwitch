//! 管理接口、请求记录与用量、指标、配置与模型表热加载（R4）。全部使用本地假上游。

mod common;

use std::time::Duration;

use common::Harness;
use reqwest::Method;
use serde_json::{Value, json};
use wiremock::matchers::method;
use wiremock::{Mock, ResponseTemplate};

const SHARED: &str = "x-vertex-ai-llm-shared-request-type";

fn gemini_reply() -> Value {
    json!({
        "candidates": [{"content": {"role": "model", "parts": [{"text": "pong"}]}}],
        "usageMetadata": {"promptTokenCount": 4, "candidatesTokenCount": 2, "trafficType": "ON_DEMAND_FLEX"},
    })
}

fn gen_body() -> Value {
    json!({"contents": [{"role": "user", "parts": [{"text": "hi"}]}]})
}

async fn mount(h: &Harness, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .respond_with(response)
        .mount(&h.upstream)
        .await;
}

async fn json(r: reqwest::RequestBuilder) -> (u16, Value) {
    let r = r.send().await.unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}

/// 轮询直到条件成立（热加载每 2 秒检查一次文件）。
async fn eventually<F, Fut>(mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..60 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("等待超时");
}

/// 等到第一条请求记录出现（流式记录在响应体结束后才落定）。
async fn first_record(h: &Harness) -> Value {
    for _ in 0..60 {
        let (_, records) = json(h.admin(Method::GET, "/admin/api/requests")).await;
        if let Some(first) = records.as_array().and_then(|r| r.first()) {
            return first.clone();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("没有请求记录");
}

#[tokio::test]
async fn admin_requires_token() {
    let h = common::start(common::settings()).await;
    let (status, body) = json(h.client.get(h.url("/admin/api/status"))).await;
    assert_eq!(status, 401);
    assert!(body["error"].is_object());
    let (status, _) = json(
        h.client
            .get(h.url("/admin/api/status"))
            .bearer_auth("wrong"),
    )
    .await;
    assert_eq!(status, 401);
    // 代理的 API key 不能当管理 token 用
    let mut s = common::settings();
    s.server.api_key = "proxy-key".into();
    let h2 = common::start(s).await;
    let (status, _) = json(
        h2.client
            .get(h2.url("/admin/api/status"))
            .bearer_auth("proxy-key"),
    )
    .await;
    assert_eq!(status, 401);

    let (status, body) = json(h.admin(Method::GET, "/admin/api/status")).await;
    assert_eq!(status, 200);
    assert_eq!(body["status"], "ok");
    assert_eq!(body["project_id"], "proj");
    assert!(body["models"]["gemini"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn admin_rejects_non_loopback_peer() {
    use axum::extract::ConnectInfo;
    use tower::ServiceExt;
    let h = common::start(common::settings()).await;
    let mut request = axum::http::Request::get("/admin/api/status")
        .header("authorization", format!("Bearer {}", h.state.admin_token))
        .body(axum::body::Body::empty())
        .unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(std::net::SocketAddr::from((
            [10, 0, 0, 5],
            4000,
        ))));
    let response = vertexswitch_core::router(h.state.clone())
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn patch_config_applies_immediately_and_writes_file() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let h = common::start_with_paths(common::settings(), Some(config.clone()), None).await;
    mount(&h, ResponseTemplate::new(200).set_body_json(gemini_reply())).await;

    let (status, body) = json(
        h.admin(Method::PATCH, "/admin/api/config")
            .json(&json!({"gemini": {"tier": "flex"}})),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["settings"]["gemini"]["tier"], "flex");
    assert_eq!(body["restart_required"], false);
    // 其他字段保持默认
    assert_eq!(body["settings"]["gemini"]["flex_server_timeout"], 1800);
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .contains("tier = \"flex\"")
    );

    // 新配置对下一个请求立即生效
    h.client
        .post(h.url("/v1beta/models/gemini-3.5-flash:generateContent"))
        .json(&gen_body())
        .send()
        .await
        .unwrap();
    let up = &h.upstream.received_requests().await.unwrap()[0];
    assert_eq!(up.headers[SHARED], "flex");

    let (_, current) = json(h.admin(Method::GET, "/admin/api/config")).await;
    assert_eq!(current["gemini"]["tier"], "flex");
}

#[tokio::test]
async fn config_changes_that_need_restart_or_are_invalid() {
    let h = common::start(common::settings()).await;
    let (status, body) = json(
        h.admin(Method::PATCH, "/admin/api/config")
            .json(&json!({"server": {"port": 9999}})),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["restart_required"], true);

    let (status, body) = json(
        h.admin(Method::PATCH, "/admin/api/config")
            .json(&json!({"server": {"port": "not a number"}})),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    // 无效修改不影响当前配置
    let (_, current) = json(h.admin(Method::GET, "/admin/api/config")).await;
    assert_eq!(current["server"]["port"], 9999);

    // 繁忙重试的等待算式写错：400 并说明原因；写对则生效
    let (status, body) = json(
        h.admin(Method::PATCH, "/admin/api/config")
            .json(&json!({"retry": {"busy_wait": "3n"}})),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("retry.busy_wait") && message.contains("3*n"),
        "{body}"
    );
    let (status, _) = json(
        h.admin(Method::PATCH, "/admin/api/config")
            .json(&json!({"retry": {"busy_wait": "min(30, 3*n)"}})),
    )
    .await;
    assert_eq!(status, 200);
    let (_, current) = json(h.admin(Method::GET, "/admin/api/config")).await;
    assert_eq!(current["retry"]["busy_wait"], "min(30, 3*n)");
}

#[tokio::test]
async fn api_key_change_protects_proxy_routes() {
    let h = common::start(common::settings()).await;
    assert_eq!(
        h.client
            .get(h.url("/v1/models"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    json(
        h.admin(Method::PATCH, "/admin/api/config")
            .json(&json!({"server": {"api_key": "k2"}})),
    )
    .await;
    assert_eq!(
        h.client
            .get(h.url("/v1/models"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        h.client
            .get(h.url("/v1/models"))
            .header("x-api-key", "k2")
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
}

#[tokio::test]
async fn config_file_hot_reload() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "[gemini]\ntier = \"standard\"\n").unwrap();
    let h = common::start_with_paths(common::settings(), Some(config.clone()), None).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    // 手动编辑配置文件
    std::fs::write(&config, "[gemini]\ntier = \"priority\"\n").unwrap();
    eventually(|| async { h.state.runtime().settings.gemini.tier == "priority" }).await;

    // 写坏了：保留当前配置
    std::fs::write(&config, "[gemini\n").unwrap();
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(h.state.runtime().settings.gemini.tier, "priority");
}

/// 手改配置文件里的端口：要通知负责监听的外壳重启监听；改别的不通知。
#[tokio::test]
async fn port_change_in_config_file_asks_for_relisten() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "[gemini]\ntier = \"standard\"\n").unwrap();
    let h = common::start_with_paths(common::settings(), Some(config.clone()), None).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    std::fs::write(&config, "[gemini]\ntier = \"priority\"\n").unwrap();
    eventually(|| async { h.state.runtime().settings.gemini.tier == "priority" }).await;
    let quiet = tokio::time::timeout(
        Duration::from_millis(300),
        h.state.listen_changed.notified(),
    )
    .await;
    assert!(quiet.is_err(), "只改档位不应要求重新监听");

    std::fs::write(
        &config,
        "[gemini]\ntier = \"priority\"\n\n[server]\nport = 18234\n",
    )
    .unwrap();
    tokio::time::timeout(Duration::from_secs(8), h.state.listen_changed.notified())
        .await
        .expect("改端口后应通知重新监听");
    assert_eq!(h.state.runtime().settings.server.port, 18234);
}

#[tokio::test]
async fn models_hot_reload_and_admin_view() {
    let dir = tempfile::tempdir().unwrap();
    let models = dir.path().join("models.toml");
    let h = common::start_with_paths(common::settings(), None, Some(models.clone())).await;
    let (_, view) = json(h.admin(Method::GET, "/admin/api/models")).await;
    assert_eq!(view["source"], "builtin");
    assert_eq!(view["gemini"]["gemini-3-flash"], "gemini-3-flash-preview");
    // 保持 models.toml 的声明顺序
    let first = view["claude"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    assert_eq!(first, "claude-fable-5-1");

    std::fs::write(&models, "[gemini]\n\"fast\" = \"gemini-3.5-flash-lite\"\n").unwrap();
    eventually(|| async {
        h.state
            .runtime()
            .registry()
            .entries(vertexswitch_core::registry::Backend::Gemini)
            .iter()
            .any(|(k, _)| k == "fast")
    })
    .await;
    let (_, view) = json(h.admin(Method::GET, "/admin/api/models")).await;
    assert_eq!(view["gemini"], json!({"fast": "gemini-3.5-flash-lite"}));

    // 手动重载：写坏后返回 400，保留原表
    std::fs::write(&models, "[gemini]\nfast = 1\n").unwrap();
    let (status, _) = json(h.admin(Method::POST, "/admin/api/models/reload")).await;
    assert_eq!(status, 400);
    assert_eq!(h.state.runtime().registry().len(), 1);
}

#[tokio::test]
async fn request_records_with_usage_and_tier() {
    let mut s = common::settings();
    s.gemini.tier = "flex".into();
    let h = common::start(s).await;
    mount(&h, ResponseTemplate::new(200).set_body_json(gemini_reply())).await;
    h.client
        .post(h.url("/v1beta/models/gemini-3-flash:generateContent"))
        .json(&gen_body())
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    h.client
        .post(h.url("/v1/messages"))
        .json(&json!({"model": "gpt-4o", "messages": []}))
        .send()
        .await
        .unwrap();

    let (_, records) = json(h.admin(Method::GET, "/admin/api/requests")).await;
    let records = records.as_array().unwrap();
    assert_eq!(records.len(), 2, "{records:?}");
    let ok = &records[0];
    assert_eq!(ok["model"], "gemini-3-flash-preview");
    assert_eq!(ok["backend"], "gemini");
    assert_eq!(ok["tier"], "flex");
    assert_eq!(ok["protocol"], "gemini");
    assert_eq!(ok["status"], 200);
    assert_eq!(ok["outcome"], "ok");
    assert_eq!(ok["input_tokens"], 4);
    assert_eq!(ok["output_tokens"], 2);

    let bad = &records[1];
    assert_eq!(bad["status"], 404);
    assert_eq!(bad["outcome"], "error");
    assert!(bad["error"].as_str().unwrap().contains("gpt-4o"));

    // 增量拉取
    let since = ok["seq"].as_u64().unwrap();
    let (_, newer) =
        json(h.admin(Method::GET, &format!("/admin/api/requests?since={since}"))).await;
    assert_eq!(newer.as_array().unwrap().len(), 1);

    // 管理接口自身不记录
    json(h.admin(Method::GET, "/admin/api/status")).await;
    let (_, again) = json(h.admin(Method::GET, "/admin/api/requests")).await;
    assert_eq!(again.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn stream_records_finalize_at_stream_end() {
    let h = common::start(common::settings()).await;
    let sse = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":7,\"output_tokens\":1}}}\n\n\
               event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":11}}\n\n";
    mount(
        &h,
        ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"),
    )
    .await;
    h.client
        .post(h.url("/v1/messages"))
        .json(&json!({"model": "claude-opus-4-6", "max_tokens": 5, "stream": true, "messages": [{"role": "user", "content": "x"}]}))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let record = first_record(&h).await;
    assert_eq!(record["stream"], true);
    assert_eq!(record["backend"], "claude");
    assert_eq!(record["outcome"], "ok");
    assert_eq!(record["input_tokens"], 7);
    assert_eq!(record["output_tokens"], 11);
}

#[tokio::test]
async fn stream_error_is_recorded_as_stream_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let broken = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 16384];
        let _ = socket.read(&mut buf).await;
        let part = "event: message_start\ndata: {}\n\n";
        let head = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n{:x}\r\n{part}\r\n",
            part.len()
        );
        let _ = socket.write_all(head.as_bytes()).await;
    });
    let mut s = common::settings();
    s.google.api_base_override = Some(broken);
    let h = common::start_with_override(s).await;
    h.client
        .post(h.url("/v1/messages"))
        .json(&json!({"model": "claude-opus-4-6", "max_tokens": 5, "stream": true, "messages": []}))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let record = first_record(&h).await;
    assert_eq!(record["status"], 200);
    assert_eq!(record["outcome"], "stream_error");
    assert!(record["error"].as_str().unwrap().contains("error"));
}

/// 首字耗时 = 第一段响应体发出的时刻：不是响应头到达的时刻，也不是整个流结束的时刻。
#[tokio::test]
async fn first_byte_time_is_recorded_for_streams() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let slow = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 16384];
        let _ = socket.read(&mut buf).await;
        let chunk = |s: &str| format!("{:x}\r\n{s}\r\n", s.len());
        let _ = socket
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n")
            .await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        let _ = socket
            .write_all(chunk("event: ping\ndata: {}\n\n").as_bytes())
            .await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = socket
            .write_all(chunk("event: message_stop\ndata: {}\n\n").as_bytes())
            .await;
        let _ = socket.write_all(b"0\r\n\r\n").await;
    });
    let mut s = common::settings();
    s.google.api_base_override = Some(slow);
    let h = common::start_with_override(s).await;
    h.client
        .post(h.url("/v1/messages"))
        .json(&json!({"model": "claude-opus-4-6", "max_tokens": 5, "stream": true, "messages": []}))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let record = first_record(&h).await;
    let first = record["first_byte_ms"].as_u64().expect("应有首字耗时");
    let total = record["duration_ms"].as_u64().unwrap();
    assert!((350..800).contains(&first), "首字 {first}ms：{record}");
    assert!(total >= first + 400, "总耗时 {total}ms，首字 {first}ms");

    // 上游直接报错：没发出任何内容，首字耗时为空
    let h = common::start(common::settings()).await;
    mount(
        &h,
        ResponseTemplate::new(429)
            .set_body_json(json!({"error": {"code": 429, "message": "busy"}})),
    )
    .await;
    h.client
        .post(h.url("/v1/messages"))
        .json(&json!({"model": "claude-opus-4-6", "max_tokens": 5, "messages": []}))
        .send()
        .await
        .unwrap();
    let record = first_record(&h).await;
    assert_eq!(record["status"], 429);
    assert!(record["first_byte_ms"].is_null(), "{record}");
}

/// OpenAI 入口转成 Claude / Gemini 原生后，上游在流里报错：记录同样要落成 stream_error。
#[tokio::test]
async fn translated_openai_stream_errors_are_recorded() {
    for (model, sse) in [
        (
            "claude-opus-4-6",
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"usage\":{\"input_tokens\":1}}}\n\n\
             event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
        ),
        (
            "gemini-3.5-flash",
            "data: {\"error\":{\"code\":429,\"message\":\"Resource exhausted\",\"status\":\"RESOURCE_EXHAUSTED\"}}\n\n",
        ),
    ] {
        let mut s = common::settings();
        s.gemini.openai_native = "always".into();
        let h = common::start(s).await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
            .mount(&h.upstream)
            .await;
        let text = h
            .client
            .post(h.url("/v1/chat/completions"))
            .json(&json!({"model": model, "stream": true, "messages": [{"role": "user", "content": "hi"}]}))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(text.contains("\"error\""), "{model}: {text}");
        let record = first_record(&h).await;
        assert_eq!(record["status"], 200, "{model}");
        assert_eq!(record["outcome"], "stream_error", "{model}: {record}");
        assert!(
            record["error"].as_str().unwrap().contains("error"),
            "{model}"
        );
    }
}

#[tokio::test]
async fn logs_query_and_stream() {
    let h = common::start(common::settings()).await;
    h.state.logs.record("WARN", "test", "第一条".into());
    let (_, logs) = json(h.admin(Method::GET, "/admin/api/logs")).await;
    let first = logs.as_array().unwrap().last().unwrap().clone();
    assert_eq!(first["message"], "第一条");
    assert_eq!(first["level"], "WARN");

    // SSE：连上后再写一条，应能收到
    let mut stream = h
        .admin(Method::GET, "/admin/api/logs/stream")
        .send()
        .await
        .unwrap();
    assert!(
        stream.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    h.state.logs.record("INFO", "test", "实时".into());
    let mut received = String::new();
    while !received.contains("实时") {
        let chunk = tokio::time::timeout(Duration::from_secs(5), stream.chunk())
            .await
            .expect("应收到实时日志")
            .unwrap()
            .unwrap();
        received.push_str(&String::from_utf8_lossy(&chunk));
    }
    let line = received.lines().find(|l| l.contains("实时")).unwrap();
    let entry: Value = serde_json::from_str(line.trim_start_matches("data:").trim()).unwrap();
    assert_eq!(entry["level"], "INFO");

    let since = first["seq"].as_u64().unwrap();
    let (_, newer) = json(h.admin(Method::GET, &format!("/admin/api/logs?since={since}"))).await;
    assert_eq!(newer[0]["message"], "实时");
}

#[tokio::test]
async fn metrics_prometheus_and_summary() {
    let mut s = common::settings();
    s.server.api_key = "k".into();
    let h = common::start(s).await;
    mount(&h, ResponseTemplate::new(200).set_body_json(gemini_reply())).await;
    h.client
        .post(h.url("/v1beta/models/gemini-3.5-flash:generateContent"))
        .header("x-goog-api-key", "k")
        .json(&gen_body())
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    // /metrics 也要鉴权（旧项目的缺陷）
    assert_eq!(
        h.client
            .get(h.url("/metrics"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let text = h
        .client
        .get(h.url("/metrics"))
        .header("x-api-key", "k")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(text.contains(
        "vertexswitch_requests_total{protocol=\"gemini\",backend=\"gemini\",status=\"200\"} 1"
    ));
    assert!(text.contains(
        "vertexswitch_tokens_total{protocol=\"gemini\",backend=\"gemini\",kind=\"input\"} 4"
    ));

    let (_, summary) = json(h.admin(Method::GET, "/admin/api/metrics")).await;
    assert_eq!(summary["totals"][0]["requests"], 1);
    assert_eq!(summary["in_flight"], 0);
}

#[tokio::test]
async fn test_request_endpoint() {
    let mut s = common::settings();
    s.server.api_key = "k".into();
    let h = common::start(s).await;
    mount(
        &h,
        ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "pong"}}]
        })),
    )
    .await;
    let (status, body) = json(
        h.admin(Method::POST, "/admin/api/test")
            .json(&json!({"model": "gemini-3.5-flash-lite"})),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["ok"], true);
    assert_eq!(body["reply"], "pong");
    let sent: Value = h.upstream.received_requests().await.unwrap()[0]
        .body_json()
        .unwrap();
    assert_eq!(sent["model"], "google/gemini-3.5-flash-lite");
    assert_eq!(sent["max_tokens"], 32);

    let (_, body) = json(
        h.admin(Method::POST, "/admin/api/test")
            .json(&json!({"model": "gpt-4o"})),
    )
    .await;
    assert_eq!(body["ok"], false);
    assert_eq!(body["status"], 404);
}
