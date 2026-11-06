//! Gemini 的思考设置（SPEC §13、§14）。
//!
//! Gemini 3 起用思考等级（`thinkingLevel`：MINIMAL / LOW / MEDIUM / HIGH）；思考预算（`thinkingBudget`）
//! 已弃用：Gemini 4 收到预算直接返回 400，Gemini 3 偶尔 400，两者同时给也报错。更早的模型（2.5）只认预算。
//! 各模型支持的等级不同（Vertex 文档 Thinking 页的 “Supported thinking levels by model”），
//! 请求的等级不支持时换成最近的一档（距离相同取更高的一档）。客户端什么都没指定时不发，用模型自己的默认值。
//!
//! 参考：<https://docs.cloud.google.com/vertex-ai/generative-ai/docs/thinking>

use serde_json::{Map, Value};

/// 想要的思考力度（从小到大）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effort {
    Minimal,
    Low,
    Medium,
    High,
}

use Effort::{High, Low, Medium, Minimal};

const ALL: &[Effort] = &[Minimal, Low, Medium, High];
const NO_MINIMAL: &[Effort] = &[Low, Medium, High];

/// 支持的等级不是全部四档的模型（按名字前缀，先列更长的前缀）；其余 Gemini 3+ 四档都支持。
const LEVELS_BY_MODEL: &[(&str, &[Effort])] = &[
    ("gemini-3-pro-image", &[High]),
    ("gemini-3.1-flash-image", &[Minimal, High]),
    ("gemini-3.1-flash-lite-image", &[Minimal, High]),
    ("gemini-3.1-pro", NO_MINIMAL),
    ("gemini-3.7-flash", NO_MINIMAL),
    ("gemini-3.8-flash", NO_MINIMAL),
    ("gemini-4", NO_MINIMAL),
];

impl Effort {
    fn level(self) -> &'static str {
        match self {
            Minimal => "MINIMAL",
            Low => "LOW",
            Medium => "MEDIUM",
            High => "HIGH",
        }
    }

    /// 老模型用的预算（与 Vertex 的 OpenAI 兼容接口对 reasoning_effort 的映射一致）。
    fn budget(self) -> u64 {
        match self {
            Minimal => 512,
            Low => 1024,
            Medium => 8192,
            High => 24576,
        }
    }

    /// Claude 格式的 `thinking.budget_tokens` 折成力度。
    pub fn from_budget(tokens: u64) -> Self {
        match tokens {
            0..4096 => Low,
            4096..16384 => Medium,
            _ => High,
        }
    }

    /// OpenAI 格式的 `reasoning_effort`；认不出来的值返回 None（不发思考设置）。
    pub fn from_reasoning_effort(effort: &str) -> Option<Self> {
        Some(match effort {
            // Gemini 3 起关不掉思考，最少就是 MINIMAL
            "none" | "minimal" => Minimal,
            "low" => Low,
            "medium" => Medium,
            "high" | "xhigh" => High,
            _ => return None,
        })
    }
}

/// `gemini-<主版本>…` 的主版本号。
pub fn gemini_major(model: &str) -> Option<u32> {
    model
        .strip_prefix("gemini-")
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|major| major.parse().ok())
}

fn uses_levels(model: &str) -> bool {
    gemini_major(model).is_some_and(|major| major >= 3)
}

fn supported(model: &str) -> &'static [Effort] {
    LEVELS_BY_MODEL
        .iter()
        .find(|(prefix, _)| model.starts_with(prefix))
        .map_or(ALL, |(_, levels)| levels)
}

/// 最近的一档；距离相同取更高的一档。
fn nearest(supported: &[Effort], wanted: Effort) -> Effort {
    let distance = |e: &Effort| (*e as i32 - wanted as i32).abs();
    *supported
        .iter()
        .min_by_key(|e| (distance(e), std::cmp::Reverse(**e)))
        .unwrap_or(&wanted)
}

/// 老模型允许的预算范围（2.5 Pro 关不掉思考）。
fn budget_range(model: &str) -> (u64, u64) {
    if model.contains("pro") {
        (128, 32768)
    } else if model.contains("flash-lite") {
        (512, 24576)
    } else {
        (1, 24576)
    }
}

/// `generationConfig.thinkingConfig`：Gemini 3 起发 `thinkingLevel`，更早的模型发 `thinkingBudget`
/// （客户端给了具体预算就用它，夹在模型允许的范围内）。`include_thoughts` 时要回思考摘要。
pub fn thinking_config(
    model: &str,
    effort: Effort,
    budget: Option<u64>,
    include_thoughts: bool,
) -> Value {
    let mut config = Map::new();
    if include_thoughts {
        config.insert("includeThoughts".into(), true.into());
    }
    if uses_levels(model) {
        config.insert(
            "thinkingLevel".into(),
            nearest(supported(model), effort).level().into(),
        );
    } else {
        let (min, max) = budget_range(model);
        let budget = budget.unwrap_or_else(|| effort.budget()).clamp(min, max);
        config.insert("thinkingBudget".into(), budget.into());
    }
    Value::Object(config)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn gemini_3_and_later_get_levels() {
        assert_eq!(
            thinking_config("gemini-3.5-flash", Medium, None, false),
            json!({"thinkingLevel": "MEDIUM"})
        );
        assert_eq!(
            thinking_config("gemini-3.5-flash-lite", Minimal, None, true),
            json!({"includeThoughts": true, "thinkingLevel": "MINIMAL"})
        );
        // Claude 格式给了具体预算：新模型按力度折成等级，不发预算
        assert_eq!(
            thinking_config(
                "gemini-4-argon",
                Effort::from_budget(31999),
                Some(31999),
                true
            ),
            json!({"includeThoughts": true, "thinkingLevel": "HIGH"})
        );
    }

    #[test]
    fn unsupported_levels_move_to_the_nearest() {
        // Gemini 4、3.7 / 3.8 Flash、3.1 Pro 没有 MINIMAL
        for model in [
            "gemini-4-argon",
            "gemini-3.8-flash",
            "gemini-3.7-flash",
            "gemini-3.1-pro-preview",
        ] {
            assert_eq!(
                thinking_config(model, Minimal, None, false)["thinkingLevel"],
                "LOW",
                "{model}"
            );
        }
        // 只有 MINIMAL / HIGH 的图像模型：MEDIUM 两边一样近，取更高的
        assert_eq!(
            thinking_config("gemini-3.1-flash-image-preview", Medium, None, false)["thinkingLevel"],
            "HIGH"
        );
        assert_eq!(
            thinking_config("gemini-3.1-flash-image-preview", Low, None, false)["thinkingLevel"],
            "MINIMAL"
        );
        // 3.1 Flash-Lite 不是图像模型，四档都有
        assert_eq!(
            thinking_config("gemini-3.1-flash-lite", Minimal, None, false)["thinkingLevel"],
            "MINIMAL"
        );
    }

    #[test]
    fn older_models_get_clamped_budgets() {
        assert_eq!(
            thinking_config("gemini-2.5-flash", Medium, None, false),
            json!({"thinkingBudget": 8192})
        );
        assert_eq!(
            thinking_config("gemini-2.5-pro", High, Some(100_000), true),
            json!({"includeThoughts": true, "thinkingBudget": 32768})
        );
        assert_eq!(
            thinking_config("gemini-2.5-flash-lite", Low, Some(10), false),
            json!({"thinkingBudget": 512})
        );
    }

    #[test]
    fn effort_from_client_values() {
        assert_eq!(Effort::from_budget(1024), Low);
        assert_eq!(Effort::from_budget(4096), Medium);
        assert_eq!(Effort::from_budget(16384), High);
        assert_eq!(Effort::from_reasoning_effort("none"), Some(Minimal));
        assert_eq!(Effort::from_reasoning_effort("high"), Some(High));
        assert_eq!(Effort::from_reasoning_effort("bogus"), None);
        assert_eq!(gemini_major("gemini-3.5-flash"), Some(3));
        assert_eq!(gemini_major("gemini-4-argon"), Some(4));
        assert_eq!(gemini_major("gemma-3"), None);
    }
}
