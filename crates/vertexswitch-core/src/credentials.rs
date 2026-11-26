//! 换凭证（SPEC §9b）：配了备用凭证时，按模型记住当前用第几个凭证，出问题时换下一个。
//!
//! 平时一直用第一个凭证（同一个项目里隐式缓存能持续命中）。当前凭证出了凭证本身的问题
//! （取 token 失败、401 / 403、平时区域 404），或者 429 / 503 在所有区域都试过了，这个请求
//! 就改用下一个凭证，之后的请求也跟着换。取 token 失败的凭证在重置前一直跳过。
//! 第一次离开第一个凭证满 `google.credential_reset_minutes` 分钟后回到它。
//!
//! 和区域切换一样，只在出问题的就是当前凭证时才改共享状态。凭证用从 0 开始的序号表示，
//! 0 是 `google.credentials_file`，之后依次是 `google.backup_credentials`。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::tr;

/// 换凭证写日志时带的 `kind` 字段。日志另带 `model`、`from`、`to`（从 1 数的凭证号）。
pub const LOG_KIND_SWITCH: &str = "credential_switch";

/// 凭证出了什么问题。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trouble {
    /// 429 / 503，而且所有区域都试过了
    Busy(u16),
    /// 401 / 403
    Denied(u16),
    /// 平时区域 404（这个项目用不了这个模型）
    Missing,
    /// 取 token 失败（密钥文件读不了、无效、换 token 被拒），带具体原因写进日志
    TokenFailed(String),
}

impl Trouble {
    fn describe(&self) -> String {
        match self {
            Trouble::Busy(status) => tr!(
                "在所有区域都返回 {status}（繁忙）",
                "returned {status} (busy) in every region"
            ),
            Trouble::Denied(status) => tr!(
                "返回 {status}（没有权限）",
                "returned {status} (not allowed)"
            ),
            Trouble::Missing => tr!("用不了这个模型（404）", "can't use this model (404)"),
            Trouble::TokenFailed(reason) => tr!(
                "取 token 失败（{reason}）",
                "couldn't get a token ({reason})"
            ),
        }
    }
}

/// 之后的请求换凭证时写的日志。
fn switch_message(model: &str, from: &str, to: &str, trouble: &Trouble) -> String {
    let why = trouble.describe();
    tr!(
        "{model} 用{from}时{why}，之后的请求改用{to}",
        "{model} with {from} {why}; later requests use {to}"
    )
}

#[derive(Debug, Clone, Default)]
struct ModelState {
    current: usize,
    /// 第一次离开第一个凭证的时刻，重置间隔从这里算
    since: Option<Instant>,
    /// 取 token 失败的凭证，重置前跳过
    broken: Vec<usize>,
}

/// 某个模型此刻用的备用凭证（给界面看）。只列离开过第一个凭证的模型。凭证号从 1 数。
#[derive(Debug, Clone, Serialize)]
pub struct ModelCredential {
    pub model: String,
    pub credential: usize,
    pub away_ms: u64,
    pub reset_in_ms: u64,
    pub broken: Vec<usize>,
}

#[derive(Debug, Default)]
pub struct CredentialSwitch {
    states: Mutex<HashMap<String, ModelState>>,
}

fn reset_after(minutes: f64) -> Duration {
    Duration::from_secs_f64((minutes * 60.0).max(0.0))
}

impl CredentialSwitch {
    /// 凭证配置变了：所有模型回到第一个凭证。
    pub fn reset(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, ModelState>> {
        self.states.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn with_state<T>(
        &self,
        model: &str,
        count: usize,
        reset_minutes: f64,
        now: Instant,
        f: impl FnOnce(&mut ModelState) -> T,
    ) -> T {
        let mut states = self.lock();
        let state = states.entry(model.to_owned()).or_default();
        let expired = state.since.is_some_and(|since| {
            now.saturating_duration_since(since) >= reset_after(reset_minutes)
        });
        if expired || state.current >= count {
            if expired && state.current != 0 {
                tracing::info!(
                    kind = LOG_KIND_SWITCH,
                    model = %model,
                    from = state.current + 1,
                    to = 1,
                    "{}",
                    tr!(
                        "{model} 离开第一个凭证满 {reset_minutes} 分钟，回到凭证 1",
                        "{model} has been off the first credential for {reset_minutes} minutes; back to credential 1"
                    )
                );
            }
            *state = ModelState::default();
        }
        f(state)
    }

    /// 这个模型现在该用第几个凭证（从 0 数）。
    pub fn current(&self, model: &str, count: usize, reset_minutes: f64) -> usize {
        self.current_at(model, count, reset_minutes, Instant::now())
    }

    fn current_at(&self, model: &str, count: usize, reset_minutes: f64, now: Instant) -> usize {
        if count <= 1 {
            return 0;
        }
        self.with_state(model, count, reset_minutes, now, |s| s.current)
    }

    /// 请求用凭证 `failed` 出了问题：返回这个请求接下来改用的凭证（`tried` 是这个请求这一轮已经用过的），
    /// 没有可换的时返回 None。出问题的是当前凭证时，之后的请求也跟着换。`label` 给日志写凭证名。
    #[allow(clippy::too_many_arguments)]
    pub fn on_trouble(
        &self,
        model: &str,
        failed: usize,
        trouble: Trouble,
        tried: &[usize],
        count: usize,
        reset_minutes: f64,
        label: impl Fn(usize) -> String,
    ) -> Option<usize> {
        self.on_trouble_at(
            model,
            failed,
            trouble,
            tried,
            count,
            reset_minutes,
            label,
            Instant::now(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn on_trouble_at(
        &self,
        model: &str,
        failed: usize,
        trouble: Trouble,
        tried: &[usize],
        count: usize,
        reset_minutes: f64,
        label: impl Fn(usize) -> String,
        now: Instant,
    ) -> Option<usize> {
        if count <= 1 {
            return None;
        }
        self.with_state(model, count, reset_minutes, now, |s| {
            if matches!(trouble, Trouble::TokenFailed(_)) && !s.broken.contains(&failed) {
                s.broken.push(failed);
            }
            let next = (1..count)
                .map(|k| (failed + k) % count)
                .find(|i| !tried.contains(i) && !s.broken.contains(i))?;
            if s.current == failed {
                s.current = next;
                s.since.get_or_insert(now);
                tracing::warn!(
                    kind = LOG_KIND_SWITCH,
                    model = %model,
                    from = failed + 1,
                    to = next + 1,
                    "{}",
                    switch_message(model, &label(failed), &label(next), &trouble)
                );
            }
            Some(next)
        })
    }

    /// 离开过第一个凭证的模型此刻的状态。
    pub fn snapshot(&self, count: usize, reset_minutes: f64) -> Vec<ModelCredential> {
        let now = Instant::now();
        let mut out: Vec<ModelCredential> = self
            .lock()
            .iter()
            .filter(|(_, s)| s.current < count)
            .filter_map(|(model, s)| {
                let away = now.saturating_duration_since(s.since?);
                Some(ModelCredential {
                    model: model.clone(),
                    credential: s.current + 1,
                    away_ms: away.as_millis() as u64,
                    reset_in_ms: reset_after(reset_minutes).saturating_sub(away).as_millis() as u64,
                    broken: s.broken.iter().map(|i| i + 1).collect(),
                })
            })
            .collect();
        out.sort_by(|a, b| a.model.cmp(&b.model));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: &str = "gemini-4-carbon";
    const RESET: f64 = 30.0;

    fn label(i: usize) -> String {
        format!("凭证 {}", i + 1)
    }

    fn trouble(
        sw: &CredentialSwitch,
        failed: usize,
        t: Trouble,
        tried: &[usize],
        now: Instant,
    ) -> Option<usize> {
        sw.on_trouble_at(M, failed, t, tried, 3, RESET, label, now)
    }

    #[test]
    fn single_credential_never_switches() {
        let sw = CredentialSwitch::default();
        let t = Instant::now();
        assert_eq!(sw.current_at(M, 1, RESET, t), 0);
        assert_eq!(
            sw.on_trouble_at(M, 0, Trouble::Denied(403), &[0], 1, RESET, label, t),
            None
        );
    }

    #[test]
    fn trouble_moves_this_request_and_later_ones() {
        let sw = CredentialSwitch::default();
        let t = Instant::now();
        assert_eq!(trouble(&sw, 0, Trouble::Busy(429), &[0], t), Some(1));
        assert_eq!(sw.current_at(M, 3, RESET, t), 1);
        assert_eq!(trouble(&sw, 1, Trouble::Denied(403), &[0, 1], t), Some(2));
        assert_eq!(trouble(&sw, 2, Trouble::Busy(429), &[0, 1, 2], t), None);
        assert_eq!(sw.current_at(M, 3, RESET, t), 2);
        // 其他模型不受影响
        assert_eq!(sw.current_at("other", 3, RESET, t), 0);
    }

    #[test]
    fn stale_trouble_only_moves_its_own_request() {
        let sw = CredentialSwitch::default();
        let t = Instant::now();
        trouble(&sw, 0, Trouble::Busy(429), &[0], t);
        // 早先用凭证 1 的另一个请求晚到的 429：它自己改用凭证 2，共享状态不变
        assert_eq!(trouble(&sw, 0, Trouble::Busy(429), &[0], t), Some(1));
        assert_eq!(sw.current_at(M, 3, RESET, t), 1);
    }

    #[test]
    fn broken_credentials_are_skipped_until_reset() {
        let sw = CredentialSwitch::default();
        let t = Instant::now();
        // 凭证 2 取不到 token：这个请求接着换凭证 3
        assert_eq!(trouble(&sw, 0, Trouble::Denied(403), &[0], t), Some(1));
        assert_eq!(
            trouble(&sw, 1, Trouble::TokenFailed("HTTP 400".into()), &[0, 1], t),
            Some(2)
        );
        // 之后凭证 3 繁忙：跳过坏掉的凭证 2，回到凭证 1
        assert_eq!(trouble(&sw, 2, Trouble::Busy(429), &[2], t), Some(0));
        let snap = sw.snapshot(3, RESET);
        assert_eq!(snap[0].broken, [2]);
    }

    #[test]
    fn token_failure_log_says_why() {
        let reason = r#"HTTP 400 Bad Request {"error":"invalid_grant","error_description":"Invalid grant: account not found"}"#;
        let message = switch_message(
            M,
            "凭证 2（proj-b）",
            "凭证 3（proj-c）",
            &Trouble::TokenFailed(reason.into()),
        );
        assert!(message.contains(reason), "{message}");
        assert!(message.contains("凭证 2（proj-b）") && message.contains("凭证 3（proj-c）"));
    }

    #[test]
    fn resets_to_the_first_after_the_interval() {
        let sw = CredentialSwitch::default();
        let t = Instant::now();
        trouble(&sw, 0, Trouble::Busy(429), &[0], t);
        assert_eq!(
            sw.current_at(M, 3, RESET, t + Duration::from_secs(29 * 60)),
            1
        );
        assert_eq!(
            sw.current_at(M, 3, RESET, t + Duration::from_secs(30 * 60)),
            0
        );
        assert!(sw.snapshot(3, RESET).is_empty());
    }

    #[test]
    fn fewer_credentials_after_a_config_change_start_over() {
        let sw = CredentialSwitch::default();
        let t = Instant::now();
        trouble(&sw, 0, Trouble::Busy(429), &[0], t);
        trouble(&sw, 1, Trouble::Busy(429), &[0, 1], t);
        assert_eq!(sw.current_at(M, 3, RESET, t), 2);
        assert_eq!(sw.current_at(M, 2, RESET, t), 0);
    }
}
