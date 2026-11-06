//! 配置（SPEC §1）。
//!
//! 位置：`%APPDATA%\VertexSwitch\config.toml`（其他平台为系统配置目录）；
//! 环境变量 `VERTEXSWITCH_HOME` 可改目录。文件不存在时用默认值，不写盘。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::tr;
use serde::{Deserialize, Serialize};

pub const APP_NAME: &str = "VertexSwitch";

#[derive(Debug)]
pub enum ConfigError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    Invalid {
        path: PathBuf,
        message: String,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Io { path, source } => tr!(
                "读取配置文件 {} 失败：{source}",
                "Couldn't read the config file {}: {source}",
                path.display()
            ),
            Self::Parse { path, source } => tr!(
                "配置文件 {} 格式错误：{source}",
                "The config file {} is malformed: {source}",
                path.display()
            ),
            Self::Invalid { path, message } => tr!(
                "配置文件 {} 有误：{message}",
                "The config file {} is invalid: {message}",
                path.display()
            ),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::Invalid { .. } => None,
        }
    }
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
            port: 8787,
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
    /// 上游繁忙（429 / 503 / 529；500 只重试一次）时自动重试（SPEC §4）
    pub busy: bool,
    /// 繁忙重试最多几次（上限见 transport::MAX_BUSY_RETRIES）
    pub busy_attempts: u32,
    /// 繁忙重试前等待的秒数：关于 n（第几次重试，从 1 开始）的算式，见 wait_formula
    pub busy_wait: String,
}

impl Default for RetrySettings {
    fn default() -> Self {
        Self {
            attempts: 2,
            backoff_seconds: 0.4,
            quota_hang_seconds: 20.0,
            busy: true,
            busy_attempts: 3,
            busy_wait: crate::wait_formula::DEFAULT.into(),
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
    /// 默认档位 standard / flex / priority
    pub tier: String,
    /// 按模型单独设的档位（键为上游模型 ID），优先于 tier；请求头 X-VertexSwitch-Tier 再优先
    pub model_tiers: BTreeMap<String, String>,
    /// 加 X-Vertex-AI-LLM-Request-Type: shared，只走 PayGo
    pub shared_only: bool,
    /// Flex 排队超时（秒，X-Server-Timeout），上限 1800
    pub flex_server_timeout: u64,
    /// 官方注明不支持 Flex 的模型
    pub flex_unsupported_models: Vec<String>,
    /// OpenAI 兼容接口的 API 版本
    pub openai_api_version: String,
    /// OpenAI 格式的 Gemini 请求怎么发：auto（需要档位头时转成原生格式，否则走兼容接口）/
    /// always（总是转成原生格式）/ never（总是走兼容接口，档位不生效）
    pub openai_native: String,
}

pub const OPENAI_NATIVE_MODES: &[&str] = &["auto", "always", "never"];

impl Default for GeminiSettings {
    fn default() -> Self {
        Self {
            region: String::new(),
            tier: "standard".into(),
            model_tiers: BTreeMap::new(),
            shared_only: false,
            flex_server_timeout: 1800,
            flex_unsupported_models: vec!["gemini-4-argon".into()],
            openai_api_version: "v1beta1".into(),
            openai_native: "auto".into(),
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
            Ok(text) => {
                let settings: Self =
                    toml::from_str(&text).map_err(|source| ConfigError::Parse {
                        path: path.into(),
                        source,
                    })?;
                settings
                    .validate()
                    .map_err(|message| ConfigError::Invalid {
                        path: path.into(),
                        message,
                    })?;
                Ok(settings)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Io {
                path: path.into(),
                source,
            }),
        }
    }

    /// serde 管不到的取值范围：档位名、OpenAI 转发方式、繁忙重试的等待算式。
    pub fn validate(&self) -> Result<(), String> {
        let g = &self.gemini;
        let tier_ok = |t: &str| crate::tiers::Tier::parse(t).is_some();
        if !tier_ok(&g.tier) {
            return Err(tr!(
                "gemini.tier 只能是 standard / flex / priority，不是 {:?}",
                "gemini.tier must be standard / flex / priority, not {:?}",
                g.tier
            ));
        }
        if let Some((model, tier)) = g.model_tiers.iter().find(|(_, t)| !tier_ok(t)) {
            return Err(tr!(
                "gemini.model_tiers.{model} 只能是 standard / flex / priority，不是 {tier:?}",
                "gemini.model_tiers.{model} must be standard / flex / priority, not {tier:?}"
            ));
        }
        if !OPENAI_NATIVE_MODES.contains(&g.openai_native.as_str()) {
            return Err(tr!(
                "gemini.openai_native 只能是 auto / always / never，不是 {:?}",
                "gemini.openai_native must be auto / always / never, not {:?}",
                g.openai_native
            ));
        }
        if let Err(e) = crate::wait_formula::WaitFormula::parse(&self.retry.busy_wait) {
            return Err(tr!(
                "retry.busy_wait（繁忙重试的等待时间）有误：{e}",
                "retry.busy_wait (wait before busy retries) is invalid: {e}"
            ));
        }
        Ok(())
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
        assert_eq!(s.server.port, 8787);
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
        assert_eq!(s.server.port, 8787);
    }

    #[test]
    fn removed_ollama_section_is_ignored() {
        // 0.1.x 写过 [ollama]；删掉 Ollama 支持后旧配置照常能读，保存时这一段自然消失
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let old = r#"
[ollama]
enabled = true
base_url = "http://127.0.0.1:11434"

[gemini]
tier = "flex"
"#;
        std::fs::write(&path, old).unwrap();
        let s = Settings::load(&path).unwrap();
        assert_eq!(s.gemini.tier, "flex");
        s.save(&path).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("ollama"));
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
