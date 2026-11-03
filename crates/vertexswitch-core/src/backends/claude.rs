//! Claude on Vertex AI 后端（SPEC §3）。
//!
//! 依据 <https://platform.claude.com/docs/en/build-with-claude/claude-models-on-google-cloud/google-managed>
//! 与 Anthropic 官方 SDK 的 Vertex 客户端：请求体与 Messages API 基本相同，区别是 `model`
//! 写在 URL 里，`anthropic_version` 放在请求体里；count_tokens 走固定的 `count-tokens` 模型路径，
//! `model` 留在请求体里。

use serde_json::Value;

use crate::config::ClaudeSettings;

pub fn messages_url(publisher_models: &str, model: &str, stream: bool) -> String {
    let specifier = if stream {
        "streamRawPredict"
    } else {
        "rawPredict"
    };
    format!("{publisher_models}/{model}:{specifier}")
}

pub fn count_tokens_url(publisher_models: &str) -> String {
    format!("{publisher_models}/count-tokens:rawPredict")
}

/// 去掉 `model` 与配置里要删的字段，缺省时补 `anthropic_version`。
pub fn messages_body(body: &mut Value, settings: &ClaudeSettings) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    obj.remove("model");
    for name in &settings.drop_body_fields {
        obj.remove(name);
    }
    obj.entry("anthropic_version")
        .or_insert_with(|| settings.anthropic_version.clone().into());
}

/// count-tokens 端点的模型写在请求体里（上游 ID）。
pub fn count_tokens_body(body: &mut Value, settings: &ClaudeSettings, upstream_model: &str) {
    messages_body(body, settings);
    if let Some(obj) = body.as_object_mut() {
        obj.insert("model".into(), upstream_model.into());
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn urls() {
        let base = "https://h/v1/projects/p/locations/us-east5/publishers/anthropic/models";
        assert_eq!(
            messages_url(base, "claude-x", false),
            format!("{base}/claude-x:rawPredict")
        );
        assert_eq!(
            messages_url(base, "claude-x", true),
            format!("{base}/claude-x:streamRawPredict")
        );
        assert_eq!(
            count_tokens_url(base),
            format!("{base}/count-tokens:rawPredict")
        );
    }

    #[test]
    fn body_rewrite() {
        let mut body =
            json!({"model": "m", "max_tokens": 5, "messages": [], "context_management": {"x": 1}});
        messages_body(&mut body, &ClaudeSettings::default());
        assert_eq!(
            body,
            json!({"max_tokens": 5, "messages": [], "anthropic_version": "vertex-2023-10-16"})
        );
    }

    #[test]
    fn keeps_client_anthropic_version() {
        let mut body = json!({"anthropic_version": "2026-10-21"});
        messages_body(&mut body, &ClaudeSettings::default());
        assert_eq!(body["anthropic_version"], "2026-10-21");
    }

    #[test]
    fn count_tokens_keeps_model() {
        let mut body = json!({"model": "alias", "messages": []});
        count_tokens_body(&mut body, &ClaudeSettings::default(), "claude-x@1");
        assert_eq!(body["model"], "claude-x@1");
        assert_eq!(body["anthropic_version"], "vertex-2023-10-16");
    }
}
