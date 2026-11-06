//! 请求记录、用量统计与 Prometheus 指标。
//!
//! 记录在**响应体真正结束时**才落定：流式请求按最终结果（正常结束 / 流中出错 / 客户端断开）
//! 和流里出现的用量记账，而不是在上游返回响应头时就记为成功。
//!
//! 处理函数通过 [`note`] 把模型、后端、档位等信息写进当前请求的记录（task-local）。

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::task::{Context, Poll};
use std::time::Instant;

use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use bytes::Bytes;
use futures_util::Stream;
use regex::bytes::Regex;
use serde::Serialize;

use crate::app::SharedState;
use crate::errors::{Protocol, ProxyError, protocol_for_path};
use crate::logs::{Ring, now_ms};
use crate::tr;

/// 处理函数填写的请求信息。
#[derive(Debug, Clone, Default)]
pub struct RequestInfo {
    pub model: Option<String>,
    pub backend: Option<&'static str>,
    pub tier: Option<&'static str>,
    pub stream: bool,
}

tokio::task_local! {
    static CURRENT: Arc<Mutex<RequestInfo>>;
}

/// 在处理函数里补充当前请求的信息（不在记录范围内调用时什么也不做）。
pub fn note(update: impl FnOnce(&mut RequestInfo)) {
    let _ = CURRENT.try_with(|info| update(&mut info.lock().expect("锁未中毒")));
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestRecord {
    pub seq: u64,
    pub ts_ms: u64,
    pub method: String,
    pub path: String,
    pub protocol: &'static str,
    pub model: Option<String>,
    pub backend: Option<&'static str>,
    pub tier: Option<&'static str>,
    pub stream: bool,
    pub status: u16,
    pub duration_ms: u64,
    /// 首字耗时：从收到请求到发出第一段响应体。流式请求里包括排队、重试和模型思考；
    /// 非流式约等于总耗时；没发出任何内容（出错、客户端提前断开）时为空
    pub first_byte_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// ok / error / stream_error / cancelled
    pub outcome: &'static str,
    pub error: Option<String>,
}

#[derive(Default, Clone, Copy)]
struct Totals {
    count: u64,
    duration_ms: u64,
    input_tokens: u64,
    output_tokens: u64,
}

pub struct Metrics {
    pub requests: Ring<RequestRecord>,
    in_flight: AtomicI64,
    /// (protocol, backend, status) → 累计
    totals: Mutex<BTreeMap<(&'static str, &'static str, u16), Totals>>,
    started: Instant,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            requests: Ring::new(500),
            in_flight: AtomicI64::new(0),
            totals: Mutex::new(BTreeMap::new()),
            started: Instant::now(),
        }
    }
}

impl Metrics {
    pub fn in_flight(&self) -> i64 {
        self.in_flight.load(Ordering::Relaxed)
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    fn finish(&self, record: RequestRecord) {
        self.in_flight.fetch_sub(1, Ordering::Relaxed);
        {
            let mut totals = self.totals.lock().expect("锁未中毒");
            let t = totals
                .entry((
                    record.protocol,
                    record.backend.unwrap_or("none"),
                    record.status,
                ))
                .or_default();
            t.count += 1;
            t.duration_ms += record.duration_ms;
            t.input_tokens += record.input_tokens.unwrap_or(0);
            t.output_tokens += record.output_tokens.unwrap_or(0);
        }
        if record.outcome != "ok" {
            tracing::info!(
                "{} {} → {} {}{}",
                record.method,
                record.path,
                record.status,
                record.outcome,
                record
                    .error
                    .as_deref()
                    .map(|e| format!("：{e}"))
                    .unwrap_or_default()
            );
        }
        self.requests.push(record);
    }

    /// 汇总（JSON 给管理接口）。
    pub fn summary(&self) -> serde_json::Value {
        let totals = self.totals.lock().expect("锁未中毒");
        let rows: Vec<serde_json::Value> = totals
            .iter()
            .map(|((protocol, backend, status), t)| {
                serde_json::json!({
                    "protocol": protocol, "backend": backend, "status": status,
                    "requests": t.count, "duration_ms": t.duration_ms,
                    "input_tokens": t.input_tokens, "output_tokens": t.output_tokens,
                })
            })
            .collect();
        serde_json::json!({
            "uptime_secs": self.uptime_secs(),
            "in_flight": self.in_flight(),
            "totals": rows,
        })
    }

    /// Prometheus 文本格式。
    pub fn prometheus(&self) -> String {
        let totals = self.totals.lock().expect("锁未中毒");
        let mut out = String::new();
        out.push_str(&tr!(
            "# HELP vertexswitch_requests_total 已完成的请求数\n",
            "# HELP vertexswitch_requests_total Completed requests\n"
        ));
        out.push_str("# TYPE vertexswitch_requests_total counter\n");
        for ((p, b, s), t) in totals.iter() {
            out.push_str(&format!(
                "vertexswitch_requests_total{{protocol=\"{p}\",backend=\"{b}\",status=\"{s}\"}} {}\n",
                t.count
            ));
        }
        // 按 (protocol, backend) 汇总耗时与用量
        let mut by_backend: BTreeMap<(&str, &str), Totals> = BTreeMap::new();
        for ((p, b, _), t) in totals.iter() {
            let e = by_backend.entry((p, b)).or_default();
            e.count += t.count;
            e.duration_ms += t.duration_ms;
            e.input_tokens += t.input_tokens;
            e.output_tokens += t.output_tokens;
        }
        out.push_str(&tr!("# HELP vertexswitch_request_duration_seconds 请求耗时（到响应体结束）\n", "# HELP vertexswitch_request_duration_seconds Request duration (until the response body ends)\n"));
        out.push_str("# TYPE vertexswitch_request_duration_seconds summary\n");
        for ((p, b), t) in &by_backend {
            let labels = format!("protocol=\"{p}\",backend=\"{b}\"");
            out.push_str(&format!(
                "vertexswitch_request_duration_seconds_sum{{{labels}}} {:.3}\n",
                t.duration_ms as f64 / 1000.0
            ));
            out.push_str(&format!(
                "vertexswitch_request_duration_seconds_count{{{labels}}} {}\n",
                t.count
            ));
        }
        out.push_str(&tr!(
            "# HELP vertexswitch_tokens_total 上游报告的 token 用量\n",
            "# HELP vertexswitch_tokens_total Token usage reported by upstream\n"
        ));
        out.push_str("# TYPE vertexswitch_tokens_total counter\n");
        for ((p, b), t) in &by_backend {
            for (kind, n) in [("input", t.input_tokens), ("output", t.output_tokens)] {
                out.push_str(&format!(
                    "vertexswitch_tokens_total{{protocol=\"{p}\",backend=\"{b}\",kind=\"{kind}\"}} {n}\n"
                ));
            }
        }
        out.push_str(&tr!(
            "# HELP vertexswitch_requests_in_flight 进行中的请求\n",
            "# HELP vertexswitch_requests_in_flight Requests in progress\n"
        ));
        out.push_str("# TYPE vertexswitch_requests_in_flight gauge\n");
        out.push_str(&format!(
            "vertexswitch_requests_in_flight {}\n",
            self.in_flight()
        ));
        out.push_str("# TYPE vertexswitch_uptime_seconds gauge\n");
        out.push_str(&format!(
            "vertexswitch_uptime_seconds {}\n",
            self.uptime_secs()
        ));
        out
    }
}

// ---------------------------------------------------------------- 用量扫描

static USAGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#""(input_tokens|output_tokens|cache_read_input_tokens|cache_creation_input_tokens|promptTokenCount|candidatesTokenCount|thoughtsTokenCount|prompt_tokens|completion_tokens)"\s*:\s*(\d+)"#,
    )
    .expect("合法正则")
});

/// 从响应字节流里取各协议的用量字段（每个字段取最后一次出现的值，流式用量是累计值）。
#[derive(Default)]
pub struct UsageScanner {
    last: BTreeMap<String, u64>,
    tail: Vec<u8>,
}

impl UsageScanner {
    const KEEP: usize = 64;

    pub fn feed(&mut self, chunk: &[u8]) {
        self.tail.extend_from_slice(chunk);
        for caps in USAGE_RE.captures_iter(&self.tail) {
            if let Ok(n) = std::str::from_utf8(&caps[2]).unwrap_or("").parse() {
                self.last
                    .insert(String::from_utf8_lossy(&caps[1]).into_owned(), n);
            }
        }
        let keep_from = self.tail.len().saturating_sub(Self::KEEP);
        self.tail.drain(..keep_from);
    }

    fn get(&self, key: &str) -> Option<u64> {
        self.last.get(key).copied()
    }

    /// (输入, 输出)。Anthropic 的输入含缓存读写；Gemini 的输出含思考 token。
    pub fn usage(&self) -> (Option<u64>, Option<u64>) {
        let input = if let Some(n) = self.get("input_tokens") {
            Some(
                n + self.get("cache_read_input_tokens").unwrap_or(0)
                    + self.get("cache_creation_input_tokens").unwrap_or(0),
            )
        } else {
            self.get("promptTokenCount")
                .or_else(|| self.get("prompt_tokens"))
        };
        let output = self
            .get("output_tokens")
            .or_else(|| {
                self.get("candidatesTokenCount")
                    .map(|n| n + self.get("thoughtsTokenCount").unwrap_or(0))
            })
            .or_else(|| self.get("completion_tokens"));
        (input, output)
    }
}

// ---------------------------------------------------------------- 中间件

fn protocol_name(p: Protocol) -> &'static str {
    match p {
        Protocol::Anthropic => "anthropic",
        Protocol::Gemini => "gemini",
        Protocol::OpenAi => "openai",
    }
}

/// 是否记录这个路径（管理接口、健康检查、指标本身不记录）。
fn is_recorded(path: &str) -> bool {
    !(path.starts_with("/admin") || path == "/health" || path == "/metrics")
}

struct Pending {
    metrics: Arc<Metrics>,
    started: Instant,
    method: String,
    path: String,
    protocol: &'static str,
    info: Arc<Mutex<RequestInfo>>,
    status: u16,
    error: Option<String>,
    scanner: UsageScanner,
    stream_error: bool,
    first_byte_ms: Option<u64>,
    done: bool,
}

impl Pending {
    fn finish(&mut self, outcome: &'static str) {
        if self.done {
            return;
        }
        self.done = true;
        let info = self.info.lock().expect("锁未中毒").clone();
        let (input_tokens, output_tokens) = self.scanner.usage();
        let outcome = if self.status >= 400 {
            "error"
        } else if self.stream_error {
            "stream_error"
        } else {
            outcome
        };
        self.metrics.finish(RequestRecord {
            seq: self.metrics.requests.next_seq(),
            ts_ms: now_ms(),
            method: std::mem::take(&mut self.method),
            path: std::mem::take(&mut self.path),
            protocol: self.protocol,
            model: info.model,
            backend: info.backend,
            tier: info.tier,
            stream: info.stream,
            status: self.status,
            duration_ms: self.started.elapsed().as_millis() as u64,
            first_byte_ms: self.first_byte_ms,
            input_tokens,
            output_tokens,
            outcome,
            error: self.error.take(),
        });
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        // 响应体没读完就被丢弃：客户端断开，或错误响应被 render_errors 换掉
        self.finish("cancelled");
    }
}

/// 包住响应体：转发数据、扫描用量，结束或被丢弃时落定记录。
struct ObservedBody {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, axum::Error>> + Send>>,
    pending: Pending,
}

impl Stream for ObservedBody {
    type Item = Result<Bytes, axum::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                if this.pending.first_byte_ms.is_none() && !chunk.is_empty() {
                    this.pending.first_byte_ms =
                        Some(this.pending.started.elapsed().as_millis() as u64);
                }
                // 代理在流中出错时补发的错误事件以空行开头（见 errors::stream_event）
                if chunk.starts_with(b"\n\nevent: error")
                    || chunk.starts_with(b"\n\ndata: {\"error\"")
                {
                    this.pending.stream_error = true;
                    this.pending.error = Some(
                        String::from_utf8_lossy(&chunk)
                            .trim()
                            .chars()
                            .take(300)
                            .collect(),
                    );
                }
                this.pending.scanner.feed(&chunk);
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(None) => {
                this.pending.finish("ok");
                Poll::Ready(None)
            }
            other => other,
        }
    }
}

/// 记录中间件：放在错误渲染层之内，能直接拿到 [`ProxyError`] 的信息。
pub async fn record_requests(
    State(state): State<SharedState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_owned();
    if !is_recorded(&path) {
        return next.run(request).await;
    }
    let metrics = state.metrics.clone();
    metrics.in_flight.fetch_add(1, Ordering::Relaxed);
    let info = Arc::new(Mutex::new(RequestInfo::default()));
    let method = request.method().to_string();
    let started = Instant::now();
    let response = CURRENT.scope(info.clone(), next.run(request)).await;

    let error = response
        .extensions()
        .get::<ProxyError>()
        .map(|e| e.message.clone());
    let pending = Pending {
        metrics,
        started,
        method,
        protocol: protocol_name(protocol_for_path(&path)),
        path,
        info,
        status: response.status().as_u16(),
        error,
        scanner: UsageScanner::default(),
        stream_error: false,
        first_byte_ms: None,
        done: false,
    };
    let (parts, body) = response.into_parts();
    let observed = ObservedBody {
        inner: Box::pin(body.into_data_stream()),
        pending,
    };
    Response::from_parts(parts, Body::from_stream(observed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_anthropic_stream_takes_last_values() {
        let mut s = UsageScanner::default();
        s.feed(br#"data: {"type":"message_start","message":{"usage":{"input_tokens":12,"cache_read_input_tokens":3,"output_tokens":1}}}"#);
        s.feed(br#"data: {"type":"message_delta","usage":{"output_tokens":30}}"#);
        assert_eq!(s.usage(), (Some(15), Some(30)));
    }

    #[test]
    fn usage_gemini_includes_thoughts_and_handles_splits() {
        let mut s = UsageScanner::default();
        let raw = br#"{"usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":7,"thoughtsTokenCount":2}}"#;
        for chunk in raw.chunks(3) {
            s.feed(chunk);
        }
        assert_eq!(s.usage(), (Some(5), Some(9)));
    }

    #[test]
    fn usage_openai_and_none() {
        let mut s = UsageScanner::default();
        s.feed(br#"{"usage":{"prompt_tokens":4,"completion_tokens":6}}"#);
        assert_eq!(s.usage(), (Some(4), Some(6)));
        assert_eq!(UsageScanner::default().usage(), (None, None));
    }

    #[test]
    fn prometheus_renders_counters() {
        let m = Metrics::default();
        m.in_flight.fetch_add(1, Ordering::Relaxed);
        m.finish(RequestRecord {
            seq: 1,
            ts_ms: 0,
            method: "POST".into(),
            path: "/v1/messages".into(),
            protocol: "anthropic",
            model: Some("claude-x".into()),
            backend: Some("claude"),
            tier: None,
            stream: false,
            status: 200,
            duration_ms: 1500,
            first_byte_ms: Some(1500),
            input_tokens: Some(10),
            output_tokens: Some(5),
            outcome: "ok",
            error: None,
        });
        let text = m.prometheus();
        assert!(text.contains(
            "vertexswitch_requests_total{protocol=\"anthropic\",backend=\"claude\",status=\"200\"} 1"
        ));
        assert!(text.contains("vertexswitch_request_duration_seconds_sum{protocol=\"anthropic\",backend=\"claude\"} 1.500"));
        assert!(text.contains(
            "vertexswitch_tokens_total{protocol=\"anthropic\",backend=\"claude\",kind=\"output\"} 5"
        ));
        assert!(text.contains("vertexswitch_requests_in_flight 0"));
    }
}
