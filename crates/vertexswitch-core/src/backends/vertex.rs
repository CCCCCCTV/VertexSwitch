//! Vertex AI 端点的公共部分（SPEC §3）。

/// 按区域选择 Vertex AI 主机（global / us、eu 多区域 / 单区域），返回含 `/v1` 的基础地址。
pub fn vertex_base_url(region: &str) -> String {
    match region {
        "global" => "https://aiplatform.googleapis.com/v1".into(),
        // 多区域端点：主机名和路径里都带 us / eu
        "us" | "eu" => format!("https://aiplatform.{region}.rep.googleapis.com/v1"),
        _ => format!("https://{region}-aiplatform.googleapis.com/v1"),
    }
}

/// `api_base_override`（仅测试用）存在时替换主机部分。
pub fn base_url(region: &str, api_base_override: Option<&str>) -> String {
    match api_base_override {
        Some(base) => format!("{}/v1", base.trim_end_matches('/')),
        None => vertex_base_url(region),
    }
}

pub fn publisher_models_url(base: &str, project_id: &str, region: &str, publisher: &str) -> String {
    format!("{base}/projects/{project_id}/locations/{region}/publishers/{publisher}/models")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert_eq!(
            vertex_base_url("global"),
            "https://aiplatform.googleapis.com/v1"
        );
        assert_eq!(
            vertex_base_url("us"),
            "https://aiplatform.us.rep.googleapis.com/v1"
        );
        assert_eq!(
            vertex_base_url("eu"),
            "https://aiplatform.eu.rep.googleapis.com/v1"
        );
        assert_eq!(
            vertex_base_url("us-east5"),
            "https://us-east5-aiplatform.googleapis.com/v1"
        );
    }

    #[test]
    fn override_replaces_host_only() {
        assert_eq!(
            base_url("us", Some("http://127.0.0.1:9/")),
            "http://127.0.0.1:9/v1"
        );
        assert_eq!(
            publisher_models_url(&base_url("global", None), "p", "global", "google"),
            "https://aiplatform.googleapis.com/v1/projects/p/locations/global/publishers/google/models"
        );
    }
}
