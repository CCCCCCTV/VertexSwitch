//! 运行日志：内存环形缓冲 + 实时广播，供管理接口查询与 SSE 推送。
//!
//! [`LogLayer`] 接到 tracing 上，把 INFO 及以上的事件写进 [`LogHub`]。只记录日志消息本身
//! 和附带的字段，不记录请求 / 响应正文。

use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    /// 单调递增的序号，客户端用它做增量拉取（`?since=`）
    pub seq: u64,
    pub ts_ms: u64,
    pub level: String,
    pub target: String,
    pub message: String,
    /// 事件附带的字段（如 `model`、`kind`）。界面按 `kind` 识别特定事件，不去解析消息文字
    pub fields: BTreeMap<String, String>,
}

/// 有界的记录缓冲：最近 N 条 + 新记录广播。日志与请求记录共用。
pub struct Ring<T: Clone> {
    items: Mutex<VecDeque<T>>,
    capacity: usize,
    next_seq: AtomicU64,
    tx: broadcast::Sender<T>,
}

impl<T: Clone> Ring<T> {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            items: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
            next_seq: AtomicU64::new(1),
            tx,
        }
    }

    pub fn next_seq(&self) -> u64 {
        self.next_seq.fetch_add(1, Ordering::Relaxed)
    }

    pub fn push(&self, item: T) {
        {
            let mut items = self.items.lock().expect("锁未中毒");
            if items.len() == self.capacity {
                items.pop_front();
            }
            items.push_back(item.clone());
        }
        // 没有订阅者时发送失败，忽略
        let _ = self.tx.send(item);
    }

    /// 满足条件的最近 `limit` 条（按时间顺序）。
    pub fn recent(&self, limit: usize, keep: impl Fn(&T) -> bool) -> Vec<T> {
        let items = self.items.lock().expect("锁未中毒");
        let matched: Vec<&T> = items.iter().filter(|i| keep(i)).collect();
        let start = matched.len().saturating_sub(limit);
        matched[start..].iter().map(|i| (*i).clone()).collect()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<T> {
        self.tx.subscribe()
    }
}

pub type LogHub = Ring<LogEntry>;

impl LogHub {
    pub fn record(&self, level: &str, target: &str, message: String) {
        self.record_fields(level, target, message, BTreeMap::new());
    }

    pub fn record_fields(
        &self,
        level: &str,
        target: &str,
        message: String,
        fields: BTreeMap<String, String>,
    ) {
        let entry = LogEntry {
            seq: self.next_seq(),
            ts_ms: now_ms(),
            level: level.to_owned(),
            target: target.to_owned(),
            message,
            fields,
        };
        self.push(entry);
    }
}

/// 把 tracing 事件写进 [`LogHub`] 的层。
pub struct LogLayer {
    hub: std::sync::Arc<LogHub>,
}

impl LogLayer {
    pub fn new(hub: std::sync::Arc<LogHub>) -> Self {
        Self { hub }
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: BTreeMap<String, String>,
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            self.fields
                .insert(field.name().to_owned(), value.to_owned());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            self.fields
                .insert(field.name().to_owned(), format!("{value:?}"));
        }
    }
}

impl<S: Subscriber> Layer<S> for LogLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        if *meta.level() > Level::INFO {
            return;
        }
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        self.hub.record_fields(
            meta.level().as_str(),
            meta.target(),
            visitor.message,
            visitor.fields,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_is_bounded_and_filters() {
        let hub = LogHub::new(3);
        for i in 0..5 {
            hub.record("INFO", "t", format!("m{i}"));
        }
        let all = hub.recent(10, |_| true);
        assert_eq!(
            all.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(),
            ["m2", "m3", "m4"]
        );
        let since = all[0].seq;
        assert_eq!(hub.recent(10, |e| e.seq > since).len(), 2);
        assert_eq!(hub.recent(1, |_| true)[0].message, "m4");
    }

    #[tokio::test]
    async fn broadcast_to_subscribers() {
        let hub = LogHub::new(10);
        let mut rx = hub.subscribe();
        hub.record("WARN", "t", "hello".into());
        assert_eq!(rx.recv().await.unwrap().message, "hello");
    }

    #[test]
    fn layer_captures_info_and_above() {
        use tracing_subscriber::layer::SubscriberExt;
        let hub = std::sync::Arc::new(LogHub::new(10));
        let subscriber = tracing_subscriber::registry().with(LogLayer::new(hub.clone()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!("hidden");
            tracing::info!(model = "m", tier = %"flex", "可见 {}", 1);
            tracing::warn!("告警");
        });
        let entries = hub.recent(10, |_| true);
        let msgs: Vec<&str> = entries.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(msgs, ["可见 1", "告警"]);
        let fields = &entries[0].fields;
        assert_eq!(fields["model"], "m");
        assert_eq!(fields["tier"], "flex");
        assert!(entries[1].fields.is_empty());
    }
}
