//! 界面调用的命令：直接调用核心的管理操作（与 `/admin/api` 行为一致）。

use std::sync::Arc;

use serde_json::{Value, json};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use vertexswitch_core::admin;
use vertexswitch_core::config::app_dir;
use vertexswitch_core::errors::ProxyError;

use crate::host::CoreHost;
use crate::tray;

type Host<'a> = State<'a, Arc<CoreHost>>;
type CmdResult = Result<Value, String>;

fn err(e: ProxyError) -> String {
    e.message
}

#[tauri::command]
pub async fn get_status(host: Host<'_>) -> CmdResult {
    let mut status = admin::status(&host.state).await;
    status["server"] = host.server_info().await;
    Ok(status)
}

#[tauri::command]
pub fn get_settings(host: Host<'_>) -> CmdResult {
    Ok(serde_json::to_value(admin::settings(&host.state)).expect("配置可序列化"))
}

/// 修改配置并立即生效；监听地址变了就重启监听。
#[tauri::command]
pub async fn patch_settings(app: AppHandle, host: Host<'_>, patch: Value) -> CmdResult {
    let mut result = admin::patch_settings(&host.state, &patch).map_err(err)?;
    if result["restart_required"] == true {
        result["restart_error"] = host.ensure_listening().await.err().into();
    }
    result["server"] = host.server_info().await;
    tray::sync(&app, &host.state);
    Ok(result)
}

#[tauri::command]
pub async fn set_tier(app: AppHandle, host: Host<'_>, tier: String) -> CmdResult {
    patch_settings(app, host, json!({"gemini": {"tier": tier}})).await
}

#[tauri::command]
pub async fn restart_server(host: Host<'_>) -> CmdResult {
    host.ensure_listening().await.map_err(|e| e.to_string())?;
    Ok(host.server_info().await)
}

#[tauri::command]
pub fn get_models(host: Host<'_>) -> CmdResult {
    Ok(admin::models(&host.state))
}

#[tauri::command]
pub fn reload_models(host: Host<'_>) -> CmdResult {
    admin::reload_models(&host.state).map_err(err)
}

/// 把内置模型表复制到用户目录（已存在则不覆盖）并用系统默认程序打开。
#[tauri::command]
pub fn edit_models(app: AppHandle, host: Host<'_>) -> CmdResult {
    let path = admin::export_models(&host.state).map_err(err)?;
    app.opener()
        .open_path(&path, None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "path": path }))
}

#[tauri::command]
pub async fn refresh_ollama(host: Host<'_>) -> CmdResult {
    Ok(admin::refresh_ollama(&host.state).await)
}

#[tauri::command]
pub fn get_logs(host: Host<'_>, since: u64, limit: usize) -> CmdResult {
    Ok(admin::logs(&host.state, since, limit))
}

#[tauri::command]
pub fn get_requests(host: Host<'_>, since: u64, limit: usize) -> CmdResult {
    Ok(admin::requests(&host.state, since, limit))
}

#[tauri::command]
pub fn get_metrics(host: Host<'_>) -> CmdResult {
    Ok(admin::metrics(&host.state))
}

#[tauri::command]
pub async fn send_test(host: Host<'_>, model: String) -> CmdResult {
    admin::send_test(&host.state, &model, "Reply with exactly: pong", 32)
        .await
        .map_err(err)
}

#[tauri::command]
pub fn get_app_info(host: Host<'_>) -> CmdResult {
    Ok(json!({
        "version": vertexswitch_core::VERSION,
        "config_dir": app_dir().display().to_string(),
        "config_path": host.state.config_path.as_ref().map(|p| p.display().to_string()),
        "admin_token_path": host.admin_token_path.display().to_string(),
    }))
}

#[tauri::command]
pub fn open_config_dir(app: AppHandle) -> CmdResult {
    let dir = app_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(Value::Null)
}
