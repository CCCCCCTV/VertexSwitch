//! 标题栏和界面背景融成一片：保留 Windows 原生标题栏（贴靠布局、拖动、读屏都照常），
//! 只把它的底色设成界面背景色，并且不画标题栏上的小图标和标题文字
//! （界面左上角已经有图标，标题栏上再来一个就重复了）。任务栏和 Alt+Tab 里的图标与标题不受影响。
//!
//! 依据：DwmSetWindowAttribute 的 DWMWA_CAPTION_COLOR / DWMWA_USE_IMMERSIVE_DARK_MODE（Windows 11 起），
//! SetWindowThemeAttribute 的 WTNCA_NODRAWICON / WTNCA_NODRAWCAPTION。

/// 窗口建好后调用一次：不画标题栏上的图标和标题文字
pub fn hide_caption_content(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        win::hide_caption_content(hwnd.0 as _);
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// 标题栏底色：`#RRGGBB`，None 表示交还系统（比如 Mica 透出来时）。`dark` 让标题栏按钮用浅色图标。
pub fn set_colors(window: &tauri::WebviewWindow, color: Option<[u8; 3]>, dark: bool) {
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        win::set_colors(hwnd.0 as _, color, dark);
    }
    #[cfg(not(windows))]
    let _ = (window, color, dark);
}

/// 解析 `#RRGGBB`
pub fn parse_hex(hex: &str) -> Option<[u8; 3]> {
    let hex = hex.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(hex, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod win {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_CAPTION_COLOR, DWMWA_COLOR_DEFAULT, DWMWA_USE_IMMERSIVE_DARK_MODE,
        DwmSetWindowAttribute,
    };
    use windows_sys::Win32::UI::Controls::{
        SetWindowThemeAttribute, WTA_NONCLIENT, WTA_OPTIONS, WTNCA_NODRAWCAPTION, WTNCA_NODRAWICON,
    };

    pub fn hide_caption_content(hwnd: HWND) {
        let flags = WTNCA_NODRAWCAPTION | WTNCA_NODRAWICON;
        let options = WTA_OPTIONS {
            dwFlags: flags,
            dwMask: flags,
        };
        // SAFETY: hwnd 是本进程刚创建的主窗口；options 在调用期间有效
        unsafe {
            SetWindowThemeAttribute(
                hwnd,
                WTA_NONCLIENT,
                (&options as *const WTA_OPTIONS).cast(),
                size_of::<WTA_OPTIONS>() as u32,
            );
        }
    }

    pub fn set_colors(hwnd: HWND, color: Option<[u8; 3]>, dark: bool) {
        // COLORREF 是 0x00BBGGRR
        let caption = color.map_or(DWMWA_COLOR_DEFAULT, |[r, g, b]| {
            u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)
        });
        let dark = i32::from(dark);
        // SAFETY: hwnd 是本进程的主窗口；属性值在调用期间有效。旧系统不认这些属性时调用失败，不影响窗口
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                (&dark as *const i32).cast(),
                size_of::<i32>() as u32,
            );
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR as u32,
                (&caption as *const u32).cast(),
                size_of::<u32>() as u32,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_hex;

    #[test]
    fn hex_colors() {
        assert_eq!(parse_hex("#1A2b3C"), Some([0x1A, 0x2B, 0x3C]));
        assert_eq!(parse_hex("1A2B3C"), None);
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("#zzzzzz"), None);
    }
}
