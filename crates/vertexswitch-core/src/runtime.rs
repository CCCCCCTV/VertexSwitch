//! 一次请求用到的全部运行时依赖：配置、token 来源、HTTP 传输、模型表。
//!
//! `AppState` 持有一个可整体替换的 `Arc<Runtime>`。每个请求开始时取一份快照并用到底，
//! 改配置 / 重载模型表时换上新快照即可立即生效，不影响正在进行中的请求。

use std::sync::Arc;

use crate::auth::{ServiceAccountAuth, TokenSource};
use crate::backends::vertex;
use crate::config::Settings;
use crate::errors::{ProxyError, ProxyResult};
use crate::registry::ModelRegistry;
use crate::tr;
use crate::transport::Transport;

pub struct Runtime {
    pub settings: Settings,
    pub tokens: Arc<TokenSource>,
    pub transport: Transport,
    registry: Arc<ModelRegistry>,
}

impl Runtime {
    pub fn new(settings: Settings, registry: ModelRegistry, tokens: TokenSource) -> Self {
        let transport = Transport::new(settings.timeouts.clone(), settings.retry.clone());
        Self {
            settings,
            tokens: Arc::new(tokens),
            transport,
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

    /// 换上新配置：认证信息没变就沿用原 token 来源（保留缓存的 token），超时与重试没变就沿用连接池。
    /// 测试用的固定 token 始终沿用。
    pub fn with_settings(&self, settings: Settings) -> Self {
        let same_auth = settings.google.credentials_file == self.settings.google.credentials_file
            && settings.google.project_id == self.settings.google.project_id;
        let tokens = if same_auth || matches!(*self.tokens, TokenSource::Static { .. }) {
            self.tokens.clone()
        } else {
            Arc::new(Self::service_account_tokens(&settings))
        };
        let transport = if settings.timeouts == self.settings.timeouts
            && settings.retry == self.settings.retry
        {
            self.transport.clone()
        } else {
            Transport::new(settings.timeouts.clone(), settings.retry.clone())
        };
        Self {
            settings,
            tokens,
            transport,
            registry: self.registry.clone(),
        }
    }

    pub fn with_registry(&self, registry: ModelRegistry) -> Self {
        Self {
            settings: self.settings.clone(),
            tokens: self.tokens.clone(),
            transport: self.transport.clone(),
            registry: Arc::new(registry),
        }
    }

    pub fn registry(&self) -> &ModelRegistry {
        &self.registry
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
