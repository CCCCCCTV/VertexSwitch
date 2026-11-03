//! 配置（SPEC §1）。
//!
//! 位置：`%APPDATA%\VertexSwitch\config.toml`（其他平台为系统配置目录）；
//! 环境变量 `VERTEXSWITCH_HOME` 可改目录。文件不存在时用默认值，不写盘。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const APP_NAME: &str = "VertexSwitch";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("读取配置文件 {path} 失败：{source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("配置文件 {path} 格式错误：{source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
}

/// 配置、日志所在目录。
pub fn app_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("VERTEXSWITCH_HOME") {
        return PathBuf::from(dir);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_NAME)
}

pub fn config_path() -> PathBuf {
    app_dir().join("config.toml")
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub server: ServerSettings,
    pub google: GoogleSettings,
    pub timeouts: TimeoutSettings,
    pub retry: RetrySettings,
    pub claude: ClaudeSettings,
    pub gemini: GeminiSettings,
    pub maas: MaasSettings,
    pub ollama: OllamaSettings,
    pub openai: OpenAiSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
    /// 非空时，除 /health 外的请求都要鉴权（SPEC §6）
    pub api_key: String,
    pub keep_alive_seconds: u64,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8788,
            api_key: String::new(),
            keep_alive_seconds: 75,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GoogleSettings {
    /// 服务账号 JSON 路径；为空时退回 GOOGLE_APPLICATION_CREDENTIALS
    pub credentials_file: String,
    /// 为空时从密钥文件读取
    pub project_id: String,
    /// global / us / eu / 具体区域（如 us-east5）
    pub region: String,
    pub token_refresh_minutes: u64,
    /// 仅供测试：把所有 Vertex 请求发到这个地址（替代按区域选择的主机）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_base_override: Option<String>,
}

impl Default for GoogleSettings {
    fn default() -> Self {
        Self {
            credentials_file: String::new(),
            project_id: String::new(),
            region: "global".into(),
            token_refresh_minutes: 50,
            api_base_override: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeoutSettings {
    pub connect: f64,
    /// 非流式：等上游完整响应
    pub read: f64,
    /// 流式：两段数据之间允许的最长空闲时间
    pub stream_idle: f64,
    pub write: f64,
    pub pool: f64,
}

impl Default for TimeoutSettings {
    fn default() -> Self {
        Self {
            connect: 10.0,
            read: 600.0,
            stream_idle: 900.0,
            write: 60.0,
            pool: 10.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RetrySettings {
    /// 只在收到任何响应字节之前出错时重试
    pub attempts: u32,
    pub backoff_seconds: f64,
    /// 上游挂住超过这个时长后无响应断开，视为配额耗尽，不再重试
    pub quota_hang_seconds: f64,
}

impl Default for RetrySettings {
    fn default() -> Self {
        Self {
            attempts: 2,
            backoff_seconds: 0.4,
            quota_hang_seconds: 20.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeSettings {
    pub anthropic_version: String,
    /// 转发前从请求体删除的顶层字段
    pub drop_body_fields: Vec<String>,
}

impl Default for ClaudeSettings {
    fn default() -> Self {
        Self {
            anthropic_version: "vertex-2023-10-16".into(),
            drop_body_fields: vec!["context_management".into()],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeminiSettings {
    /// 为空时用 google.region；Flex 只支持 global
    pub region: String,
    /// standard / flex / priority；请求头 X-VertexSwitch-Tier 可按请求覆盖
    pub tier: String,
    /// 加 X-Vertex-AI-LLM-Request-Type: shared，只走 PayGo
    pub shared_only: bool,
    /// Flex 排队超时（秒，X-Server-Timeout），上限 1800
    pub flex_server_timeout: u64,
    /// 官方注明不支持 Flex 的模型
    pub flex_unsupported_models: Vec<String>,
    /// OpenAI 兼容接口的 API 版本
    pub openai_api_version: String,
}

impl Default for GeminiSettings {
    fn default() -> Self {
        Self {
            region: String::new(),
            tier: "standard".into(),
            shared_only: false,
            flex_server_timeout: 1800,
            flex_unsupported_models: vec!["gemini-4-argon".into()],
            openai_api_version: "v1beta1".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaasSettings {
    pub region: String,
    pub api_version: String,
}

impl Default for MaasSettings {
    fn default() -> Self {
        Self {
            region: String::new(),
            api_version: "v1".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OllamaSettings {
    pub enabled: bool,
    pub base_url: String,
}

impl Default for OllamaSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: "http://127.0.0.1:11434".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAiSettings {
    pub default_max_tokens: u64,
}

impl Default for OpenAiSettings {
    fn default() -> Self {
        Self {
            default_max_tokens: 8192,
        }
    }
}

impl Settings {
    /// 读取配置；文件不存在时返回默认值。
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|source| ConfigError::Parse {
                path: path.into(),
                source,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Io {
                path: path.into(),
                source,
            }),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)
    }

    pub fn gemini_region(&self) -> &str {
        non_empty_or(&self.gemini.region, &self.google.region)
    }

    pub fn maas_region(&self) -> &str {
        non_empty_or(&self.maas.region, &self.google.region)
    }

    pub fn credentials_file(&self) -> Option<PathBuf> {
        if !self.google.credentials_file.is_empty() {
            return Some(self.google.credentials_file.clone().into());
        }
        std::env::var_os("GOOGLE_APPLICATION_CREDENTIALS")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    }
}

fn non_empty_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() { fallback } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let s = Settings::load(&dir.path().join("nope.toml")).unwrap();
        assert_eq!(s.server.port, 8788);
        assert_eq!(s.google.region, "global");
        assert_eq!(s.gemini.flex_unsupported_models, ["gemini-4-argon"]);
    }

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut s = Settings::default();
        s.server.port = 9999;
        s.google.region = "us".into();
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
    }

    #[test]
    fn partial_file_fills_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[gemini]\ntier = \"flex\"\n").unwrap();
        let s = Settings::load(&path).unwrap();
        assert_eq!(s.gemini.tier, "flex");
        assert_eq!(s.gemini.flex_server_timeout, 1800);
        assert_eq!(s.server.port, 8788);
    }

    #[test]
    fn python_prototype_config_is_compatible() {
        // 用户机器上已有 Python 原型写的配置，键名必须兼容
        let text = r#"
[server]
host = "127.0.0.1"
port = 8788
api_key = ""

[google]
credentials_file = 'C:\key.json'
project_id = ""
region = "global"

[gemini]
region = ""
tier = "standard"
shared_only = false
flex_server_timeout = 1800
"#;
        let s: Settings = toml::from_str(text).unwrap();
        assert_eq!(s.google.credentials_file, r"C:\key.json");
    }

    #[test]
    fn region_fallbacks() {
        let mut s = Settings::default();
        s.google.region = "us-east5".into();
        assert_eq!(s.gemini_region(), "us-east5");
        s.gemini.region = "global".into();
        assert_eq!(s.gemini_region(), "global");
        assert_eq!(s.maas_region(), "us-east5");
    }

    #[test]
    fn bad_toml_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[server\n").unwrap();
        assert!(matches!(
            Settings::load(&path),
            Err(ConfigError::Parse { .. })
        ));
    }
}
