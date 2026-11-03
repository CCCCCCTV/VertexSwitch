//! Google 服务账号 access token 管理（SPEC §2）。
//!
//! 依据 <https://developers.google.com/identity/protocols/oauth2/service-account#httprest>：
//! 用密钥里的 `private_key` 以 RS256 签 JWT，POST 到 `token_uri` 换取 access token。
//!
//! - 剩余有效期少于 5 分钟就刷新；刷新过程持锁，并发请求只有一个真正去刷新
//! - 后台定时刷新；失败只记日志，不让进程退出
//! - 不读取、不打印密钥内容以外的任何敏感信息；日志里不出现 token

use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub const SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
const JWT_LIFETIME_SECS: u64 = 3600;
const EARLY_REFRESH: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct AuthError(pub String);

#[derive(Deserialize)]
struct ServiceAccountKey {
    client_email: String,
    private_key: String,
    #[serde(default)]
    private_key_id: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    token_uri: Option<String>,
}

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: u64,
    exp: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

struct Loaded {
    key: ServiceAccountKey,
    encoding_key: EncodingKey,
}

#[derive(Default)]
struct State {
    loaded: Option<Loaded>,
    token: Option<(String, Instant)>,
}

/// 用服务账号密钥文件获取 token。
pub struct ServiceAccountAuth {
    credentials_file: Option<PathBuf>,
    explicit_project: String,
    http: reqwest::Client,
    state: Mutex<State>,
    project_id: RwLock<String>,
    last_error: RwLock<Option<String>>,
}

impl ServiceAccountAuth {
    pub fn new(credentials_file: Option<PathBuf>, project_id: &str, http: reqwest::Client) -> Self {
        Self {
            credentials_file,
            explicit_project: project_id.to_owned(),
            http,
            state: Mutex::new(State::default()),
            project_id: RwLock::new(project_id.to_owned()),
            last_error: RwLock::new(None),
        }
    }

    fn load(&self) -> Result<Loaded, AuthError> {
        let path = self
            .credentials_file
            .as_ref()
            .ok_or_else(|| AuthError("未配置服务账号密钥文件（google.credentials_file）".into()))?;
        let text = std::fs::read_to_string(path)
            .map_err(|e| AuthError(format!("无法读取服务账号密钥文件 {}：{e}", path.display())))?;
        let key: ServiceAccountKey = serde_json::from_str(&text)
            .map_err(|e| AuthError(format!("服务账号密钥文件格式错误：{e}")))?;
        let encoding_key = EncodingKey::from_rsa_pem(key.private_key.as_bytes())
            .map_err(|e| AuthError(format!("服务账号私钥无效：{e}")))?;
        if self.explicit_project.is_empty()
            && let Some(project) = &key.project_id
        {
            *self.project_id.write().expect("锁未中毒") = project.clone();
        }
        Ok(Loaded { key, encoding_key })
    }

    pub fn project_id(&self) -> String {
        self.project_id.read().expect("锁未中毒").clone()
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error.read().expect("锁未中毒").clone()
    }

    pub async fn get_token(&self) -> Result<String, AuthError> {
        let mut state = self.state.lock().await;
        if let Some((token, expiry)) = &state.token
            && expiry.saturating_duration_since(Instant::now()) > EARLY_REFRESH
        {
            return Ok(token.clone());
        }
        self.refresh_locked(&mut state).await
    }

    /// 强制刷新（后台定时任务用）。
    pub async fn refresh(&self) -> Result<String, AuthError> {
        let mut state = self.state.lock().await;
        self.refresh_locked(&mut state).await
    }

    async fn refresh_locked(&self, state: &mut State) -> Result<String, AuthError> {
        let result = self.fetch(state).await;
        match &result {
            Ok(_) => {
                *self.last_error.write().expect("锁未中毒") = None;
                tracing::info!("Google access token 已刷新");
            }
            Err(e) => *self.last_error.write().expect("锁未中毒") = Some(e.0.clone()),
        }
        result
    }

    async fn fetch(&self, state: &mut State) -> Result<String, AuthError> {
        if state.loaded.is_none() {
            state.loaded = Some(self.load()?);
        }
        let loaded = state.loaded.as_ref().expect("刚加载");
        let token_uri = loaded.key.token_uri.as_deref().unwrap_or(DEFAULT_TOKEN_URI);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let claims = Claims {
            iss: &loaded.key.client_email,
            scope: SCOPE,
            aud: token_uri,
            iat: now,
            exp: now + JWT_LIFETIME_SECS,
        };
        let mut header = Header::new(Algorithm::RS256);
        header.kid.clone_from(&loaded.key.private_key_id);
        let assertion = jsonwebtoken::encode(&header, &claims, &loaded.encoding_key)
            .map_err(|e| AuthError(format!("签名 JWT 失败：{e}")))?;

        let requested_at = Instant::now();
        let response = self
            .http
            .post(token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ])
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| AuthError(format!("获取 Google access token 失败：{e}")))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let detail: String = body.chars().take(500).collect();
            return Err(AuthError(format!(
                "获取 Google access token 失败：HTTP {status} {detail}"
            )));
        }
        let parsed: TokenResponse = response
            .json()
            .await
            .map_err(|e| AuthError(format!("Google token 响应格式错误：{e}")))?;
        let expiry = requested_at + Duration::from_secs(parsed.expires_in);
        state.token = Some((parsed.access_token.clone(), expiry));
        Ok(parsed.access_token)
    }
}

/// token 来源：服务账号，或测试用的固定值。
pub enum TokenSource {
    Static { token: String, project_id: String },
    ServiceAccount(Box<ServiceAccountAuth>),
}

impl TokenSource {
    pub fn fixed(token: &str, project_id: &str) -> Self {
        Self::Static {
            token: token.into(),
            project_id: project_id.into(),
        }
    }

    pub async fn get_token(&self) -> Result<String, AuthError> {
        match self {
            Self::Static { token, .. } => Ok(token.clone()),
            Self::ServiceAccount(auth) => auth.get_token().await,
        }
    }

    pub fn project_id(&self) -> String {
        match self {
            Self::Static { project_id, .. } => project_id.clone(),
            Self::ServiceAccount(auth) => auth.project_id(),
        }
    }

    /// 后台循环：先取一次，然后按间隔刷新。异常只记录。
    pub async fn run_background_refresh(&self, interval: Duration) {
        let Self::ServiceAccount(auth) = self else {
            return;
        };
        loop {
            if let Err(e) = auth.refresh().await {
                tracing::warn!("{e}");
            }
            tokio::time::sleep(interval).await;
        }
    }
}
