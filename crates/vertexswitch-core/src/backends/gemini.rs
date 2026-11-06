//! Gemini 原生 generateContent 后端（Vertex AI publishers/google）。唯一支持服务档位头的通道。

use std::time::Duration;

use serde_json::Value;

use crate::config::Settings;
use crate::tiers::{
    LOG_KIND_FALLBACK, LOG_KIND_MISMATCH, Tier, TierOptions, TierPlan, TrafficTypeWatcher,
    check_traffic_type, plan_tier,
};
use crate::tr;

/// 客户端按请求覆盖档位的请求头。
pub const TIER_OVERRIDE_HEADER: &str = "x-vertexswitch-tier";
/// Flex 排队到 X-Server-Timeout 才返回时，本地多等这么久再判超时
pub(crate) const TIMEOUT_MARGIN: Duration = Duration::from_secs(60);

pub const GENERATE_ACTIONS: &[&str] = &["generateContent", "streamGenerateContent"];
pub const ACTIONS: &[&str] = &["generateContent", "streamGenerateContent", "countTokens"];

/// 某个模型按配置该用的档位：按模型单独设的优先，否则用默认档位。
pub fn configured_tier(settings: &Settings, model: &str) -> Tier {
    let g = &settings.gemini;
    g.model_tiers
        .get(model)
        .and_then(|t| Tier::parse(t))
        .or_else(|| Tier::parse(&g.tier))
        .unwrap_or(Tier::Standard)
}

/// 档位头只用于生成类请求；countTokens 等不发。
/// 优先级：请求头 `X-VertexSwitch-Tier`（`override_tier`）> 按模型设的档位 > 默认档位。
pub fn tier_plan(
    settings: &Settings,
    model: &str,
    action: &str,
    override_tier: Option<&str>,
) -> Option<TierPlan> {
    if !GENERATE_ACTIONS.contains(&action) {
        return None;
    }
    let g = &settings.gemini;
    let tier = override_tier
        .and_then(Tier::parse)
        .unwrap_or_else(|| configured_tier(settings, model));
    Some(plan_tier(
        tier,
        model,
        settings.gemini_region(),
        &TierOptions {
            shared_only: g.shared_only,
            flex_timeout: g.flex_server_timeout,
            flex_unsupported_models: &g.flex_unsupported_models,
        },
    ))
}

/// OpenAI 格式的 Gemini 请求走哪条路：要发档位头（或 `openai_native = always`）时转成原生接口，
/// 否则走 OpenAI 兼容接口（那里带档位头会被 Vertex 拒绝）。走兼容接口时档位告警在这里记，
/// 走原生接口时由 `backends::call` 记。
pub fn openai_wants_native(settings: &Settings, model: &str, override_tier: Option<&str>) -> bool {
    let plan =
        tier_plan(settings, model, "generateContent", override_tier).expect("生成请求总有档位方案");
    let native = match settings.gemini.openai_native.as_str() {
        "always" => true,
        "never" => false,
        _ => !plan.headers.is_empty(),
    };
    if !native {
        for warning in &plan.warnings {
            tracing::warn!(model = %model, kind = LOG_KIND_FALLBACK, "{warning}");
        }
        if plan.sent_tier_header() {
            tracing::warn!(
                model = %model,
                kind = LOG_KIND_FALLBACK,
                "{}",
                tr!(
                    "gemini.openai_native = never，OpenAI 兼容接口带不了档位头，本次按 Standard 发送",
                    "gemini.openai_native = never: the OpenAI-compatible endpoint can't carry tier headers, so this request goes out as Standard"
                )
            );
        }
    }
    native
}

/// 收到第一个 trafficType 时核对一次档位；没有档位方案（countTokens、Claude 等）时什么也不做。
pub(crate) struct TrafficCheck {
    plan: Option<TierPlan>,
    model: String,
    watcher: TrafficTypeWatcher,
}

impl TrafficCheck {
    pub(crate) fn new(plan: Option<TierPlan>, model: String) -> Self {
        Self {
            plan,
            model,
            watcher: TrafficTypeWatcher::default(),
        }
    }

    pub(crate) fn feed(&mut self, chunk: &[u8]) {
        let Some(plan) = &self.plan else {
            return;
        };
        if let Some(traffic) = self.watcher.feed(chunk)
            && let Some(warning) = check_traffic_type(plan, &traffic)
        {
            tracing::warn!(model = %self.model, kind = LOG_KIND_MISMATCH, "{warning}");
        }
    }
}

/// Gemini API 允许 contents 省略 role，Vertex 要求写明；缺省补 "user"。
pub fn prepare_body(body: &mut Value) {
    let Some(contents) = body.get_mut("contents").and_then(Value::as_array_mut) else {
        return;
    };
    for item in contents.iter_mut().filter_map(Value::as_object_mut) {
        let has_role = item
            .get("role")
            .and_then(Value::as_str)
            .is_some_and(|r| !r.is_empty());
        if !has_role {
            item.insert("role".into(), "user".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn fills_missing_role_only() {
        let mut body = json!({"contents": [{"parts": []}, {"role": "model", "parts": []}, {"role": "", "parts": []}]});
        prepare_body(&mut body);
        assert_eq!(body["contents"][0]["role"], "user");
        assert_eq!(body["contents"][1]["role"], "model");
        assert_eq!(body["contents"][2]["role"], "user");
    }

    #[test]
    fn model_tier_beats_default_and_header_beats_both() {
        let mut s = Settings::default();
        s.gemini.tier = "priority".into();
        s.gemini
            .model_tiers
            .insert("gemini-3.5-flash".into(), "flex".into());
        let plan = |model, header| tier_plan(&s, model, "generateContent", header).unwrap();
        assert_eq!(plan("gemini-3.5-flash", None).effective, Tier::Flex);
        assert_eq!(plan("gemini-3.5-pro", None).effective, Tier::Priority);
        assert_eq!(
            plan("gemini-3.5-flash", Some("standard")).effective,
            Tier::Standard
        );
        // 请求头写错时退回配置
        assert_eq!(
            plan("gemini-3.5-flash", Some("bogus")).effective,
            Tier::Flex
        );
    }

    #[test]
    fn count_tokens_has_no_plan() {
        let s = Settings::default();
        assert!(tier_plan(&s, "m", "countTokens", None).is_none());
        let p = tier_plan(&s, "m", "generateContent", Some("priority")).unwrap();
        assert_eq!(p.effective, Tier::Priority);
    }
}
