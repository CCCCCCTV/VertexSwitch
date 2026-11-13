//! 服务档位（Standard / Flex / Priority）：请求头规则与 trafficType 核对（SPEC §9），全部是纯函数。
//!
//! 依据：
//! - <https://docs.cloud.google.com/vertex-ai/generative-ai/docs/flex-paygo>
//! - <https://docs.cloud.google.com/vertex-ai/generative-ai/docs/priority-paygo>
//!
//! 要点：`X-Vertex-AI-LLM-Shared-Request-Type: flex|priority` 选档位（只带它时先用预配吞吐量）；
//! 再加 `X-Vertex-AI-LLM-Request-Type: shared` 则只走 PayGo。Flex 只支持 global 且不支持部分
//! 新模型；Priority 支持 global / us / eu。只有 Gemini 原生生成请求支持这些头。

use std::sync::LazyLock;

use crate::tr;
use regex::bytes::Regex;

pub const HEADER_REQUEST_TYPE: &str = "X-Vertex-AI-LLM-Request-Type";
pub const HEADER_SHARED_TYPE: &str = "X-Vertex-AI-LLM-Shared-Request-Type";
pub const HEADER_SERVER_TIMEOUT: &str = "X-Server-Timeout";

/// 档位告警写进日志时带的 `kind` 字段，界面按它识别降级：发请求前就改用了 Standard，
/// 或上游实际按别的档位处理。日志另带 `model` 字段。
pub const LOG_KIND_FALLBACK: &str = "tier_fallback";
pub const LOG_KIND_MISMATCH: &str = "tier_mismatch";

pub const FLEX_MAX_TIMEOUT: u64 = 30 * 60;
const FLEX_REGIONS: &[&str] = &["global"];
const PRIORITY_REGIONS: &[&str] = &["global", "us", "eu"];

/// 不支持 Flex 的模型：Flex PayGo 文档的支持列表里没有，argon 另有明确注明（2026-11-14 核对）。
/// 不管配置里的 `gemini.flex_unsupported_models` 怎么写都按不支持处理；读配置时也会并进那份名单，
/// 旧配置文件里存的名单不会因为出了新模型而过时，界面上也看得到。
pub const FLEX_UNSUPPORTED_MODELS: &[&str] = &["gemini-4-carbon", "gemini-4-argon"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Standard,
    Flex,
    Priority,
}

impl Tier {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "standard" => Some(Self::Standard),
            "flex" => Some(Self::Flex),
            "priority" => Some(Self::Priority),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Flex => "flex",
            Self::Priority => "priority",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierPlan {
    pub requested: Tier,
    /// 实际按哪个档位发请求头（降级后可能是 Standard）
    pub effective: Tier,
    pub headers: Vec<(&'static str, String)>,
    pub shared_only: bool,
    pub warnings: Vec<String>,
    /// 发了 Flex 头时，本地读超时不能小于这个值（秒）
    pub server_timeout: Option<u64>,
}

impl TierPlan {
    pub fn sent_tier_header(&self) -> bool {
        self.headers.iter().any(|(k, _)| *k == HEADER_SHARED_TYPE)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.as_str())
    }
}

pub struct TierOptions<'a> {
    pub shared_only: bool,
    pub flex_timeout: u64,
    pub flex_unsupported_models: &'a [String],
}

/// 根据档位、模型、区域算出要发的请求头。不支持时降级为 Standard 并给出告警，且什么头都不发。
pub fn plan_tier(tier: Tier, model: &str, region: &str, opts: &TierOptions<'_>) -> TierPlan {
    let mut warnings = Vec::new();
    let mut effective = tier;
    match tier {
        Tier::Flex if !FLEX_REGIONS.contains(&region) => {
            warnings.push(tr!(
                "Flex 只支持 global 端点，当前区域 {region}，已改用 Standard", "Flex only works on the global endpoint (current region: {region}); using Standard instead"
            ));
            effective = Tier::Standard;
        }
        Tier::Flex
            if FLEX_UNSUPPORTED_MODELS.contains(&model)
                || opts.flex_unsupported_models.iter().any(|m| m == model) =>
        {
            warnings.push(tr!(
                "模型 {model} 不支持 Flex，已改用 Standard",
                "Model {model} doesn't support Flex; using Standard instead"
            ));
            effective = Tier::Standard;
        }
        Tier::Priority if !PRIORITY_REGIONS.contains(&region) => {
            warnings.push(tr!(
                "Priority 只支持 global / us / eu 端点，当前区域 {region}，已改用 Standard", "Priority only works on the global / us / eu endpoints (current region: {region}); using Standard instead"
            ));
            effective = Tier::Standard;
        }
        _ => {}
    }

    let downgraded = effective != tier;
    let shared_only = opts.shared_only && !downgraded;
    let mut headers = Vec::new();
    let mut server_timeout = None;
    if shared_only {
        headers.push((HEADER_REQUEST_TYPE, "shared".to_owned()));
    }
    if effective != Tier::Standard {
        headers.push((HEADER_SHARED_TYPE, effective.as_str().to_owned()));
    }
    if effective == Tier::Flex {
        let timeout = opts.flex_timeout.clamp(1, FLEX_MAX_TIMEOUT);
        headers.push((HEADER_SERVER_TIMEOUT, timeout.to_string()));
        server_timeout = Some(timeout);
    }
    TierPlan {
        requested: tier,
        effective,
        headers,
        shared_only,
        warnings,
        server_timeout,
    }
}

/// 核对响应里的 `usageMetadata.trafficType`，不符时返回告警文字。只在真的发了档位头时核对。
pub fn check_traffic_type(plan: &TierPlan, traffic_type: &str) -> Option<String> {
    if !plan.sent_tier_header() {
        return None;
    }
    let expected = match plan.effective {
        Tier::Flex => "ON_DEMAND_FLEX",
        Tier::Priority => "ON_DEMAND_PRIORITY",
        Tier::Standard => return None,
    };
    if traffic_type == expected {
        return None;
    }
    // 未加 shared 头时先用预配吞吐量是文档描述的正常行为
    if traffic_type == "PROVISIONED_THROUGHPUT" && !plan.shared_only {
        return None;
    }
    Some(tr!(
        "请求了 {} 档位，但上游实际按 {traffic_type} 处理（期望 {expected}）",
        "Requested the {} tier, but upstream served it as {traffic_type} (expected {expected})",
        plan.effective.as_str()
    ))
}

static TRAFFIC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""trafficType"\s*:\s*"([A-Z_]+)""#).expect("合法正则"));

/// 从（可能被任意切分的）响应字节流里找出第一个 trafficType。
#[derive(Debug, Default)]
pub struct TrafficTypeWatcher {
    pub traffic_type: Option<String>,
    tail: Vec<u8>,
}

impl TrafficTypeWatcher {
    const KEEP: usize = 64;

    /// 第一次找到时返回它，之后都返回 None。
    pub fn feed(&mut self, chunk: &[u8]) -> Option<String> {
        if self.traffic_type.is_some() {
            return None;
        }
        self.tail.extend_from_slice(chunk);
        if let Some(found) = TRAFFIC_RE.captures(&self.tail) {
            let value = String::from_utf8_lossy(&found[1]).into_owned();
            self.traffic_type = Some(value.clone());
            self.tail.clear();
            return Some(value);
        }
        let keep_from = self.tail.len().saturating_sub(Self::KEEP);
        self.tail.drain(..keep_from);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unsupported() -> Vec<String> {
        vec!["gemini-4-argon".into()]
    }

    fn plan(
        tier: Tier,
        model: &str,
        region: &str,
        shared_only: bool,
        flex_timeout: u64,
    ) -> TierPlan {
        let models = unsupported();
        plan_tier(
            tier,
            model,
            region,
            &TierOptions {
                shared_only,
                flex_timeout,
                flex_unsupported_models: &models,
            },
        )
    }

    fn simple(tier: Tier) -> TierPlan {
        plan(tier, "gemini-3.5-flash", "global", false, 1800)
    }

    #[test]
    fn standard_sends_nothing() {
        let p = simple(Tier::Standard);
        assert!(p.headers.is_empty());
        assert!(!p.sent_tier_header());
    }

    #[test]
    fn standard_shared_only() {
        let p = plan(Tier::Standard, "gemini-3.5-flash", "global", true, 1800);
        assert_eq!(p.headers, vec![(HEADER_REQUEST_TYPE, "shared".to_owned())]);
    }

    #[test]
    fn flex_headers_and_timeout() {
        let p = simple(Tier::Flex);
        assert_eq!(
            p.headers,
            vec![
                (HEADER_SHARED_TYPE, "flex".to_owned()),
                (HEADER_SERVER_TIMEOUT, "1800".to_owned())
            ]
        );
        assert_eq!(p.server_timeout, Some(1800));
        assert!(p.warnings.is_empty());
    }

    #[test]
    fn flex_timeout_clamped() {
        let p = plan(Tier::Flex, "gemini-3.5-flash", "global", false, 99_999);
        assert_eq!(p.header(HEADER_SERVER_TIMEOUT), Some("1800"));
        let p = plan(Tier::Flex, "gemini-3.5-flash", "global", false, 0);
        assert_eq!(p.header(HEADER_SERVER_TIMEOUT), Some("1"));
    }

    #[test]
    fn flex_shared_only() {
        let p = plan(Tier::Flex, "gemini-3.5-flash", "global", true, 600);
        assert_eq!(p.header(HEADER_REQUEST_TYPE), Some("shared"));
        assert_eq!(p.header(HEADER_SHARED_TYPE), Some("flex"));
        assert_eq!(p.header(HEADER_SERVER_TIMEOUT), Some("600"));
    }

    #[test]
    fn flex_non_global_downgrades_without_any_header() {
        for region in ["us", "eu", "us-central1"] {
            let p = plan(Tier::Flex, "gemini-3.5-flash", region, true, 1800);
            assert_eq!(p.effective, Tier::Standard);
            assert!(p.headers.is_empty());
            assert_eq!(p.server_timeout, None);
            assert!(p.warnings[0].contains("global"));
        }
    }

    #[test]
    fn flex_unsupported_model_downgrades() {
        let p = plan(Tier::Flex, "gemini-4-argon", "global", true, 1800);
        assert_eq!(p.effective, Tier::Standard);
        assert!(p.headers.is_empty());
        assert!(p.warnings[0].contains("gemini-4-argon"));
    }

    #[test]
    fn builtin_flex_unsupported_applies_without_config() {
        // 旧配置文件里的名单没有 carbon，照样降级；hydrogen 在 Flex 支持列表里，照常发 flex
        let carbon = plan_tier(
            Tier::Flex,
            "gemini-4-carbon",
            "global",
            &TierOptions {
                shared_only: false,
                flex_timeout: 1800,
                flex_unsupported_models: &[],
            },
        );
        assert_eq!(carbon.effective, Tier::Standard);
        assert!(carbon.headers.is_empty());
        let hydrogen = plan(Tier::Flex, "gemini-4-hydrogen", "global", false, 1800);
        assert_eq!(hydrogen.effective, Tier::Flex);
        assert!(hydrogen.warnings.is_empty());
    }

    #[test]
    fn priority_regions() {
        for region in ["global", "us", "eu"] {
            let p = plan(Tier::Priority, "gemini-3.5-flash", region, false, 1800);
            assert_eq!(p.headers, vec![(HEADER_SHARED_TYPE, "priority".to_owned())]);
            assert!(p.warnings.is_empty());
        }
        let p = plan(Tier::Priority, "gemini-3.5-flash", "us-east5", false, 1800);
        assert_eq!(p.effective, Tier::Standard);
        assert!(p.headers.is_empty());
    }

    #[test]
    fn priority_on_argon_is_fine() {
        let p = plan(Tier::Priority, "gemini-4-argon", "global", false, 1800);
        assert_eq!(p.effective, Tier::Priority);
    }

    #[test]
    fn parse() {
        assert_eq!(Tier::parse("FLEX"), Some(Tier::Flex));
        assert_eq!(Tier::parse(" priority "), Some(Tier::Priority));
        assert_eq!(Tier::parse("bogus"), None);
    }

    #[test]
    fn traffic_type_checks() {
        let cases = [
            (Tier::Flex, false, "ON_DEMAND_FLEX", false),
            (Tier::Flex, false, "ON_DEMAND", true),
            (Tier::Flex, false, "PROVISIONED_THROUGHPUT", false),
            (Tier::Flex, true, "PROVISIONED_THROUGHPUT", true),
            (Tier::Priority, false, "ON_DEMAND_PRIORITY", false),
            (Tier::Priority, false, "ON_DEMAND", true),
            (Tier::Standard, false, "ON_DEMAND", false),
            (Tier::Standard, true, "PROVISIONED_THROUGHPUT", false),
        ];
        for (tier, shared_only, traffic, warns) in cases {
            let p = plan(tier, "gemini-3.5-flash", "global", shared_only, 1800);
            assert_eq!(
                check_traffic_type(&p, traffic).is_some(),
                warns,
                "{tier:?} shared_only={shared_only} {traffic}"
            );
        }
    }

    #[test]
    fn no_check_when_downgraded() {
        let p = plan(Tier::Flex, "gemini-3.5-flash", "us", false, 1800);
        assert_eq!(check_traffic_type(&p, "ON_DEMAND"), None);
    }

    #[test]
    fn watcher_split_chunks_reports_once() {
        let mut w = TrafficTypeWatcher::default();
        assert_eq!(w.feed(br#"data: {"usageMetadata": {"traffic"#), None);
        assert_eq!(
            w.feed(b"Type\": \"ON_DEMAND_FLEX\"}}\r\n\r\n").as_deref(),
            Some("ON_DEMAND_FLEX")
        );
        assert_eq!(w.feed(br#"{"trafficType": "ON_DEMAND"}"#), None);
        assert_eq!(w.traffic_type.as_deref(), Some("ON_DEMAND_FLEX"));
    }

    #[test]
    fn watcher_bounded_memory() {
        let mut w = TrafficTypeWatcher::default();
        for _ in 0..1000 {
            w.feed(&[b'x'; 1024]);
        }
        assert!(w.tail.len() <= TrafficTypeWatcher::KEEP);
    }
}
