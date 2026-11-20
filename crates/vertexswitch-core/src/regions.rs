//! 区域切换（SPEC §9a）：Gemini 请求按模型记住当前用哪个区域，429 或太慢时自动换。
//!
//! - 当前区域返回 429 / 503（繁忙）：这个请求立刻改发下一个区域，之后的请求也用它；之前因为慢
//!   记下过一个区域时，改回那个区域。一圈都繁忙时交还给繁忙重试，按等待算式等一会儿再来一圈。
//! - 不是平时区域的区域返回 404（这个区域没有这个模型）：同样改发下一个区域，重置前跳过它。
//! - 流式请求成功后看首字（从发到这个区域算起）和出字速度：慢就记下这个区域和它的表现，之后的
//!   请求换下一个区域；新区域比记录的还慢，或者一圈都慢过，就回到记录里最快的区域并停在那里。
//! - 第一次离开平时区域满 `reset_minutes` 分钟后回到平时区域，记录清空。
//!
//! 共享的状态只在“出问题的就是当前区域”时才改，并发的旧请求晚到的结果不会把区域来回拨。
//! 平时区域就是 `gemini.region`（为空时 `google.region`）；改了相关配置时整体清空。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::config::RegionSwitchSettings;
use crate::tr;

/// 区域切换写日志时带的 `kind` 字段。日志另带 `model`、`from`、`to`。
pub const LOG_KIND_SWITCH: &str = "region_switch";

/// 出字速度至少要基于这么多 token 才算数
const MIN_SPEED_TOKENS: u64 = 50;
/// 出字时间至少这么久才算速度
const MIN_SPEED_SECONDS: f64 = 1.0;

/// 一次成功的流式请求在某个区域的表现。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// 从发到这个区域到出第一段内容（毫秒）
    pub first_byte_ms: u64,
    /// 出字速度（token/秒）；输出太少或太快结束时算不准，为空
    pub tokens_per_second: Option<f64>,
}

impl Sample {
    /// `generating` 是从第一段内容到结束的时长。
    pub fn new(first_byte_ms: u64, output_tokens: Option<u64>, generating: Duration) -> Self {
        let secs = generating.as_secs_f64();
        let tokens_per_second = output_tokens
            .filter(|t| *t >= MIN_SPEED_TOKENS && secs >= MIN_SPEED_SECONDS)
            .map(|t| t as f64 / secs);
        Self {
            first_byte_ms,
            tokens_per_second,
        }
    }
}

/// 请求在某个区域出了什么问题。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trouble {
    /// 429 / 503
    Busy(u16),
    /// 404：这个区域没有这个模型
    Missing,
}

#[derive(Debug, Clone)]
struct Recorded {
    region: String,
    sample: Sample,
}

#[derive(Debug, Clone)]
struct ModelState {
    current: String,
    /// 第一次离开平时区域的时刻，重置间隔从这里算；没离开过时为空
    since: Option<Instant>,
    /// 因为慢切走时记下的区域（慢过的区域里表现最好的那个）
    recorded: Option<Recorded>,
    /// 因为慢试过的区域
    slow_tried: Vec<String>,
    /// 这个模型没有的区域（404）
    missing: Vec<String>,
    /// 已经回到记录的区域，停在这里，直到重置或它也出问题
    settled: bool,
}

impl ModelState {
    fn at(home: &str) -> Self {
        Self {
            current: home.to_owned(),
            since: None,
            recorded: None,
            slow_tried: Vec::new(),
            missing: Vec::new(),
            settled: false,
        }
    }

    fn is_fresh(&self, home: &str) -> bool {
        self.current == home && self.since.is_none()
    }

    /// 之后的请求改用 `to`，写一条日志。
    fn switch(&mut self, model: &str, to: &str, why: &str, now: Instant) {
        if self.current == to {
            return;
        }
        let from = std::mem::replace(&mut self.current, to.to_owned());
        self.since.get_or_insert(now);
        tracing::warn!(
            kind = LOG_KIND_SWITCH,
            model = %model,
            from = %from,
            to = %to,
            "{}",
            tr!(
                "{model} 在 {from} {why}，之后的请求改用 {to}",
                "{model} in {from}: {why}; later requests use {to}"
            )
        );
    }
}

/// 某个模型此刻的区域切换状态（给界面看）。只列离开过平时区域的模型。
#[derive(Debug, Clone, Serialize)]
pub struct ModelRegion {
    pub model: String,
    pub region: String,
    /// 离开平时区域多久了（毫秒）
    pub away_ms: u64,
    /// 离重置回平时区域还有多久（毫秒）
    pub reset_in_ms: u64,
    pub recorded_region: Option<String>,
    pub recorded_first_byte_ms: Option<u64>,
    pub recorded_tokens_per_second: Option<f64>,
    pub settled: bool,
    pub missing: Vec<String>,
}

#[derive(Debug, Default)]
pub struct RegionSwitch {
    states: Mutex<HashMap<String, ModelState>>,
}

fn reset_after(cfg: &RegionSwitchSettings) -> Duration {
    Duration::from_secs_f64((cfg.reset_minutes * 60.0).max(0.0))
}

/// 轮换顺序：配置的区域，平时区域不在其中时排在最前。
fn order(cfg: &RegionSwitchSettings, home: &str) -> Vec<String> {
    let mut list: Vec<String> = Vec::new();
    for region in cfg.regions.iter().map(|r| r.trim()) {
        if !region.is_empty() && !list.iter().any(|r| r == region) {
            list.push(region.to_owned());
        }
    }
    if !list.iter().any(|r| r == home) {
        list.insert(0, home.to_owned());
    }
    list
}

/// `from` 之后（循环）第一个不跳过的区域；`from` 不在顺序里时从头找。
fn next_region(order: &[String], from: &str, skip: impl Fn(&str) -> bool) -> Option<String> {
    let start = order.iter().position(|r| r == from).map_or(0, |i| i + 1);
    (0..order.len())
        .map(|k| &order[(start + k) % order.len()])
        .find(|r| r.as_str() != from && !skip(r))
        .cloned()
}

fn seconds(ms: u64) -> f64 {
    ms as f64 / 1000.0
}

impl RegionSwitch {
    /// 配置变了：所有模型回到平时区域，记录清空。
    pub fn reset(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, ModelState>> {
        self.states.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 取出模型的状态；离开平时区域满重置间隔时先回到平时区域。
    fn with_state<T>(
        &self,
        cfg: &RegionSwitchSettings,
        home: &str,
        model: &str,
        now: Instant,
        f: impl FnOnce(&mut ModelState) -> T,
    ) -> T {
        let mut states = self.lock();
        let state = states
            .entry(model.to_owned())
            .or_insert_with(|| ModelState::at(home));
        if let Some(since) = state.since
            && now.saturating_duration_since(since) >= reset_after(cfg)
        {
            if state.current != home {
                tracing::info!(
                    kind = LOG_KIND_SWITCH,
                    model = %model,
                    from = %state.current,
                    to = %home,
                    "{}",
                    tr!(
                        "{model} 离开平时区域满 {} 分钟，回到 {home}",
                        "{model} has been away from its usual region for {} minutes; back to {home}",
                        cfg.reset_minutes
                    )
                );
            }
            *state = ModelState::at(home);
        }
        f(state)
    }

    /// 这个模型现在该发到哪个区域。
    pub fn region_for(&self, cfg: &RegionSwitchSettings, home: &str, model: &str) -> String {
        self.region_for_at(cfg, home, model, Instant::now())
    }

    fn region_for_at(
        &self,
        cfg: &RegionSwitchSettings,
        home: &str,
        model: &str,
        now: Instant,
    ) -> String {
        self.with_state(cfg, home, model, now, |s| s.current.clone())
    }

    /// 请求在 `region` 出了问题：返回这个请求接下来改发的区域（`tried` 是这个请求这一轮已经试过的），
    /// 没有可换的时返回 None。出问题的是当前区域时，之后的请求也跟着换。
    pub fn on_trouble(
        &self,
        cfg: &RegionSwitchSettings,
        home: &str,
        model: &str,
        region: &str,
        trouble: Trouble,
        tried: &[String],
    ) -> Option<String> {
        self.on_trouble_at(cfg, home, model, region, trouble, tried, Instant::now())
    }

    #[allow(clippy::too_many_arguments)]
    fn on_trouble_at(
        &self,
        cfg: &RegionSwitchSettings,
        home: &str,
        model: &str,
        region: &str,
        trouble: Trouble,
        tried: &[String],
        now: Instant,
    ) -> Option<String> {
        // 平时区域没有这个模型就是真的没有，交给客户端
        if trouble == Trouble::Missing && region == home {
            return None;
        }
        let order = order(cfg, home);
        self.with_state(cfg, home, model, now, |s| {
            if trouble == Trouble::Missing && !s.missing.iter().any(|r| r == region) {
                s.missing.push(region.to_owned());
            }
            let skip = |r: &str| tried.iter().any(|t| t == r) || s.missing.iter().any(|m| m == r);
            let back = s
                .recorded
                .as_ref()
                .map(|r| r.region.clone())
                .filter(|r| r != region && !skip(r));
            let target = back.clone().or_else(|| next_region(&order, region, skip))?;
            if s.current == region {
                if s.recorded.as_ref().is_some_and(|r| r.region == region) {
                    // 记录的区域自己也出问题了，记录作废
                    s.recorded = None;
                    s.slow_tried.clear();
                }
                s.settled = back.is_some();
                let why = match trouble {
                    Trouble::Busy(status) => {
                        tr!("返回 {status}（繁忙）", "returned {status} (busy)")
                    }
                    Trouble::Missing => tr!("没有这个模型（404）", "doesn't have this model (404)"),
                };
                s.switch(model, &target, &why, now);
            }
            Some(target)
        })
    }

    /// 一次成功的流式请求在 `region` 的表现：慢就换区域（见模块说明）。
    pub fn observe(
        &self,
        cfg: &RegionSwitchSettings,
        home: &str,
        model: &str,
        region: &str,
        sample: Sample,
    ) {
        self.observe_at(cfg, home, model, region, sample, Instant::now());
    }

    fn observe_at(
        &self,
        cfg: &RegionSwitchSettings,
        home: &str,
        model: &str,
        region: &str,
        sample: Sample,
        now: Instant,
    ) {
        let slow_first_byte = cfg.slow_first_byte_seconds > 0.0
            && seconds(sample.first_byte_ms) > cfg.slow_first_byte_seconds;
        let slow_speed = cfg.slow_tokens_per_second > 0.0
            && sample
                .tokens_per_second
                .is_some_and(|t| t < cfg.slow_tokens_per_second);
        if !slow_first_byte && !slow_speed {
            return;
        }
        let order = order(cfg, home);
        self.with_state(cfg, home, model, now, |s| {
            if s.current != region || s.settled {
                return;
            }
            let how = match (slow_first_byte, sample.tokens_per_second) {
                (true, _) => tr!(
                    "首字 {:.1} 秒，太慢",
                    "took {:.1}s to the first byte, too slow",
                    seconds(sample.first_byte_ms)
                ),
                (false, Some(tps)) => tr!(
                    "出字 {tps:.1} token/秒，太慢",
                    "output at {tps:.1} tokens/s, too slow"
                ),
                (false, None) => unreachable!("慢在速度时一定有速度"),
            };
            // 比记录的区域还慢：回到记录的区域，停在那里
            if let Some(rec) = &s.recorded
                && rec.region != region
                && worse(&sample, &rec.sample, slow_first_byte, slow_speed)
            {
                let to = rec.region.clone();
                s.settled = true;
                let why = tr!(
                    "{how}，比记录的 {to} 还慢",
                    "{how}, slower than the recorded {to}"
                );
                s.switch(model, &to, &why, now);
                return;
            }
            // 没有记录，或这里比记录的好：记下这里，换下一个没慢过的区域
            if !s.slow_tried.iter().any(|r| r == region) {
                s.slow_tried.push(region.to_owned());
            }
            s.recorded = Some(Recorded {
                region: region.to_owned(),
                sample,
            });
            let next = next_region(&order, region, |r| {
                s.slow_tried.iter().any(|t| t == r) || s.missing.iter().any(|m| m == r)
            });
            match next {
                Some(next) => s.switch(model, &next, &how, now),
                // 一圈都慢过：这里就是最好的，停在这里
                None => s.settled = true,
            }
        });
    }

    /// 离开过平时区域的模型此刻的状态。
    pub fn snapshot(&self, cfg: &RegionSwitchSettings, home: &str) -> Vec<ModelRegion> {
        let now = Instant::now();
        let mut out: Vec<ModelRegion> = self
            .lock()
            .iter()
            .filter(|(_, s)| !s.is_fresh(home))
            .filter_map(|(model, s)| {
                let away = now.saturating_duration_since(s.since?);
                Some(ModelRegion {
                    model: model.clone(),
                    region: s.current.clone(),
                    away_ms: away.as_millis() as u64,
                    reset_in_ms: reset_after(cfg).saturating_sub(away).as_millis() as u64,
                    recorded_region: s.recorded.as_ref().map(|r| r.region.clone()),
                    recorded_first_byte_ms: s.recorded.as_ref().map(|r| r.sample.first_byte_ms),
                    recorded_tokens_per_second: s
                        .recorded
                        .as_ref()
                        .and_then(|r| r.sample.tokens_per_second),
                    settled: s.settled,
                    missing: s.missing.clone(),
                })
            })
            .collect();
        out.sort_by(|a, b| a.model.cmp(&b.model));
        out
    }
}

/// 新的表现是否比记录的差：只比这次慢在的那一项。
fn worse(new: &Sample, recorded: &Sample, slow_first_byte: bool, slow_speed: bool) -> bool {
    let first_byte_worse = slow_first_byte && new.first_byte_ms > recorded.first_byte_ms;
    let speed_worse = slow_speed
        && matches!(
            (new.tokens_per_second, recorded.tokens_per_second),
            (Some(n), Some(r)) if n < r
        );
    first_byte_worse || speed_worse
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> RegionSwitchSettings {
        RegionSwitchSettings {
            enabled: true,
            regions: vec!["global".into(), "us".into(), "eu".into()],
            slow_first_byte_seconds: 10.0,
            slow_tokens_per_second: 20.0,
            reset_minutes: 30.0,
        }
    }

    const M: &str = "gemini-4-carbon";

    fn slow(first_byte_secs: u64) -> Sample {
        Sample {
            first_byte_ms: first_byte_secs * 1000,
            tokens_per_second: Some(100.0),
        }
    }

    fn fast() -> Sample {
        slow(1)
    }

    fn busy(sw: &RegionSwitch, region: &str, tried: &[&str], now: Instant) -> Option<String> {
        let tried: Vec<String> = tried.iter().map(|s| s.to_string()).collect();
        sw.on_trouble_at(&cfg(), "global", M, region, Trouble::Busy(429), &tried, now)
    }

    fn current(sw: &RegionSwitch, now: Instant) -> String {
        sw.region_for_at(&cfg(), "global", M, now)
    }

    fn observe(sw: &RegionSwitch, region: &str, sample: Sample, now: Instant) {
        sw.observe_at(&cfg(), "global", M, region, sample, now);
    }

    #[test]
    fn busy_moves_this_request_and_later_ones_to_the_next_region() {
        let sw = RegionSwitch::default();
        let t = Instant::now();
        assert_eq!(current(&sw, t), "global");
        assert_eq!(busy(&sw, "global", &["global"], t).as_deref(), Some("us"));
        assert_eq!(current(&sw, t), "us");
        // 这个请求在 us 也繁忙：再换 eu；一圈都试过就交还给繁忙重试
        assert_eq!(busy(&sw, "us", &["global", "us"], t).as_deref(), Some("eu"));
        assert_eq!(busy(&sw, "eu", &["global", "us", "eu"], t), None);
        assert_eq!(current(&sw, t), "eu");
        // 其他模型不受影响
        assert_eq!(sw.region_for_at(&cfg(), "global", "other", t), "global");
    }

    #[test]
    fn stale_trouble_does_not_move_the_shared_region() {
        let sw = RegionSwitch::default();
        let t = Instant::now();
        busy(&sw, "global", &["global"], t);
        // 早先发到 global 的另一个请求晚到的 429：它自己改发 us，共享状态仍是 us
        assert_eq!(busy(&sw, "global", &["global"], t).as_deref(), Some("us"));
        assert_eq!(current(&sw, t), "us");
    }

    #[test]
    fn slow_region_is_recorded_and_slower_next_region_switches_back() {
        let sw = RegionSwitch::default();
        let t = Instant::now();
        observe(&sw, "global", fast(), t);
        assert_eq!(current(&sw, t), "global");
        observe(&sw, "global", slow(20), t);
        assert_eq!(current(&sw, t), "us");
        // us 比记录的 global 还慢：回到 global 并停在那里
        observe(&sw, "us", slow(30), t);
        assert_eq!(current(&sw, t), "global");
        observe(&sw, "global", slow(40), t);
        assert_eq!(current(&sw, t), "global");
    }

    #[test]
    fn slow_but_better_region_becomes_the_record() {
        let sw = RegionSwitch::default();
        let t = Instant::now();
        observe(&sw, "global", slow(30), t);
        observe(&sw, "us", slow(20), t); // 比 global 好：记下 us，再试 eu
        assert_eq!(current(&sw, t), "eu");
        observe(&sw, "eu", slow(25), t); // 比 us 慢：回到 us
        assert_eq!(current(&sw, t), "us");
        let snap = sw.snapshot(&cfg(), "global");
        assert_eq!(snap[0].recorded_region.as_deref(), Some("us"));
        assert!(snap[0].settled);
    }

    #[test]
    fn all_slow_settles_on_the_best() {
        let sw = RegionSwitch::default();
        let t = Instant::now();
        observe(&sw, "global", slow(30), t);
        observe(&sw, "us", slow(20), t);
        observe(&sw, "eu", slow(15), t); // 一圈都慢，eu 最好：停在 eu
        assert_eq!(current(&sw, t), "eu");
        observe(&sw, "eu", slow(50), t);
        assert_eq!(current(&sw, t), "eu");
    }

    #[test]
    fn busy_after_slow_switch_goes_back_to_the_recorded_region() {
        let sw = RegionSwitch::default();
        let t = Instant::now();
        observe(&sw, "global", slow(20), t);
        assert_eq!(current(&sw, t), "us");
        assert_eq!(busy(&sw, "us", &["us"], t).as_deref(), Some("global"));
        assert_eq!(current(&sw, t), "global");
        // 记录的区域自己也繁忙了：记录作废，按顺序换下一个
        assert_eq!(busy(&sw, "global", &["global"], t).as_deref(), Some("us"));
        assert_eq!(current(&sw, t), "us");
    }

    #[test]
    fn slow_speed_counts_too() {
        let sw = RegionSwitch::default();
        let t = Instant::now();
        let crawl = |tps| Sample {
            first_byte_ms: 500,
            tokens_per_second: Some(tps),
        };
        observe(&sw, "global", crawl(25.0), t);
        assert_eq!(current(&sw, t), "global");
        observe(&sw, "global", crawl(10.0), t);
        assert_eq!(current(&sw, t), "us");
        observe(&sw, "us", crawl(5.0), t);
        assert_eq!(current(&sw, t), "global");
    }

    #[test]
    fn missing_model_skips_the_region_but_not_at_home() {
        let sw = RegionSwitch::default();
        let t = Instant::now();
        assert_eq!(
            sw.on_trouble_at(&cfg(), "global", M, "global", Trouble::Missing, &[], t),
            None
        );
        busy(&sw, "global", &["global"], t);
        let next = sw.on_trouble_at(
            &cfg(),
            "global",
            M,
            "us",
            Trouble::Missing,
            &["global".into(), "us".into()],
            t,
        );
        assert_eq!(next.as_deref(), Some("eu"));
        // 之后在 eu 繁忙，换区域时跳过没有这个模型的 us
        assert_eq!(busy(&sw, "eu", &["eu"], t).as_deref(), Some("global"));
    }

    #[test]
    fn resets_to_home_after_the_interval() {
        let sw = RegionSwitch::default();
        let t = Instant::now();
        observe(&sw, "global", slow(20), t);
        busy(&sw, "us", &["us"], t + Duration::from_secs(60));
        assert_eq!(current(&sw, t + Duration::from_secs(29 * 60)), "global");
        assert!(!sw.snapshot(&cfg(), "global").is_empty());
        // 从第一次离开算满 30 分钟：回到平时区域，记录清空
        let later = t + Duration::from_secs(30 * 60);
        busy(&sw, "global", &["global"], later - Duration::from_secs(1));
        assert_eq!(current(&sw, later), "global");
        observe(&sw, "global", slow(20), later);
        assert_eq!(current(&sw, later), "us");
    }

    #[test]
    fn home_outside_the_list_goes_first() {
        let mut c = cfg();
        c.regions = vec!["us".into(), "eu".into()];
        assert_eq!(order(&c, "global"), ["global", "us", "eu"]);
        c.regions = vec!["us".into(), " us ".into(), "eu".into(), "global".into()];
        assert_eq!(order(&c, "global"), ["us", "eu", "global"]);
        assert_eq!(
            next_region(&order(&c, "global"), "global", |_| false).as_deref(),
            Some("us")
        );
    }

    #[test]
    fn speed_needs_enough_tokens_and_time() {
        let s = Sample::new(800, Some(400), Duration::from_secs(4));
        assert_eq!(s.tokens_per_second, Some(100.0));
        assert_eq!(
            Sample::new(800, Some(10), Duration::from_secs(4)).tokens_per_second,
            None
        );
        assert_eq!(
            Sample::new(800, Some(400), Duration::from_millis(200)).tokens_per_second,
            None
        );
    }
}
