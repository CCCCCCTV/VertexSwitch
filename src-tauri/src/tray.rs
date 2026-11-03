//! 托盘图标与菜单：打开主界面、切换 Gemini 档位、退出。

use std::sync::Arc;

use serde_json::json;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use vertexswitch_core::{SharedState, admin};

use crate::host::CoreHost;

const TRAY_ID: &str = "main";
pub const TIERS: [(&str, &str); 3] = [
    ("standard", "Standard（标准）"),
    ("flex", "Flex（低价，排队）"),
    ("priority", "Priority（优先）"),
];

/// 档位勾选项，供 [`sync`] 更新。
pub struct TierItems(Vec<(&'static str, CheckMenuItem<Wry>)>);

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开 VertexSwitch", true, None::<&str>)?;
    let items = TIERS
        .iter()
        .map(|(id, label)| {
            CheckMenuItem::with_id(app, format!("tier:{id}"), *label, true, false, None::<&str>)
                .map(|item| (*id, item))
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = items
        .iter()
        .map(|(_, item)| item as &dyn tauri::menu::IsMenuItem<Wry>)
        .collect();
    let tier_menu = Submenu::with_items(app, "Gemini 档位", true, &refs)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &PredefinedMenuItem::separator(app)?,
            &tier_menu,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    app.manage(TierItems(items));

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(
            app.default_window_icon()
                .cloned()
                .expect("打包时已配置图标"),
        )
        .tooltip("VertexSwitch")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            if id == "open" {
                show_main(app);
            } else if id == "quit" {
                app.exit(0);
            } else if let Some(tier) = id.strip_prefix("tier:") {
                set_tier(app, tier.to_owned());
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn set_tier(app: &AppHandle, tier: String) {
    let host = app.state::<Arc<CoreHost>>().inner().clone();
    match admin::patch_settings(&host.state, &json!({"gemini": {"tier": tier}})) {
        Ok(_) => {
            // 通知界面刷新
            let _ = app.emit("settings-changed", ());
        }
        Err(e) => tracing::warn!("切换档位失败：{}", e.message),
    }
    sync(app, &host.state);
}

/// 让勾选状态与提示文字跟上当前配置（配置也可能由界面或文件热加载修改）。
pub fn sync(app: &AppHandle, state: &SharedState) {
    let runtime = state.runtime();
    let settings = &runtime.settings;
    let tier = settings.gemini.tier.as_str();
    if let Some(items) = app.try_state::<TierItems>() {
        for (id, item) in &items.0 {
            let _ = item.set_checked(*id == tier);
        }
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(format!(
            "VertexSwitch\n{}:{}\nGemini 档位：{tier}",
            settings.server.host, settings.server.port
        )));
    }
}
