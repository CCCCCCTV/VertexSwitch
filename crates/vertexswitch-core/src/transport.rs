//! 向上游发请求：超时、重试、配额耗尽识别（SPEC §4）。
//!
//! 只在**收到任何响应字节之前**连接出问题时重试。Vertex 配额耗尽时常见的表现是挂住 20 秒以上
//! 然后不发任何响应就断开（本应返回 429），这种情况不重试，直接返回 504。
//! 状态码约定：504 = 等上游超时，502 = 上游连接或响应坏了。

use std::time::{Duration, Instant};

use bytes::Bytes;
use futures_util::Stream;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::config::{RetrySettings, TimeoutSettings};
use crate::errors::{ProxyError, ProxyResult};

pub const VERTEX: &str = "Vertex AI";

/// 一次上游请求。
pub struct UpstreamRequest<'a> {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
    pub stream: bool,
    /// 上游可能合法地排队这么久（如 Flex 的 X-Server-Timeout），本地等待不得小于它
    pub min_read_timeout: Option<Duration>,
    /// 报错信息里的上游名称
    pub upstream: &'a str,
    /// 是否按 Vertex 的配额耗尽特征处理（本地 Ollama 等关掉）
    pub detect_quota: bool,
}

impl<'a> UpstreamRequest<'a> {
    pub fn post_json(
        url: String,
        headers: Vec<(String, String)>,
        body: Bytes,
        stream: bool,
    ) -> Self {
        Self {
            url,
            headers,
            body,
            stream,
            min_read_timeout: None,
            upstream: VERTEX,
            detect_quota: true,
        }
    }
}

#[derive(Clone)]
pub struct Transport {
    pub client: reqwest::Client,
    timeouts: TimeoutSettings,
    retry: RetrySettings,
}

impl Transport {
    pub fn new(timeouts: TimeoutSettings, retry: RetrySettings) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(secs(timeouts.connect))
            .pool_idle_timeout(Duration::from_secs(60))
            .build()
            .expect("HTTP 客户端配置合法");
        Self {
            client,
            timeouts,
            retry,
        }
    }

    /// 等响应头的时长。
    fn header_timeout(&self, req: &UpstreamRequest<'_>) -> Duration {
        let base = secs(if req.stream {
            self.timeouts.stream_idle
        } else {
            self.timeouts.read
        });
        req.min_read_timeout.map_or(base, |min| base.max(min))
    }

    /// 发出请求并返回已收到响应头、尚未读取正文的响应。
    pub async fn open(&self, req: &UpstreamRequest<'_>) -> ProxyResult<reqwest::Response> {
        let headers = header_map(&req.headers)?;
        let wait = self.header_timeout(req);
        let attempts = self.retry.attempts + 1;
        let upstream = req.upstream;
        for attempt in 1..=attempts {
            let request = self
                .client
                .post(&req.url)
                .headers(headers.clone())
                .body(req.body.clone());
            let started = Instant::now();
            let error = match tokio::time::timeout(wait, request.send()).await {
                Ok(Ok(response)) => return Ok(response),
                Ok(Err(e)) => e,
                Err(_) => return Err(ProxyError::new(504, format!("等待 {upstream} 响应超时"))),
            };
            if error.is_builder() {
                return Err(ProxyError::new(
                    502,
                    format!("请求 {upstream} 失败：{error}"),
                ));
            }
            let elapsed = started.elapsed();
            // 连接都没建立起来（含连接超时）不是配额特征；对端在没有响应的情况下断开才是
            let disconnected = !error.is_connect();
            if req.detect_quota && disconnected && elapsed >= secs(self.retry.quota_hang_seconds) {
                tracing::warn!(
                    "上游 {:.1}s 无响应后断开，按配额耗尽处理：{error}",
                    elapsed.as_secs_f64()
                );
                return Err(ProxyError::new(504, quota_message(elapsed)));
            }
            if attempt >= attempts {
                return Err(ProxyError::new(
                    502,
                    format!(
                        "无法连接 {upstream}（已重试 {} 次）：{}",
                        attempt - 1,
                        describe(&error)
                    ),
                ));
            }
            tracing::info!("上游连接错误，第 {attempt} 次重试：{}", describe(&error));
            tokio::time::sleep(secs(self.retry.backoff_seconds) * attempt).await;
        }
        unreachable!("循环内必定返回")
    }

    /// 读取非流式响应正文。
    pub async fn read_body(&self, response: reqwest::Response) -> ProxyResult<Bytes> {
        match tokio::time::timeout(secs(self.timeouts.read), response.bytes()).await {
            Ok(Ok(body)) => Ok(body),
            Ok(Err(e)) => Err(ProxyError::new(
                502,
                format!("读取上游响应失败：{}", describe(&e)),
            )),
            Err(_) => Err(ProxyError::new(504, "读取上游响应超时")),
        }
    }

    /// 把上游响应变成带空闲超时的数据块流。
    pub fn chunks(
        &self,
        mut response: reqwest::Response,
        upstream: &'static str,
    ) -> impl Stream<Item = ProxyResult<Bytes>> + Send + 'static {
        let idle = secs(self.timeouts.stream_idle);
        async_stream::stream! {
            loop {
                match tokio::time::timeout(idle, response.chunk()).await {
                    Ok(Ok(Some(chunk))) => yield Ok(chunk),
                    Ok(Ok(None)) => break,
                    Ok(Err(e)) => {
                        tracing::warn!("上游流式响应中断：{}", describe(&e));
                        yield Err(ProxyError::new(502, format!("{upstream} 流式响应中断")));
                        break;
                    }
                    Err(_) => {
                        tracing::warn!("上游流式响应空闲超时");
                        yield Err(ProxyError::new(504, format!("{upstream} 流式响应空闲超时")));
                        break;
                    }
                }
            }
        }
    }
}

fn header_map(headers: &[(String, String)]) -> ProxyResult<HeaderMap> {
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| ProxyError::new(500, format!("非法请求头名 {name}")))?;
        let value = HeaderValue::from_str(value)
            .map_err(|_| ProxyError::new(400, format!("请求头 {name} 的值非法")))?;
        map.insert(name, value);
    }
    Ok(map)
}

fn secs(value: f64) -> Duration {
    Duration::from_secs_f64(value.max(0.0))
}

fn quota_message(elapsed: Duration) -> String {
    format!(
        "Vertex AI 在 {:.0} 秒内没有返回任何响应就断开了连接，通常表示配额耗尽（RESOURCE_EXHAUSTED）。\
         请稍后重试或在 Google Cloud 控制台检查配额。",
        elapsed.as_secs_f64()
    )
}

/// reqwest 的错误信息加上最底层原因。
fn describe(error: &reqwest::Error) -> String {
    let mut text = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(inner) = source {
        text = format!("{text}: {inner}");
        source = inner.source();
    }
    text
}
