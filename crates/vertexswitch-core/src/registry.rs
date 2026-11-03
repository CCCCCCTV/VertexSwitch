//! 模型名 → (后端, 上游模型 ID)（SPEC §7）。
//!
//! 模型表来自 models.toml：包内默认，用户目录下的同名文件存在时整体优先。
//! 表保持声明顺序，用于模型列表输出。

use std::collections::BTreeSet;
use std::path::Path;

use crate::config::app_dir;

pub const DEFAULT_MODELS: &str = include_str!("../data/models.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backend {
    Claude,
    Gemini,
    Maas,
    Ollama,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        self.table_key()
    }

    fn table_key(self) -> &'static str {
        match self {
            Backend::Claude => "claude",
            Backend::Gemini => "gemini",
            Backend::Maas => "maas",
            Backend::Ollama => "ollama",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub backend: Backend,
    pub upstream_model: String,
}

impl Route {
    fn new(backend: Backend, model: impl Into<String>) -> Self {
        Self {
            backend,
            upstream_model: model.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("读取模型表失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("模型表格式错误：{0}")]
    Parse(#[from] toml::de::Error),
    #[error("模型表中 {0:?} 的值必须是字符串")]
    NotString(String),
}

type Table = Vec<(String, String)>;

#[derive(Debug, Clone, Default)]
pub struct ModelRegistry {
    claude: Table,
    gemini: Table,
    maas: Table,
    /// 运行时从 Ollama /api/tags 发现的模型名
    pub ollama_models: BTreeSet<String>,
}

impl ModelRegistry {
    pub fn from_toml(text: &str) -> Result<Self, RegistryError> {
        let doc: toml::Table = toml::from_str(text)?;
        let table = |backend: Backend| -> Result<Table, RegistryError> {
            let Some(section) = doc.get(backend.table_key()).and_then(|v| v.as_table()) else {
                return Ok(Vec::new());
            };
            section
                .iter()
                .map(|(k, v)| {
                    v.as_str()
                        .map(|v| (k.clone(), v.to_owned()))
                        .ok_or_else(|| RegistryError::NotString(k.clone()))
                })
                .collect()
        };
        Ok(Self {
            claude: table(Backend::Claude)?,
            gemini: table(Backend::Gemini)?,
            maas: table(Backend::Maas)?,
            ollama_models: BTreeSet::new(),
        })
    }

    pub fn builtin() -> Self {
        Self::from_toml(DEFAULT_MODELS).expect("内置模型表必须合法")
    }

    /// 用户目录下有 models.toml 就用它，否则用内置表。
    pub fn load() -> Result<Self, RegistryError> {
        let user = app_dir().join("models.toml");
        if user.exists() {
            Self::load_file(&user)
        } else {
            Ok(Self::builtin())
        }
    }

    pub fn load_file(path: &Path) -> Result<Self, RegistryError> {
        Self::from_toml(&std::fs::read_to_string(path)?)
    }

    fn table(&self, backend: Backend) -> &Table {
        match backend {
            Backend::Claude => &self.claude,
            Backend::Gemini => &self.gemini,
            Backend::Maas | Backend::Ollama => &self.maas,
        }
    }

    fn lookup(&self, backend: Backend, alias: &str) -> Option<Route> {
        self.table(backend)
            .iter()
            .find(|(k, _)| k == alias)
            .map(|(_, v)| Route::new(backend, v.clone()))
    }

    /// 某个后端的 (别名, 上游 ID) 表（按声明顺序；Ollama 没有表）。
    pub fn entries(&self, backend: Backend) -> &[(String, String)] {
        match backend {
            Backend::Ollama => &[],
            other => self.table(other),
        }
    }

    /// 所有后端的别名总数。
    pub fn len(&self) -> usize {
        self.claude.len() + self.gemini.len() + self.maas.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 别名列表（按声明顺序）。
    pub fn aliases(&self, backend: Backend) -> Vec<String> {
        if backend == Backend::Ollama {
            return self.ollama_models.iter().cloned().collect();
        }
        self.table(backend).iter().map(|(k, _)| k.clone()).collect()
    }

    /// 解析顺序：显式前缀 → Gemini → Claude → MaaS → 已发现的 Ollama 模型。
    pub fn resolve(&self, model: &str) -> Option<Route> {
        let model = model.trim();
        let model = model.strip_prefix("models/").unwrap_or(model);
        if let Some(name) = model.strip_prefix("ollama/") {
            return Some(Route::new(Backend::Ollama, name));
        }
        let model = model.strip_prefix("google/").unwrap_or(model);
        let model = model.strip_prefix("anthropic/").unwrap_or(model);

        if let Some(route) = self.lookup(Backend::Gemini, model) {
            return Some(route);
        }
        if model.starts_with("gemini-") {
            return Some(Route::new(Backend::Gemini, model));
        }
        if let Some(route) = self.lookup(Backend::Claude, model) {
            return Some(route);
        }
        if model.starts_with("claude-") {
            return Some(Route::new(Backend::Claude, undate_claude(model)));
        }
        if let Some(route) = self.lookup(Backend::Maas, model) {
            return Some(route);
        }
        if model.contains('/') {
            return Some(Route::new(Backend::Maas, model));
        }
        if self.ollama_models.contains(model) {
            return Some(Route::new(Backend::Ollama, model));
        }
        None
    }
}

/// `claude-haiku-4-5-20251001` → `claude-haiku-4-5@20251001`
fn undate_claude(model: &str) -> String {
    if let Some((base, date)) = model.rsplit_once('-')
        && date.len() == 8
        && date.bytes().all(|b| b.is_ascii_digit())
    {
        return format!("{base}@{date}");
    }
    model.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(backend: Backend, model: &str) -> Option<Route> {
        Some(Route::new(backend, model))
    }

    #[test]
    fn claude_aliases_and_dates() {
        let reg = ModelRegistry::builtin();
        assert_eq!(
            reg.resolve("claude-opus-4-6"),
            route(Backend::Claude, "claude-opus-4-6")
        );
        assert_eq!(
            reg.resolve("claude-haiku-4-5"),
            route(Backend::Claude, "claude-haiku-4-5@20251001")
        );
        assert_eq!(
            reg.resolve("claude-sonnet-4-5-20250929"),
            route(Backend::Claude, "claude-sonnet-4-5@20250929")
        );
        assert_eq!(
            reg.resolve("claude-future-9"),
            route(Backend::Claude, "claude-future-9")
        );
    }

    #[test]
    fn gemini_aliases_and_prefixes() {
        let reg = ModelRegistry::builtin();
        assert_eq!(
            reg.resolve("gemini-3-flash"),
            route(Backend::Gemini, "gemini-3-flash-preview")
        );
        assert_eq!(
            reg.resolve("models/gemini-3.1-pro"),
            route(Backend::Gemini, "gemini-3.1-pro-preview")
        );
        assert_eq!(
            reg.resolve("google/gemini-3.5-flash"),
            route(Backend::Gemini, "gemini-3.5-flash")
        );
        assert_eq!(
            reg.resolve("gemini-9-future"),
            route(Backend::Gemini, "gemini-9-future")
        );
    }

    #[test]
    fn maas_ollama_and_unknown() {
        let mut reg = ModelRegistry::builtin();
        assert_eq!(
            reg.resolve("deepseek-ai/deepseek-v3.1-maas"),
            route(Backend::Maas, "deepseek-ai/deepseek-v3.1-maas")
        );
        assert_eq!(
            reg.resolve("ollama/qwen3:8b"),
            route(Backend::Ollama, "qwen3:8b")
        );
        assert_eq!(reg.resolve("llama3.2:3b"), None);
        reg.ollama_models.insert("llama3.2:3b".into());
        assert_eq!(
            reg.resolve("llama3.2:3b"),
            route(Backend::Ollama, "llama3.2:3b")
        );
        assert_eq!(reg.resolve("gpt-4o"), None);
    }

    #[test]
    fn builtin_has_no_retired_gemini() {
        // 官方已宣布下线的模型，默认表不应再列出（避免出现在模型列表里误导客户端）
        let retired = [
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.5-flash-lite",
            "gemini-2.0-flash",
            "gemini-2.0-flash-001",
            "gemini-3-pro-preview",
        ];
        let reg = ModelRegistry::builtin();
        for (k, v) in &reg.gemini {
            assert!(!retired.contains(&k.as_str()) && !retired.contains(&v.as_str()));
        }
    }

    #[test]
    fn aliases_keep_declaration_order() {
        let reg = ModelRegistry::builtin();
        assert_eq!(reg.aliases(Backend::Gemini)[0], "gemini-4-argon");
        assert_eq!(reg.aliases(Backend::Claude)[0], "claude-fable-5-1");
        assert!(reg.aliases(Backend::Maas).is_empty());
    }

    #[test]
    fn rejects_non_string_values() {
        assert!(matches!(
            ModelRegistry::from_toml("[gemini]\nx = 1\n"),
            Err(RegistryError::NotString(_))
        ));
    }
}
