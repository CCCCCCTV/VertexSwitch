//! 在本进程里运行代理核心：启动后台任务、监听端口，监听地址变化时重启监听。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use vertexswitch_core::app::{Background, load_or_create_admin_token};
use vertexswitch_core::config::{Settings, app_dir, config_path};
use vertexswitch_core::logs::LogHub;
use vertexswitch_core::registry::ModelRegistry;
use vertexswitch_core::{AppState, SharedState, serve_until, start_background};

struct Running {
    addr: String,
    shutdown: oneshot::Sender<()>,
}

pub struct CoreHost {
    pub state: SharedState,
    pub admin_token_path: PathBuf,
    running: tokio::sync::Mutex<Option<Running>>,
    background: tokio::sync::Mutex<Option<Background>>,
    last_error: Mutex<Option<String>>,
}

impl CoreHost {
    /// 读取配置与模型表（失败时用默认值并记录原因，界面里可以看到并修正）。
    pub fn create(logs: Arc<LogHub>) -> Arc<Self> {
        let config_file = config_path();
        let settings = Settings::load(&config_file).unwrap_or_else(|e| {
            tracing::warn!("{e}；暂用默认配置");
            Settings::default()
        });
        let registry = ModelRegistry::load().unwrap_or_else(|e| {
            tracing::warn!("{e}；暂用内置模型表");
            ModelRegistry::builtin()
        });
        let admin_token_path = app_dir().join("admin_token");
        let mut state = AppState::from_settings(settings, registry)
            .with_logs(logs)
            .with_paths(Some(config_file), Some(app_dir().join("models.toml")));
        match load_or_create_admin_token(&admin_token_path) {
            Ok(token) => state = state.with_admin_token(token),
            Err(e) => tracing::warn!("无法读写管理 token 文件：{e}；本次使用临时 token"),
        }
        Arc::new(Self {
            state: Arc::new(state),
            admin_token_path,
            running: tokio::sync::Mutex::new(None),
            background: tokio::sync::Mutex::new(None),
            last_error: Mutex::new(None),
        })
    }

    fn listen_addr(&self) -> String {
        let settings = &self.state.runtime().settings;
        format!("{}:{}", settings.server.host, settings.server.port)
    }

    /// 启动后台任务（只做一次）并开始监听。
    pub async fn start(&self) -> Result<String, String> {
        {
            let mut background = self.background.lock().await;
            if background.is_none() {
                *background = Some(start_background(&self.state).await);
            }
        }
        self.listen().await
    }

    async fn listen(&self) -> Result<String, String> {
        let mut running = self.running.lock().await;
        if let Some(old) = running.take() {
            let _ = old.shutdown.send(());
        }
        let addr = self.listen_addr();
        // 旧监听刚关闭时端口可能还没释放，稍等重试
        let mut bound = None;
        let mut last = String::new();
        for _ in 0..10 {
            match TcpListener::bind(&addr).await {
                Ok(listener) => {
                    bound = Some(listener);
                    break;
                }
                Err(e) => {
                    last = e.to_string();
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
        let Some(listener) = bound else {
            let message = format!("无法监听 {addr}：{last}");
            tracing::error!("{message}");
            *self.last_error.lock().expect("锁未中毒") = Some(message.clone());
            return Err(message);
        };
        let (tx, rx) = oneshot::channel();
        let state = self.state.clone();
        tauri::async_runtime::spawn(async move {
            let shutdown = async {
                let _ = rx.await;
            };
            if let Err(e) = serve_until(listener, state, shutdown).await {
                tracing::error!("代理服务异常退出：{e}");
            }
        });
        tracing::info!(
            "VertexSwitch {} 监听 http://{addr}",
            vertexswitch_core::VERSION
        );
        *self.last_error.lock().expect("锁未中毒") = None;
        *running = Some(Running {
            addr: addr.clone(),
            shutdown: tx,
        });
        Ok(addr)
    }

    /// 监听地址与配置不一致时重启监听。
    pub async fn ensure_listening(&self) -> Result<String, String> {
        let wanted = self.listen_addr();
        {
            let running = self.running.lock().await;
            if let Some(r) = running.as_ref()
                && r.addr == wanted
            {
                return Ok(wanted);
            }
        }
        self.listen().await
    }

    pub async fn server_info(&self) -> Value {
        let running = self.running.lock().await;
        json!({
            "running": running.is_some(),
            "addr": running.as_ref().map(|r| r.addr.clone()),
            "error": self.last_error.lock().expect("锁未中毒").clone(),
        })
    }
}
