//! 窗口与托盘图标按屏幕缩放取正好合适的尺寸。
//!
//! Tauri 只拿 icon.ico 里最大的一帧（256px）当窗口和托盘图标，再让系统缩小；
//! 我们的图标在 16–24px 有专门的小尺寸版（光芒更少更粗），系统缩出来的效果不如它。
//! 所以窗口图标改从 exe 里嵌的 ICO 按尺寸取帧，托盘直接用对应尺寸的 PNG。

use tauri::image::Image;

/// 托盘图标：取最接近系统托盘图标尺寸的一张（16/20/24/32px）
pub fn tray() -> Image<'static> {
    let bytes: &'static [u8] = match small_icon_size() {
        ..=16 => include_bytes!("../icons/tray/16.png"),
        17..=20 => include_bytes!("../icons/tray/20.png"),
        21..=24 => include_bytes!("../icons/tray/24.png"),
        _ => include_bytes!("../icons/tray/32.png"),
    };
    Image::from_bytes(bytes).expect("内置的托盘图标是有效的 PNG")
}

/// 窗口标题栏与任务栏图标：让系统从 exe 内嵌的 ICO 里按当前缩放挑帧
pub fn apply_to_window(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        win::set_window_icons(hwnd.0 as _);
    }
    #[cfg(not(windows))]
    let _ = window;
}

#[cfg(windows)]
fn small_icon_size() -> u32 {
    win::small_icon_size()
}

#[cfg(not(windows))]
fn small_icon_size() -> u32 {
    32
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod win {
    use std::ptr::null;

    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow, GetSystemMetricsForDpi};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CXSMICON,
        SendMessageW, WM_SETICON,
    };

    /// tauri-build 把 icons/icon.ico 嵌进 exe 时用的资源号
    const APP_ICON_RESOURCE: u16 = tauri::utils::platform::WINDOWS_APP_ICON_RESOURCE_ID;

    pub fn small_icon_size() -> u32 {
        // SAFETY: 只读系统度量
        unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()).max(16) as u32 }
    }

    pub fn set_window_icons(hwnd: HWND) {
        // SAFETY: hwnd 是本进程刚创建的主窗口；图标句柄交给窗口后随进程存活，不需要释放
        unsafe {
            let dpi = GetDpiForWindow(hwnd);
            let module = GetModuleHandleW(null());
            for (which, metric) in [(ICON_SMALL, SM_CXSMICON), (ICON_BIG, SM_CXICON)] {
                let size = GetSystemMetricsForDpi(metric, dpi);
                let icon = LoadImageW(
                    module,
                    APP_ICON_RESOURCE as usize as *const u16,
                    IMAGE_ICON,
                    size,
                    size,
                    LR_DEFAULTCOLOR,
                );
                if !icon.is_null() {
                    SendMessageW(hwnd, WM_SETICON, which as usize, icon as isize);
                }
            }
        }
    }
}
