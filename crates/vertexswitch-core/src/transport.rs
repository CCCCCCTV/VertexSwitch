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
use crate::tr;
use crate::wait_formula::{MAX_WAIT_SECONDS, WaitFormula};

pub const VERTEX: &str = "Vertex AI";

/// 繁忙重试次数的上限（配置写得再大也按这个算）
pub const MAX_BUSY_RETRIES: u32 = 10;
/// 日志字段 kind：上游繁忙，即将重试 / 重试后成功（界面“运行状况”按它识别）
pub const LOG_KIND_RETRY: &str = "upstream_retry";
pub const LOG_KIND_RETRY_OK: &str = "upstream_retry_ok";
/// 上游 `Retry-After` 要求等的秒数超过这个值、也超过自己算出的等待时，不重试，直接交给客户端
const RETRY_AFTER_CAP: f64 = 30.0;

/// 值得等一会儿再试的状态码：429 限流 / 资源耗尽、503 暂不可用、529 过载；500 只重试一次。
fn is_busy(status: u16, retried_500: bool) -> bool {
    matches!(status, 429 | 503 | 529) || (status == 500 && !retried_500)
}

/// 第 `retried + 1` 次重试前等多久：按等待算式算出秒数，再乘 0.75 ~ 1.25 的随机系数；
/// `Retry-After`（秒）更长时按它，但它超过 30 秒且超过算出的秒数时返回 None（不重试）。
/// 结果不超过 [`MAX_WAIT_SECONDS`]。
fn busy_wait(formula: &WaitFormula, retried: u32, headers: &HeaderMap) -> Option<Duration> {
    let wanted = formula.seconds(retried + 1);
    let mut wait = wanted * jitter();
    if let Some(after) = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
    {
        if after > RETRY_AFTER_CAP.max(wanted) {
            return None;
        }
        wait = wait.max(after);
    }
    Some(Duration::from_secs_f64(wait.min(MAX_WAIT_SECONDS)))
}

/// 0.75 ~ 1.25 之间的随机系数，让同时被限流的请求错开重试。
fn jitter() -> f64 {
    use std::hash::{BuildHasher, Hasher};
    let x = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    0.75 + (x % 10_000) as f64 / 20_000.0
}

/// 日志里的模型名：先看上游地址（`.../models/{model}:{action}`）；OpenAI 兼容接口等地址里没有时
/// 读请求体的 `model`，去掉 `google/` 这类发布方前缀；都没有时为空。
fn upstream_model(url: &str, body: &[u8]) -> String {
    let from_url = model_in_url(url);
    if !from_url.is_empty() {
        return from_url;
    }
    #[derive(serde::Deserialize)]
    struct Body {
        model: Option<String>,
    }
    serde_json::from_slice::<Body>(body)
        .ok()
        .and_then(|b| b.model)
        .map(|m| m.rsplit('/').next().unwrap_or_default().to_string())
        .unwrap_or_default()
}

/// 上游地址里的模型名（`.../models/{model}:{action}`），没有时为空。
fn model_in_url(url: &str) -> String {
    url.split_once("/models/")
        .map(|(_, rest)| {
            rest.split([':', '/', '?'])
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .unwrap_or_default()
}

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
    busy_wait: WaitFormula,
}

impl Transport {
    pub fn new(timeouts: TimeoutSettings, retry: RetrySettings) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(secs(timeouts.connect))
            .pool_idle_timeout(Duration::from_secs(60))
            .build()
            .expect("HTTP 客户端配置合法");
        // 配置在载入 / 修改时已校验过；万一不合法就用默认算式
        let busy_wait = WaitFormula::parse(&retry.busy_wait).unwrap_or_default();
        Self {
            client,
            timeouts,
            retry,
            busy_wait,
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
    ///
    /// 上游繁忙（429 / 503 / 529，500 只算一次）时按配置等一会儿再发：等待按 `busy_wait` 算式算出
    /// 并带 ±25% 抖动，上游给了 `Retry-After` 时至少等这么久。还没给客户端发出任何字节，
    /// 所以对流式请求同样适用。不管上游过了多久才返回繁忙都会重试（Standard 也可能先挂一分多钟再回 429）；
    /// 最多等多久由客户端决定：客户端断开时整个请求被丢弃，重试随之停止。
    pub async fn open(&self, req: &UpstreamRequest<'_>) -> ProxyResult<reqwest::Response> {
        let headers = header_map(&req.headers)?;
        let limit = if self.retry.busy {
            self.retry.busy_attempts.min(MAX_BUSY_RETRIES)
        } else {
            0
        };
        let upstream = req.upstream;
        // 只在要写日志时才解析请求体取模型名
        let mut model_cache: Option<String> = None;
        let mut model = || {
            model_cache
                .get_or_insert_with(|| upstream_model(&req.url, &req.body))
                .clone()
        };
        let mut retried = 0;
        let mut retried_500 = false;
        loop {
            let response = self.send(req, &headers).await?;
            let status = response.status().as_u16();
            let busy = is_busy(status, retried_500);
            let wait = if busy && retried < limit {
                busy_wait(&self.busy_wait, retried, response.headers())
            } else {
                None
            };
            let Some(wait) = wait else {
                if retried > 0 && response.status().is_success() {
                    tracing::info!(
                        kind = LOG_KIND_RETRY_OK,
                        model = %model(),
                        retries = retried,
                        "{}",
                        tr!(
                            "{upstream} 在第 {retried} 次重试后成功",
                            "{upstream} succeeded on retry {retried}"
                        )
                    );
                } else if retried > 0 {
                    tracing::warn!(
                        model = %model(),
                        "{}",
                        tr!(
                            "{upstream} 重试 {retried} 次后仍返回 {status}，交给客户端处理",
                            "{upstream} still returned {status} after retry {retried}; passing it to the client"
                        )
                    );
                }
                return Ok(response);
            };
            retried += 1;
            retried_500 |= status == 500;
            tracing::warn!(
                kind = LOG_KIND_RETRY,
                model = %model(),
                status,
                "{}",
                tr!(
                    "{upstream} 返回 {status}（繁忙），{:.1} 秒后第 {retried} 次重试（最多 {limit} 次）",
                    "{upstream} returned {status} (busy); retry {retried} of {limit} in {:.1}s",
                    wait.as_secs_f64()
                )
            );
            // 读掉错误正文好让连接复用；读不完也无所谓
            let _ = tokio::time::timeout(Duration::from_secs(5), response.bytes()).await;
            tokio::time::sleep(wait).await;
        }
    }

    /// 发一次请求拿到响应头；连接层出错且还没收到任何响应时按 `attempts` 重试。
    async fn send(
        &self,
        req: &UpstreamRequest<'_>,
        headers: &HeaderMap,
    ) -> ProxyResult<reqwest::Response> {
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
                Err(_) => {
                    return Err(ProxyError::new(
                        504,
                        tr!(
                            "等待 {upstream} 响应超时",
                            "Timed out waiting for {upstream} to respond"
                        ),
                    ));
                }
            };
            if error.is_builder() {
                return Err(ProxyError::new(
                    502,
                    tr!(
                        "请求 {upstream} 失败：{error}",
                        "Request to {upstream} failed: {error}"
                    ),
                ));
            }
            let elapsed = started.elapsed();
            // 连接都没建立起来（含连接超时）不是配额特征；对端在没有响应的情况下断开才是
            let disconnected = !error.is_connect();
            if req.detect_quota && disconnected && elapsed >= secs(self.retry.quota_hang_seconds) {
                tracing::warn!(
                    "{}",
                    tr!(
                        "上游 {:.1}s 无响应后断开，按配额耗尽处理：{error}",
                        "Upstream disconnected after {:.1}s without responding; treating it as quota exhaustion: {error}",
                        elapsed.as_secs_f64()
                    )
                );
                return Err(ProxyError::new(504, quota_message(elapsed)));
            }
            if attempt >= attempts {
                return Err(ProxyError::new(
                    502,
                    tr!(
                        "无法连接 {upstream}（已重试 {} 次）：{}",
                        "Couldn't connect to {upstream} (retried {} times): {}",
                        attempt - 1,
                        describe(&error)
                    ),
                ));
            }
            tracing::info!(
                "{}",
                tr!(
                    "上游连接错误，第 {attempt} 次重试：{}",
                    "Upstream connection error, retry {attempt}: {}",
                    describe(&error)
                )
            );
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
                tr!(
                    "读取上游响应失败：{}",
                    "Couldn't read the upstream response: {}",
                    describe(&e)
                ),
            )),
            Err(_) => Err(ProxyError::new(
                504,
                tr!(
                    "读取上游响应超时",
                    "Timed out reading the upstream response"
                ),
            )),
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
                        tracing::warn!("{}", tr!("上游流式响应中断：{}", "Upstream stream interrupted: {}", describe(&e)));
                        yield Err(ProxyError::new(502, tr!("{upstream} 流式响应中断", "{upstream} stream interrupted")));
                        break;
                    }
                    Err(_) => {
                        tracing::warn!("{}", tr!("上游流式响应空闲超时", "Upstream stream went idle and timed out"));
                        yield Err(ProxyError::new(504, tr!("{upstream} 流式响应空闲超时", "{upstream} stream went idle and timed out")));
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
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            ProxyError::new(
                500,
                tr!("非法请求头名 {name}", "Invalid request header name {name}"),
            )
        })?;
        let value = HeaderValue::from_str(value).map_err(|_| {
            ProxyError::new(
                400,
                tr!(
                    "请求头 {name} 的值非法",
                    "Invalid value for request header {name}"
                ),
            )
        })?;
        map.insert(name, value);
    }
    Ok(map)
}

fn secs(value: f64) -> Duration {
    Duration::from_secs_f64(value.max(0.0))
}

fn quota_message(elapsed: Duration) -> String {
    tr!(
        "Vertex AI 在 {:.0} 秒内没有返回任何响应就断开了连接，通常表示配额耗尽（RESOURCE_EXHAUSTED）。\
         请稍后重试或在 Google Cloud 控制台检查配额。",
        "Vertex AI closed the connection without any response within {:.0} seconds, which usually \
         means the quota is exhausted (RESOURCE_EXHAUSTED). Try again later or check your quota in \
         the Google Cloud console.",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn retry_after(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            reqwest::header::RETRY_AFTER,
            HeaderValue::from_str(value).unwrap(),
        );
        h
    }

    #[test]
    fn busy_statuses() {
        for status in [429, 503, 529] {
            assert!(is_busy(status, false) && is_busy(status, true), "{status}");
        }
        assert!(is_busy(500, false));
        assert!(!is_busy(500, true));
        for status in [200, 400, 401, 403, 404, 413, 502, 504] {
            assert!(!is_busy(status, false), "{status}");
        }
    }

    fn formula(text: &str) -> WaitFormula {
        WaitFormula::parse(text).unwrap()
    }

    #[test]
    fn wait_follows_the_formula_with_jitter() {
        let none = HeaderMap::new();
        let default = WaitFormula::default();
        for _ in 0..50 {
            let first = busy_wait(&default, 0, &none).unwrap().as_secs_f64();
            assert!((0.75..=1.25).contains(&first), "{first}");
            let third = busy_wait(&default, 2, &none).unwrap().as_secs_f64();
            assert!((3.0..=5.0).contains(&third), "{third}");
            let fixed = busy_wait(&formula("5"), 3, &none).unwrap().as_secs_f64();
            assert!((3.75..=6.25).contains(&fixed), "{fixed}");
            let huge = busy_wait(&formula("1000"), 0, &none).unwrap().as_secs_f64();
            assert!(huge <= MAX_WAIT_SECONDS, "{huge}");
        }
        assert_eq!(busy_wait(&formula("0"), 3, &none), Some(Duration::ZERO));
    }

    #[test]
    fn retry_after_sets_a_floor_or_skips_the_retry() {
        let zero = formula("0");
        assert_eq!(
            busy_wait(&zero, 0, &retry_after("7")),
            Some(Duration::from_secs(7))
        );
        assert_eq!(busy_wait(&zero, 0, &retry_after("120")), None);
        // 自己本来就打算等很久时，上游要求的长等待也接受
        let wait = busy_wait(&formula("200"), 0, &retry_after("120")).unwrap();
        assert!(wait >= Duration::from_secs(150), "{wait:?}");
        // HTTP 日期格式不认，按自己的算式
        let date = retry_after("Wed, 21 Oct 2026 07:28:00 GMT");
        assert_eq!(busy_wait(&zero, 0, &date), Some(Duration::ZERO));
    }

    #[test]
    fn model_from_the_body_when_the_url_has_none() {
        let compat =
            "https://h/v1beta1/projects/p/locations/global/endpoints/openapi/chat/completions";
        assert_eq!(
            upstream_model(
                compat,
                br#"{"model":"google/gemini-4-argon","messages":[]}"#
            ),
            "gemini-4-argon"
        );
        assert_eq!(upstream_model(compat, br#"{"model":"qwen3"}"#), "qwen3");
        assert_eq!(upstream_model(compat, br#"{"messages":[]}"#), "");
        assert_eq!(upstream_model(compat, b"not json"), "");
        // 地址里有模型时以地址为准
        assert_eq!(
            upstream_model(
                "https://h/v1/projects/p/locations/global/publishers/google/models/gemini-3.5-flash:generateContent",
                br#"{"model":"other"}"#
            ),
            "gemini-3.5-flash"
        );
    }

    #[test]
    fn model_from_upstream_url() {
        assert_eq!(
            model_in_url(
                "https://h/v1/projects/p/locations/global/publishers/google/models/gemini-3.5-flash:streamGenerateContent?alt=sse"
            ),
            "gemini-3.5-flash"
        );
        assert_eq!(
            model_in_url(
                "https://h/v1/projects/p/locations/global/publishers/anthropic/models/claude-x:rawPredict"
            ),
            "claude-x"
        );
        assert_eq!(
            model_in_url(
                "https://h/v1beta1/projects/p/locations/global/endpoints/openapi/chat/completions"
            ),
            ""
        );
    }
}
