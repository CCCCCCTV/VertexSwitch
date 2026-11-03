//! Gemini 原生 generateContent 后端（Vertex AI publishers/google）。唯一支持服务档位头的通道。

use serde_json::Value;

use crate::config::Settings;
use crate::tiers::{Tier, TierOptions, TierPlan, plan_tier};

pub const GENERATE_ACTIONS: &[&str] = &["generateContent", "streamGenerateContent"];
pub const ACTIONS: &[&str] = &["generateContent", "streamGenerateContent", "countTokens"];

/// 档位头只用于生成类请求；countTokens 等不发。`override_tier` 来自请求头，优先于配置。
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
        .or_else(|| Tier::parse(&g.tier))
        .unwrap_or(Tier::Standard);
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
    fn count_tokens_has_no_plan() {
        let s = Settings::default();
        assert!(tier_plan(&s, "m", "countTokens", None).is_none());
        let p = tier_plan(&s, "m", "generateContent", Some("priority")).unwrap();
        assert_eq!(p.effective, Tier::Priority);
    }
}
