//! 管理 token 的保存位置：优先系统凭据库（Windows 凭据管理器 / macOS 钥匙串 / Secret Service），
//! 不可用时退回配置目录下的文件。
//!
//! 早期版本把 token 存在 `admin_token` 文件里；凭据库可用时会把它迁入凭据库并删除该文件。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::app::{load_or_create_admin_token, random_token};
use crate::tr;

pub const SERVICE: &str = "VertexSwitch";
pub const ADMIN_TOKEN_USER: &str = "admin-token";

/// 存一个秘密值的地方。
pub trait SecretStore {
    /// 不存在时返回 `Ok(None)`。
    fn get(&self) -> Result<Option<String>, String>;
    fn set(&self, value: &str) -> Result<(), String>;
}

/// 系统凭据库里的一条记录。
pub struct KeyringStore {
    entry: keyring::Entry,
}

impl KeyringStore {
    pub fn admin_token() -> Result<Self, String> {
        keyring::Entry::new(SERVICE, ADMIN_TOKEN_USER)
            .map(|entry| Self { entry })
            .map_err(|e| e.to_string())
    }
}

impl SecretStore for KeyringStore {
    fn get(&self) -> Result<Option<String>, String> {
        match self.entry.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, value: &str) -> Result<(), String> {
        self.entry.set_password(value).map_err(|e| e.to_string())
    }
}

/// 内存里的存储（测试用）；`fail` 为真时模拟凭据库不可用。
#[derive(Default)]
pub struct MemoryStore {
    pub value: Mutex<Option<String>>,
    pub fail: bool,
}

impl SecretStore for MemoryStore {
    fn get(&self) -> Result<Option<String>, String> {
        if self.fail {
            return Err(tr!("凭据库不可用", "credential store unavailable"));
        }
        Ok(self.value.lock().expect("锁未中毒").clone())
    }

    fn set(&self, value: &str) -> Result<(), String> {
        if self.fail {
            return Err(tr!("凭据库不可用", "credential store unavailable"));
        }
        *self.value.lock().expect("锁未中毒") = Some(value.to_owned());
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenLocation {
    CredentialStore,
    File(PathBuf),
}

impl TokenLocation {
    pub fn describe(&self) -> String {
        match self {
            Self::CredentialStore => tr!(
                "系统凭据库（{SERVICE} / {ADMIN_TOKEN_USER}）",
                "the system credential store ({SERVICE} / {ADMIN_TOKEN_USER})"
            ),
            Self::File(path) => path.display().to_string(),
        }
    }
}

/// 读取管理 token，没有就生成。`legacy_file` 是旧版本的 token 文件，也是凭据库不可用时的退路。
pub fn admin_token(store: &dyn SecretStore, legacy_file: &Path) -> (String, TokenLocation) {
    let fallback = |reason: String| {
        tracing::warn!(
            "{}",
            tr!(
                "无法使用系统凭据库保存管理 token（{reason}），改用文件",
                "Can't keep the admin token in the system credential store ({reason}); using a file instead"
            )
        );
        match load_or_create_admin_token(legacy_file) {
            Ok(token) => (token, TokenLocation::File(legacy_file.to_owned())),
            Err(e) => {
                tracing::warn!(
                    "{}",
                    tr!(
                        "也无法读写 {}：{e}；本次使用临时 token",
                        "Can't read or write {} either: {e}; using a temporary token for this run",
                        legacy_file.display()
                    )
                );
                (random_token(), TokenLocation::File(legacy_file.to_owned()))
            }
        }
    };
    match store.get() {
        Ok(Some(token)) if !token.trim().is_empty() => {
            remove_legacy(legacy_file);
            (token.trim().to_owned(), TokenLocation::CredentialStore)
        }
        Ok(_) => {
            // 有旧文件就迁移它，保证已经在用的 token 不变
            let token = std::fs::read_to_string(legacy_file)
                .ok()
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(random_token);
            match store.set(&token) {
                Ok(()) => {
                    remove_legacy(legacy_file);
                    (token, TokenLocation::CredentialStore)
                }
                Err(e) => fallback(e),
            }
        }
        Err(e) => fallback(e),
    }
}

/// 程序里实际使用的入口：系统凭据库 + 配置目录下的旧文件 / 退路文件。
pub fn load_admin_token() -> (String, TokenLocation) {
    let legacy = crate::config::app_dir().join("admin_token");
    match KeyringStore::admin_token() {
        Ok(store) => admin_token(&store, &legacy),
        Err(e) => {
            let failing = MemoryStore {
                fail: true,
                ..Default::default()
            };
            tracing::warn!(
                "{}",
                tr!(
                    "系统凭据库不可用：{e}",
                    "System credential store unavailable: {e}"
                )
            );
            admin_token(&failing, &legacy)
        }
    }
}

fn remove_legacy(path: &Path) {
    if path.exists() {
        match std::fs::remove_file(path) {
            Ok(()) => tracing::info!(
                "{}",
                tr!(
                    "管理 token 已迁入系统凭据库，删除旧文件 {}",
                    "Moved the admin token into the system credential store and deleted the old file {}",
                    path.display()
                )
            ),
            Err(e) => tracing::warn!(
                "{}",
                tr!(
                    "删除旧的管理 token 文件失败：{e}",
                    "Couldn't delete the old admin token file: {e}"
                )
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_reuses_in_store() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("admin_token");
        let store = MemoryStore::default();
        let (first, loc) = admin_token(&store, &file);
        assert_eq!(loc, TokenLocation::CredentialStore);
        assert_eq!(first.len(), 48);
        assert!(!file.exists());
        assert_eq!(admin_token(&store, &file).0, first);
    }

    #[test]
    fn migrates_legacy_file_into_store() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("admin_token");
        std::fs::write(&file, "old-token\n").unwrap();
        let store = MemoryStore::default();
        let (token, loc) = admin_token(&store, &file);
        assert_eq!(token, "old-token");
        assert_eq!(loc, TokenLocation::CredentialStore);
        assert_eq!(store.value.lock().unwrap().as_deref(), Some("old-token"));
        assert!(!file.exists(), "迁移后应删除旧文件");
    }

    #[test]
    fn falls_back_to_file_when_store_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("admin_token");
        let store = MemoryStore {
            fail: true,
            ..Default::default()
        };
        let (token, loc) = admin_token(&store, &file);
        assert_eq!(loc, TokenLocation::File(file.clone()));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), token);
        assert_eq!(admin_token(&store, &file).0, token);
    }
}
