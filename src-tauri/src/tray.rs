//! 托盘图标与菜单：打开主界面、切换 Gemini 档位、退出。文字跟随界面语言（见 [`crate::i18n`]）。

use std::sync::Arc;

use serde_json::json;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use vertexswitch_core::{SharedState, admin};

use crate::host::CoreHost;
use crate::i18n::tr;

const TRAY_ID: &str = "main";
/// 档位 id 与菜单文字（中文、英文）
const TIERS: [(&str, &str, &str); 3] = [
    ("standard", "Standard（标准）", "Standard"),
    ("flex", "Flex（低价，排队）", "Flex (cheaper, may queue)"),
    ("priority", "Priority（优先）", "Priority (served first)"),
];

fn open_text() -> &'static str {
    tr("打开 VertexSwitch", "Open VertexSwitch")
}

fn tier_menu_text() -> &'static str {
    tr("Gemini 默认档位", "Gemini default tier")
}

fn quit_text() -> &'static str {
    tr("退出", "Quit")
}

/// 菜单项的句柄：切换语言时改文字，档位变化时改勾选。
pub struct TrayMenu {
    open: MenuItem<Wry>,
    tier_menu: Submenu<Wry>,
    quit: MenuItem<Wry>,
    tiers: Vec<(&'static str, CheckMenuItem<Wry>)>,
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", open_text(), true, None::<&str>)?;
    let tiers = TIERS
        .iter()
        .map(|(id, zh, en)| {
            CheckMenuItem::with_id(
                app,
                format!("tier:{id}"),
                tr(zh, en),
                true,
                false,
                None::<&str>,
            )
            .map(|item| (*id, item))
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = tiers
        .iter()
        .map(|(_, item)| item as &dyn tauri::menu::IsMenuItem<Wry>)
        .collect();
    let tier_menu = Submenu::with_items(app, tier_menu_text(), true, &refs)?;
    let quit = MenuItem::with_id(app, "quit", quit_text(), true, None::<&str>)?;
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
    app.manage(TrayMenu {
        open,
        tier_menu,
        quit,
        tiers,
    });

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(crate::app_icon::tray())
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

/// 界面语言变了：菜单文字换成新语言，提示文字也跟着刷新。
pub fn relabel(app: &AppHandle, state: &SharedState) {
    if let Some(menu) = app.try_state::<TrayMenu>() {
        let _ = menu.open.set_text(open_text());
        let _ = menu.tier_menu.set_text(tier_menu_text());
        let _ = menu.quit.set_text(quit_text());
        for ((_, item), (_, zh, en)) in menu.tiers.iter().zip(TIERS) {
            let _ = item.set_text(tr(zh, en));
        }
    }
    sync(app, state);
}

/// 让勾选状态与提示文字跟上当前配置（配置也可能由界面或文件热加载修改）。
pub fn sync(app: &AppHandle, state: &SharedState) {
    let runtime = state.runtime();
    let settings = &runtime.settings;
    let tier = settings.gemini.tier.as_str();
    if let Some(menu) = app.try_state::<TrayMenu>() {
        for (id, item) in &menu.tiers {
            let _ = item.set_checked(*id == tier);
        }
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(format!(
            "VertexSwitch\n{}:{}\n{}{tier}",
            settings.server.host,
            settings.server.port,
            tr("Gemini 默认档位：", "Gemini default tier: "),
        )));
    }
}
