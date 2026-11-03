//! 上游请求的超时、重试与配额识别（SPEC §4）。用本地原始 TCP 服务模拟"无响应断开"等坏上游。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use vertexswitch_core::config::{RetrySettings, TimeoutSettings};
use vertexswitch_core::transport::{Transport, UpstreamRequest};

/// 起一个原始 TCP 服务：每个连接读完请求头后按 `behave` 行事。返回地址与连接计数。
async fn raw_server<F, Fut>(behave: F) -> (String, Arc<AtomicUsize>)
where
    F: Fn(tokio::net::TcpStream) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let behave = Arc::new(behave);
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            counter.fetch_add(1, Ordering::SeqCst);
            let behave = behave.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let _ = socket.read(&mut buf).await;
                behave(socket).await;
            });
        }
    });
    (format!("http://{addr}"), count)
}

fn transport(quota_hang: f64, read: f64, idle: f64) -> Transport {
    Transport::new(
        TimeoutSettings {
            connect: 2.0,
            read,
            stream_idle: idle,
            ..TimeoutSettings::default()
        },
        RetrySettings {
            attempts: 2,
            backoff_seconds: 0.0,
            quota_hang_seconds: quota_hang,
        },
    )
}

fn req(url: &str, stream: bool) -> UpstreamRequest<'static> {
    UpstreamRequest::post_json(
        format!("{url}/x"),
        vec![],
        Bytes::from_static(b"{}"),
        stream,
    )
}

#[tokio::test]
async fn immediate_disconnect_is_retried_then_502() {
    let (url, count) = raw_server(|socket| async move { drop(socket) }).await;
    let err = transport(20.0, 5.0, 5.0)
        .open(&req(&url, false))
        .await
        .unwrap_err();
    assert_eq!(err.status, 502);
    assert!(err.message.contains("已重试 2 次"), "{}", err.message);
    assert_eq!(count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn long_hang_then_disconnect_is_quota_504_without_retry() {
    let (url, count) = raw_server(|socket| async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        drop(socket);
    })
    .await;
    let err = transport(0.2, 5.0, 5.0)
        .open(&req(&url, false))
        .await
        .unwrap_err();
    assert_eq!(err.status, 504);
    assert!(err.message.contains("配额"), "{}", err.message);
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn quota_detection_can_be_disabled() {
    let (url, count) = raw_server(|socket| async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        drop(socket);
    })
    .await;
    let mut r = req(&url, false);
    r.detect_quota = false;
    r.upstream = "Ollama";
    let err = transport(0.1, 5.0, 5.0).open(&r).await.unwrap_err();
    assert_eq!(err.status, 502);
    assert!(err.message.contains("Ollama"));
    assert_eq!(count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn connection_refused_is_retried_not_quota() {
    // 绑定后立即释放端口，连接会被拒绝
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let err = transport(0.0, 5.0, 5.0)
        .open(&req(&url, false))
        .await
        .unwrap_err();
    assert_eq!(err.status, 502);
    assert!(err.message.contains("无法连接 Vertex AI"));
}

#[tokio::test]
async fn header_wait_timeout_is_504_without_retry() {
    let (url, count) = raw_server(|socket| async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(socket);
    })
    .await;
    let err = transport(20.0, 0.3, 5.0)
        .open(&req(&url, false))
        .await
        .unwrap_err();
    assert_eq!(err.status, 504);
    assert!(err.message.contains("超时"));
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn min_read_timeout_extends_header_wait() {
    let (url, _) = raw_server(|mut socket| async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = socket
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok")
            .await;
    })
    .await;
    let mut r = req(&url, false);
    r.min_read_timeout = Some(Duration::from_secs(3));
    let t = transport(20.0, 0.2, 0.2);
    let response = t.open(&r).await.unwrap();
    assert_eq!(
        t.read_body(response).await.unwrap(),
        Bytes::from_static(b"ok")
    );
}

#[tokio::test]
async fn stream_idle_timeout_yields_504_error() {
    let (url, _) = raw_server(|mut socket| async move {
        let _ = socket
            .write_all(b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n5\r\nhello\r\n")
            .await;
        tokio::time::sleep(Duration::from_secs(5)).await;
    })
    .await;
    let t = transport(20.0, 5.0, 0.3);
    let response = t.open(&req(&url, true)).await.unwrap();
    let items: Vec<_> = t.chunks(response, "Vertex AI").collect().await;
    assert_eq!(items[0].as_ref().unwrap(), &Bytes::from_static(b"hello"));
    let err = items[1].as_ref().unwrap_err();
    assert_eq!(err.status, 504);
    assert_eq!(items.len(), 2);
}

#[tokio::test]
async fn stream_broken_mid_way_yields_502_error() {
    let (url, _) = raw_server(|mut socket| async move {
        let _ = socket
            .write_all(b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n5\r\nhello\r\n")
            .await;
        drop(socket);
    })
    .await;
    let t = transport(20.0, 5.0, 5.0);
    let response = t.open(&req(&url, true)).await.unwrap();
    let items: Vec<_> = t.chunks(response, "Vertex AI").collect().await;
    assert!(items[0].is_ok());
    assert_eq!(items.last().unwrap().as_ref().unwrap_err().status, 502);
}
