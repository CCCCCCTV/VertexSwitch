//! 界面调用的命令：直接调用核心的管理操作（与 `/admin/api` 行为一致）。

use std::sync::Arc;

use serde_json::{Value, json};
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
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
        "admin_token_location": host.admin_token_location,
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

/// 背景图片复制到配置目录下的 background.<扩展名>，界面启动时读回来。
const BACKGROUND_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif"];
const BACKGROUND_MAX_BYTES: u64 = 30 * 1024 * 1024;

fn background_files() -> impl Iterator<Item = std::path::PathBuf> {
    let dir = app_dir();
    BACKGROUND_EXTS
        .iter()
        .map(move |ext| dir.join(format!("background.{ext}")))
        .filter(|p| p.is_file())
}

fn remove_background() -> Result<(), String> {
    for path in background_files() {
        std::fs::remove_file(&path).map_err(|e| format!("删除旧背景图片失败：{e}"))?;
    }
    Ok(())
}

#[tauri::command]
pub fn set_background_image(path: String) -> CmdResult {
    let src = std::path::PathBuf::from(&path);
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| BACKGROUND_EXTS.contains(&e.as_str()))
        .ok_or("只支持 PNG、JPG、WebP、GIF、BMP、AVIF 图片")?;
    let size = std::fs::metadata(&src)
        .map_err(|e| format!("读取图片失败：{e}"))?
        .len();
    if size > BACKGROUND_MAX_BYTES {
        return Err("图片不能超过 30 MB".into());
    }
    let dir = app_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    remove_background()?;
    std::fs::copy(&src, dir.join(format!("background.{ext}")))
        .map_err(|e| format!("保存背景图片失败：{e}"))?;
    Ok(Value::Null)
}

#[tauri::command]
pub fn clear_background_image() -> CmdResult {
    remove_background()?;
    Ok(Value::Null)
}

/// 当前背景图片的原始字节；没有设置时返回空。
#[tauri::command]
pub fn get_background_image() -> Result<tauri::ipc::Response, String> {
    let bytes = match background_files().next() {
        Some(path) => std::fs::read(path).map_err(|e| format!("读取背景图片失败：{e}"))?,
        None => Vec::new(),
    };
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> CmdResult {
    let enabled = app.autolaunch().is_enabled().map_err(|e| e.to_string())?;
    Ok(json!({ "enabled": enabled }))
}

/// 开机自启（写当前用户的启动项，启动时带 --minimized 只在托盘运行）。
#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> CmdResult {
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    }
    .map_err(|e| e.to_string())?;
    tracing::info!("开机自启已{}", if enabled { "开启" } else { "关闭" });
    get_autostart(app)
}

/// 管理 token（供用户复制给 curl 等工具使用）。
#[tauri::command]
pub fn get_admin_token(host: Host<'_>) -> CmdResult {
    Ok(json!({ "token": host.state.admin_token }))
}

/// Windows 系统强调色（`#RRGGBB`），界面用它生成整套色调；其他平台或读取失败时为 null。
#[tauri::command]
pub fn get_system_accent() -> CmdResult {
    Ok(system_accent().map_or(Value::Null, Value::String))
}

/// 实时取色：scope 为 wallpaper（整张壁纸）或 behind（窗口后方）。
/// 返回 `{seed, cols, rows, field}`；读屏失败或不支持时为 null。
#[tauri::command]
pub async fn sample_backdrop(window: tauri::WebviewWindow, scope: String) -> CmdResult {
    use crate::backdrop_color::{Scope, sample};
    let scope = match scope.as_str() {
        "wallpaper" => Scope::Wallpaper,
        "behind" => Scope::Behind,
        other => return Err(format!("未知的取色范围：{other}")),
    };
    #[cfg(windows)]
    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as isize;
    #[cfg(not(windows))]
    let hwnd = {
        let _ = window;
        0
    };
    let result = tauri::async_runtime::spawn_blocking(move || sample(scope, hwnd))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

/// 液态玻璃：读一帧窗口内容区正后方的画面（宽不超过 max_width），连同取色结果一起返回。
///
/// 二进制格式（小端）：宽 u32、高 u32、取色 JSON 长度 u32、取色 JSON（同 sample_backdrop）、
/// 宽 × 高 × 4 字节 RGBA。读屏失败或不支持时返回空。
#[tauri::command]
pub async fn capture_glass_frame(
    window: tauri::WebviewWindow,
    max_width: u32,
) -> Result<tauri::ipc::Response, String> {
    #[cfg(windows)]
    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as isize;
    #[cfg(not(windows))]
    let hwnd = {
        let _ = window;
        0
    };
    let frame = tauri::async_runtime::spawn_blocking(move || {
        crate::backdrop_color::capture_frame(hwnd, max_width.clamp(64, 4096))
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(frame) = frame else {
        return Ok(tauri::ipc::Response::new(Vec::new()));
    };
    let sample = serde_json::to_vec(&frame.sample).map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(12 + sample.len() + frame.rgba.len());
    out.extend_from_slice(&frame.width.to_le_bytes());
    out.extend_from_slice(&frame.height.to_le_bytes());
    out.extend_from_slice(&(sample.len() as u32).to_le_bytes());
    out.extend_from_slice(&sample);
    out.extend_from_slice(&frame.rgba);
    Ok(tauri::ipc::Response::new(out))
}

/// 切换窗口背景材质（Windows 11）：mica、tabbed（Mica Alt）、acrylic（亚克力）或 none。
#[tauri::command]
pub fn set_backdrop(window: tauri::WebviewWindow, kind: String) -> CmdResult {
    use tauri::window::{Effect, EffectsBuilder};
    let effect = match kind.as_str() {
        "mica" => Some(Effect::Mica),
        "tabbed" => Some(Effect::Tabbed),
        "acrylic" => Some(Effect::Acrylic),
        "none" => None,
        other => return Err(format!("未知的窗口材质：{other}")),
    };
    window
        .set_effects(effect.map(|e| EffectsBuilder::new().effect(e).build()))
        .map_err(|e| e.to_string())?;
    Ok(Value::Null)
}

#[cfg(windows)]
pub fn system_accent() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let dwm = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\DWM")
        .ok()?;
    // AccentColor 按 0xAABBGGRR 存储
    let abgr: u32 = dwm.get_value("AccentColor").ok()?;
    let [r, g, b, _] = abgr.to_le_bytes();
    Some(format!("#{r:02X}{g:02X}{b:02X}"))
}

#[cfg(not(windows))]
pub fn system_accent() -> Option<String> {
    None
}

/// Windows 11（内部版本 ≥ 22000）才有 Mica；其他系统返回 false，窗口保持不透明。
#[cfg(windows)]
pub fn supports_mica() -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .and_then(|k| k.get_value::<String, _>("CurrentBuildNumber"))
        .ok()
        .and_then(|b| b.parse::<u32>().ok())
        .is_some_and(|build| build >= 22000)
}

#[cfg(not(windows))]
pub fn supports_mica() -> bool {
    false
}
