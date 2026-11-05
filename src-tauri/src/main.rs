//! VertexSwitch 桌面程序：托盘外壳 + 管理界面，代理核心在同一进程内运行。
//!
//! 关闭窗口只是隐藏到托盘；从托盘菜单"退出"才结束进程。`--minimized` 启动时不显示窗口。

// 发布版不弹控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_icon;
mod backdrop_color;
mod commands;
mod host;
mod title_bar;
mod tray;

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, WindowEvent};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use vertexswitch_core::admin::broadcast_stream;
use vertexswitch_core::logs::{LogHub, LogLayer};

use crate::host::CoreHost;

fn init_logging() -> Arc<LogHub> {
    let logs = Arc::new(LogHub::new(1000));
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_ansi(false))
        .with(LogLayer::new(logs.clone()))
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    logs
}

/// 把核心的广播（日志、请求记录）转成界面事件。
fn forward<T: Serialize + Clone + Send + 'static>(
    app: AppHandle,
    event: &'static str,
    rx: tokio::sync::broadcast::Receiver<T>,
) {
    tauri::async_runtime::spawn(async move {
        let mut stream = std::pin::pin!(broadcast_stream(rx));
        while let Some(item) = stream.next().await {
            let _ = app.emit(event, item);
        }
    });
}

fn main() {
    // 最先读回界面语言：读取配置时的告警、托盘菜单都要用它
    vertexswitch_core::i18n::load();
    let logs = init_logging();
    let host = CoreHost::create(logs.clone());
    let start_minimized = std::env::args().any(|a| a == "--minimized");

    tauri::Builder::default()
        // 必须最先注册：已有实例在运行时，新启动的进程把参数转给它后立即退出
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        // 开机自启时只在托盘运行
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(host.clone())
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_settings,
            commands::patch_settings,
            commands::set_tier,
            commands::restart_server,
            commands::get_models,
            commands::reload_models,
            commands::edit_models,
            commands::refresh_ollama,
            commands::get_logs,
            commands::get_requests,
            commands::get_metrics,
            commands::send_test,
            commands::get_app_info,
            commands::open_config_dir,
            commands::set_background_image,
            commands::clear_background_image,
            commands::get_background_image,
            commands::get_autostart,
            commands::set_autostart,
            commands::get_admin_token,
            commands::get_system_accent,
            commands::set_backdrop,
            commands::set_title_bar,
            commands::sample_backdrop,
            commands::set_language,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            // 主窗口在这里创建（配置里 create: false），以便在页面加载前注入系统强调色，
            // 界面第一帧就用正确的色调，不会先画成后备色再跳变
            let accent = commands::system_accent().unwrap_or_default();
            let mica = commands::supports_mica();
            let window_config = app
                .config()
                .app
                .windows
                .first()
                .cloned()
                .expect("tauri.conf.json 里定义了主窗口");
            let mut builder = tauri::WebviewWindowBuilder::from_config(&handle, &window_config)?
                .initialization_script(format!(
                    "window.__VS_ACCENT__ = {accent:?}; window.__VS_MICA__ = {mica};"
                ));
            // Windows 11：透明窗口 + Mica（设置里可换成 Mica Alt / 亚克力 / 不透明），界面底色改为半透明
            if mica {
                builder = builder.transparent(true).effects(
                    tauri::window::EffectsBuilder::new()
                        .effect(tauri::window::Effect::Mica)
                        .build(),
                );
            }
            let window = builder.build()?;
            app_icon::apply_to_window(&window);
            title_bar::hide_caption_content(&window);
            tray::create(&handle)?;
            forward(handle.clone(), "log", logs.subscribe());
            forward(
                handle.clone(),
                "request",
                host.state.metrics.requests.subscribe(),
            );

            // 启动核心；失败（如端口被占用）时界面里能看到原因并修改端口
            let starter = host.clone();
            tauri::async_runtime::spawn(async move {
                let _ = starter.start().await;
            });

            // 托盘勾选与提示跟上配置（配置也可能被文件热加载修改）
            let syncer = handle.clone();
            let state = host.state.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tray::sync(&syncer, &state);
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            });

            if !start_minimized {
                tray::show_main(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("启动 VertexSwitch 失败");
}
