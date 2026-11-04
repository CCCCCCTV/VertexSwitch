//! 实时取色：从桌面上没被窗口挡住的区域采样壁纸，取出最有代表性的鲜明颜色作为界面的种子色。
//!
//! 直接读屏幕而不是读壁纸文件：Wallpaper Engine 这类动态壁纸、播放列表轮换时，
//! 壁纸文件和注册表都不会更新，只有屏幕上显示的才是当前画面。
//! 被窗口（包括本程序自己的窗口）挡住的部分不参与取色。

/// 屏幕缩小到这个网格再读像素
const GRID_W: i32 = 96;
const GRID_H: i32 = 60;

/// 露出的壁纸格子少于这个比例时不取色（几乎被窗口盖满，取到的不是壁纸）
const MIN_VISIBLE: f64 = 0.02;

/// 采样一次。露出的壁纸太少或读屏失败时返回 None，界面沿用上一次的颜色。
#[cfg(windows)]
pub fn sample() -> Option<String> {
    let screen = win::virtual_screen();
    let pixels = win::capture(&screen)?;
    let covers = win::covering_windows();
    let mut visible = Vec::new();
    for gy in 0..GRID_H {
        for gx in 0..GRID_W {
            // 整个格子都没被窗口挡住才算（缩放时格子里的像素会被平均）
            let cell = win::Rect {
                left: screen.left + gx * screen.width() / GRID_W,
                top: screen.top + gy * screen.height() / GRID_H,
                right: screen.left + (gx + 1) * screen.width() / GRID_W,
                bottom: screen.top + (gy + 1) * screen.height() / GRID_H,
            };
            if covers.iter().any(|r| r.intersects(&cell)) {
                continue;
            }
            let i = ((gy * GRID_W + gx) * 4) as usize;
            visible.push([pixels[i + 2], pixels[i + 1], pixels[i]]);
        }
    }
    if (visible.len() as f64) < f64::from(GRID_W * GRID_H) * MIN_VISIBLE {
        return None;
    }
    Some(dominant(&visible))
}

#[cfg(not(windows))]
pub fn sample() -> Option<String> {
    None
}

/// 取主色（`#RRGGBB`）：按色相分 24 档、以彩度平方加权投票，取得票最多的一档（连同两侧相邻档）的加权平均色；
/// 鲜明的像素太少（灰暗的壁纸）时退回整体平均色，界面会把它的彩度夹到最低值。
pub fn dominant(pixels: &[[u8; 3]]) -> String {
    const BINS: usize = 24;
    let mut weight = [0f64; BINS];
    let mut sum = [[0f64; 3]; BINS];
    let mut avg = [0f64; 3];
    let mut chromatic = 0usize;
    for p in pixels {
        let [r, g, b] = p.map(f64::from);
        avg[0] += r;
        avg[1] += g;
        avg[2] += b;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let c = max - min;
        // 太灰或太暗的像素不投票
        if c < 28.0 || max < 40.0 {
            continue;
        }
        chromatic += 1;
        let h = if max == r {
            ((g - b) / c).rem_euclid(6.0)
        } else if max == g {
            (b - r) / c + 2.0
        } else {
            (r - g) / c + 4.0
        } * 60.0;
        let bin = (h / (360.0 / BINS as f64)) as usize % BINS;
        let w = c * c;
        weight[bin] += w;
        for k in 0..3 {
            sum[bin][k] += p[k] as f64 * w;
        }
    }
    let n = pixels.len().max(1) as f64;
    if (chromatic as f64) < n * 0.05 {
        return hex(avg.map(|v| v / n));
    }
    let score = |i: usize| weight[(i + BINS - 1) % BINS] + weight[i] + weight[(i + 1) % BINS];
    let best = (0..BINS)
        .max_by(|&a, &b| score(a).total_cmp(&score(b)))
        .unwrap_or(0);
    let mut total = 0f64;
    let mut color = [0f64; 3];
    for i in [(best + BINS - 1) % BINS, best, (best + 1) % BINS] {
        total += weight[i];
        for k in 0..3 {
            color[k] += sum[i][k];
        }
    }
    hex(color.map(|v| v / total))
}

fn hex([r, g, b]: [f64; 3]) -> String {
    let c = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    format!("#{:02X}{:02X}{:02X}", c(r), c(g), c(b))
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod win {
    use std::ffi::c_void;
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
        DeleteDC, DeleteObject, GetDC, HALFTONE, ReleaseDC, SRCCOPY, SelectObject, SetBrushOrgEx,
        SetStretchBltMode, StretchBlt,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GWL_EXSTYLE, GetClassNameW, GetSystemMetrics, GetWindowLongPtrW, IsIconic,
        IsWindowVisible, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TRANSPARENT,
    };

    use super::{GRID_H, GRID_W};

    #[derive(Clone, Copy)]
    pub struct Rect {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }

    impl Rect {
        pub fn width(&self) -> i32 {
            self.right - self.left
        }
        pub fn height(&self) -> i32 {
            self.bottom - self.top
        }
        pub fn intersects(&self, o: &Rect) -> bool {
            self.left < o.right && o.left < self.right && self.top < o.bottom && o.top < self.bottom
        }
    }

    /// 所有显示器拼起来的虚拟屏幕（物理像素；Tauri 进程是按显示器感知 DPI 的）
    pub fn virtual_screen() -> Rect {
        // SAFETY: 只读系统度量
        unsafe {
            let left = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let top = GetSystemMetrics(SM_YVIRTUALSCREEN);
            Rect {
                left,
                top,
                right: left + GetSystemMetrics(SM_CXVIRTUALSCREEN),
                bottom: top + GetSystemMetrics(SM_CYVIRTUALSCREEN),
            }
        }
    }

    /// 把整个屏幕缩放到 GRID_W × GRID_H 读出来（BGRA）
    pub fn capture(screen: &Rect) -> Option<Vec<u8>> {
        // SAFETY: 标准 GDI 截屏流程，所有句柄在返回前释放；位图内存由 DIB 段持有，读完再删
        unsafe {
            let screen_dc = GetDC(null_mut());
            if screen_dc.is_null() {
                return None;
            }
            let mem = CreateCompatibleDC(screen_dc);
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: GRID_W,
                biHeight: -GRID_H,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            };
            let mut bits: *mut c_void = null_mut();
            let dib = CreateDIBSection(mem, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            let mut out = None;
            if !dib.is_null() && !bits.is_null() {
                let old = SelectObject(mem, dib);
                SetStretchBltMode(mem, HALFTONE);
                SetBrushOrgEx(mem, 0, 0, null_mut());
                let ok = StretchBlt(
                    mem,
                    0,
                    0,
                    GRID_W,
                    GRID_H,
                    screen_dc,
                    screen.left,
                    screen.top,
                    screen.width(),
                    screen.height(),
                    SRCCOPY,
                );
                if ok != 0 {
                    let len = (GRID_W * GRID_H * 4) as usize;
                    out = Some(std::slice::from_raw_parts(bits as *const u8, len).to_vec());
                }
                SelectObject(mem, old);
                DeleteObject(dib);
            }
            DeleteDC(mem);
            ReleaseDC(null_mut(), screen_dc);
            out
        }
    }

    /// 挡在壁纸前面的窗口：可见、没最小化、没被系统隐藏（cloaked，例如其他虚拟桌面上的窗口）、
    /// 不是透明覆盖层，也不是桌面本身。
    /// 覆盖层指点击穿透的窗口，以及分层且不接受激活的窗口（例如 NVIDIA 覆盖层铺满全屏但几乎全透明）。
    pub fn covering_windows() -> Vec<Rect> {
        unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> i32 {
            // SAFETY: data 是下面传进来的 Vec 指针，只在 EnumWindows 期间使用
            unsafe {
                let out = &mut *(data as *mut Vec<Rect>);
                if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
                    return 1;
                }
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
                let overlay = WS_EX_LAYERED | WS_EX_NOACTIVATE;
                if ex & WS_EX_TRANSPARENT != 0 || ex & overlay == overlay {
                    return 1;
                }
                let mut cloaked = 0u32;
                DwmGetWindowAttribute(
                    hwnd,
                    DWMWA_CLOAKED as u32,
                    (&mut cloaked as *mut u32).cast(),
                    4,
                );
                if cloaked != 0 {
                    return 1;
                }
                let mut class = [0u16; 32];
                let n = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
                let class = String::from_utf16_lossy(&class[..n.max(0) as usize]);
                if class == "Progman" || class == "WorkerW" {
                    return 1;
                }
                // 用 DWM 的可见边框，不含窗口四周看不见的拖拽边
                let mut r: RECT = std::mem::zeroed();
                let got = DwmGetWindowAttribute(
                    hwnd,
                    DWMWA_EXTENDED_FRAME_BOUNDS as u32,
                    (&mut r as *mut RECT).cast(),
                    size_of::<RECT>() as u32,
                );
                if got == 0 && r.right > r.left && r.bottom > r.top {
                    out.push(Rect {
                        left: r.left,
                        top: r.top,
                        right: r.right,
                        bottom: r.bottom,
                    });
                }
                1
            }
        }
        let mut out: Vec<Rect> = Vec::new();
        // SAFETY: 回调只在本调用期间运行
        unsafe {
            EnumWindows(Some(each), &mut out as *mut Vec<Rect> as LPARAM);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::dominant;

    #[test]
    fn picks_the_vivid_majority_over_grey() {
        let mut px = vec![[60, 62, 66]; 700];
        px.extend(vec![[200, 40, 120]; 200]); // 洋红
        px.extend(vec![[40, 160, 60]; 60]); // 少量绿
        let c = dominant(&px);
        assert_eq!(c, "#C82878");
    }

    #[test]
    fn grey_wallpaper_falls_back_to_average() {
        let px = vec![[44, 51, 55]; 500];
        assert_eq!(dominant(&px), "#2C3337");
    }

    #[test]
    fn hue_wraps_around_red() {
        // 色相 355° 与 5° 落在相邻两档的两端，应合并成一种红
        let mut px = vec![[220, 30, 45]; 100];
        px.extend(vec![[220, 45, 30]; 100]);
        px.extend(vec![[30, 30, 220]; 120]);
        let c = dominant(&px);
        assert!(c.starts_with("#DC"), "{c}");
    }
}
