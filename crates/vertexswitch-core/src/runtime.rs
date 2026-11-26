//! 一次请求用到的全部运行时依赖：配置、token 来源、HTTP 传输、模型表。
//!
//! `AppState` 持有一个可整体替换的 `Arc<Runtime>`。每个请求开始时取一份快照并用到底，
//! 改配置 / 重载模型表时换上新快照即可立即生效，不影响正在进行中的请求。

use std::path::PathBuf;
use std::sync::Arc;

use crate::auth::{ServiceAccountAuth, TokenSource};
use crate::backends::vertex;
use crate::config::{BackupCredential, Settings};
use crate::credentials::CredentialSwitch;
use crate::errors::{ProxyError, ProxyResult};
use crate::regions::RegionSwitch;
use crate::registry::ModelRegistry;
use crate::tr;

/// 凭证在日志和界面里的名字：“凭证 2（项目）”，不知道项目时只写“凭证 2”。`index` 从 0 数。
pub fn credential_name(index: usize, project: &str) -> String {
    let n = index + 1;
    if project.is_empty() {
        tr!("凭证 {n}", "credential {n}")
    } else {
        tr!("凭证 {n}（{project}）", "credential {n} ({project})")
    }
}
use crate::transport::Transport;

pub struct Runtime {
    pub settings: Settings,
    /// 第一个凭证（`google.credentials_file`）
    pub tokens: Arc<TokenSource>,
    /// 备用凭证（`google.backup_credentials`，按顺序），凭证号 1、2……
    pub backups: Arc<Vec<Arc<TokenSource>>>,
    pub transport: Transport,
    /// 区域切换的状态，跨配置快照共享
    pub regions: Arc<RegionSwitch>,
    /// 换凭证的状态，跨配置快照共享
    pub credentials: Arc<CredentialSwitch>,
    registry: Arc<ModelRegistry>,
}

impl Runtime {
    pub fn new(settings: Settings, registry: ModelRegistry, tokens: TokenSource) -> Self {
        let transport = Transport::new(settings.timeouts.clone(), settings.retry.clone());
        let backups = Arc::new(Self::backup_tokens(&settings.google.backup_credentials));
        Self {
            settings,
            tokens: Arc::new(tokens),
            backups,
            transport,
            regions: Arc::default(),
            credentials: Arc::default(),
            registry: Arc::new(registry),
        }
    }

    /// 按配置用服务账号认证。
    pub fn service_account_tokens(settings: &Settings) -> TokenSource {
        TokenSource::ServiceAccount(Box::new(ServiceAccountAuth::new(
            settings.credentials_file(),
            &settings.google.project_id,
            reqwest::Client::new(),
        )))
    }

    fn backup_tokens(backups: &[BackupCredential]) -> Vec<Arc<TokenSource>> {
        backups
            .iter()
            .map(|b| {
                Arc::new(TokenSource::ServiceAccount(Box::new(
                    ServiceAccountAuth::new(
                        Some(PathBuf::from(b.credentials_file.trim())),
                        &b.project_id,
                        reqwest::Client::new(),
                    ),
                )))
            })
            .collect()
    }

    /// 测试用：换上固定的备用凭证（之后改配置时沿用）。
    pub fn with_backups(&self, backups: Vec<TokenSource>) -> Self {
        Self {
            settings: self.settings.clone(),
            tokens: self.tokens.clone(),
            backups: Arc::new(backups.into_iter().map(Arc::new).collect()),
            transport: self.transport.clone(),
            regions: self.regions.clone(),
            credentials: self.credentials.clone(),
            registry: self.registry.clone(),
        }
    }

    /// 换上新配置：认证信息没变就沿用原 token 来源（保留缓存的 token），超时与重试没变就沿用连接池。
    /// 测试用的固定 token 始终沿用。
    pub fn with_settings(&self, settings: Settings) -> Self {
        let old = &self.settings.google;
        let new = &settings.google;
        let same_auth =
            new.credentials_file == old.credentials_file && new.project_id == old.project_id;
        let tokens = if same_auth || matches!(*self.tokens, TokenSource::Static { .. }) {
            self.tokens.clone()
        } else {
            Arc::new(Self::service_account_tokens(&settings))
        };
        let static_backups = !self.backups.is_empty()
            && self
                .backups
                .iter()
                .all(|b| matches!(**b, TokenSource::Static { .. }));
        let backups = if new.backup_credentials == old.backup_credentials || static_backups {
            self.backups.clone()
        } else {
            Arc::new(Self::backup_tokens(&new.backup_credentials))
        };
        // 凭证换了：各模型回到第一个凭证重新开始
        if !same_auth
            || new.backup_credentials != old.backup_credentials
            || new.credential_reset_minutes != old.credential_reset_minutes
        {
            self.credentials.reset();
        }
        let transport = if settings.timeouts == self.settings.timeouts
            && settings.retry == self.settings.retry
        {
            self.transport.clone()
        } else {
            Transport::new(settings.timeouts.clone(), settings.retry.clone())
        };
        // 区域切换的配置或平时区域变了：各模型回到平时区域重新开始
        if settings.gemini.region_switch != self.settings.gemini.region_switch
            || settings.gemini_region() != self.settings.gemini_region()
        {
            self.regions.reset();
        }
        Self {
            settings,
            tokens,
            backups,
            transport,
            regions: self.regions.clone(),
            credentials: self.credentials.clone(),
            registry: self.registry.clone(),
        }
    }

    pub fn with_registry(&self, registry: ModelRegistry) -> Self {
        Self {
            settings: self.settings.clone(),
            tokens: self.tokens.clone(),
            backups: self.backups.clone(),
            transport: self.transport.clone(),
            regions: self.regions.clone(),
            credentials: self.credentials.clone(),
            registry: Arc::new(registry),
        }
    }

    pub fn registry(&self) -> &ModelRegistry {
        &self.registry
    }

    /// 凭证总数（第一个加上备用的）。
    pub fn credential_count(&self) -> usize {
        1 + self.backups.len()
    }

    /// 第 `index` 个凭证（从 0 数，0 是第一个）。
    pub fn credential(&self, index: usize) -> &Arc<TokenSource> {
        match index {
            0 => &self.tokens,
            i => &self.backups[i - 1],
        }
    }

    /// 第 `index` 个凭证的项目 ID（须在这个凭证取过 token 之后调用）。
    pub fn credential_project(&self, index: usize) -> ProxyResult<String> {
        if index == 0 {
            return self.project_id();
        }
        let project = self.credential(index).project_id();
        if project.is_empty() {
            return Err(ProxyError::new(
                500,
                tr!(
                    "备用凭证 {index} 没有项目 ID：密钥文件里没有，也没有单独填写",
                    "Backup credential {index} has no project ID: the key file has none and none was entered"
                ),
            ));
        }
        Ok(project)
    }

    /// 日志里的凭证名：“凭证 2（项目）”。
    pub fn credential_label(&self, index: usize) -> String {
        let project = if index == 0 {
            self.project_id().unwrap_or_default()
        } else {
            self.credential(index).project_id()
        };
        credential_name(index, &project)
    }

    /// 凭证（token 来源）和另一个快照是同一批：没变就不用重启后台刷新。
    pub fn same_credentials(&self, other: &Runtime) -> bool {
        Arc::ptr_eq(&self.tokens, &other.tokens) && Arc::ptr_eq(&self.backups, &other.backups)
    }

    pub async fn google_token(&self) -> ProxyResult<String> {
        self.tokens
            .get_token()
            .await
            .map_err(|e| ProxyError::new(500, e.0))
    }

    /// 项目 ID：配置优先，否则来自密钥文件（须在取过 token 之后调用）。
    pub fn project_id(&self) -> ProxyResult<String> {
        let project = if self.settings.google.project_id.is_empty() {
            self.tokens.project_id()
        } else {
            self.settings.google.project_id.clone()
        };
        if project.is_empty() {
            return Err(ProxyError::new(
                500,
                tr!(
                    "未配置 Google Cloud 项目 ID（google.project_id）",
                    "No Google Cloud project ID is configured (google.project_id)"
                ),
            ));
        }
        Ok(project)
    }

    pub fn vertex_base(&self, region: &str) -> String {
        vertex::base_url(region, self.settings.google.api_base_override.as_deref())
    }

    /// 不含 API 版本的 Vertex 主机地址。
    pub fn vertex_host(&self, region: &str) -> String {
        vertex::host(region, self.settings.google.api_base_override.as_deref())
    }
}
