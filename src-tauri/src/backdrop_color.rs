//! 实时取色：从桌面背景取界面的种子色和一张粗略的“颜色场”。
//!
//! 两种范围：
//! - 整张壁纸：让桌面窗口（Progman）把自己完整画一遍再取色，被其他窗口挡住也不影响。
//!   Wallpaper Engine 这类动态壁纸挂在桌面窗口下面，取到的是当前画面；
//!   直接读壁纸文件不行：播放列表轮换时文件和注册表都不会变。
//! - 窗口后方：让本窗口下面的桌面和各个应用窗口分别把自己画进内存，按前后顺序叠成本窗口背后的样子；
//!   后面是别的应用就取那些应用的颜色。不动本窗口的截屏属性，所以用户随时截图、录屏都看得到本窗口
//!   （早先的做法是取样瞬间把本窗口排除出截屏，取得勤时截图里本窗口就总是不见）。
//!
//! 颜色场是把取到的画面缩成 FIELD_COLS × FIELD_ROWS 的平均色，界面把它模糊后铺满背景，
//! 颜色的位置保持不变：后面左边是红色的应用、右边是蓝色的应用，界面也是左红右蓝。

use serde::Serialize;

/// 画面先缩到这个网格再读像素
const GRID_W: usize = 96;
const GRID_H: usize = 60;

const FIELD_COLS: usize = 8;
const FIELD_ROWS: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Wallpaper,
    Behind,
}

#[derive(Debug, Serialize)]
pub struct Sample {
    /// 种子色 `#RRGGBB`
    pub seed: String,
    pub cols: usize,
    pub rows: usize,
    /// 颜色场，按行排列，共 cols × rows 个 `#RRGGBB`
    pub field: Vec<String>,
}

/// 取样一次；读屏失败或当前平台不支持时返回 None，界面沿用上一次的颜色。
#[cfg(windows)]
pub fn sample(scope: Scope, window: isize) -> Option<Sample> {
    let bgra = match scope {
        Scope::Wallpaper => win::capture_wallpaper()?,
        Scope::Behind => win::capture_behind(window as win::Hwnd)?,
    };
    Some(summarize(&bgra, scope))
}

#[cfg(not(windows))]
pub fn sample(_scope: Scope, _window: isize) -> Option<Sample> {
    None
}

/// 把 GRID_W × GRID_H 的 BGRA 像素整理成种子色与颜色场
fn summarize(bgra: &[u8], scope: Scope) -> Sample {
    let mut pixels = Vec::with_capacity(GRID_W * GRID_H);
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            let i = (y * GRID_W + x) * 4;
            let weight = match scope {
                // 桌面图标多排在边上：越靠画面中间权重越高，主色更接近壁纸本身
                Scope::Wallpaper => {
                    let dx = (x as f64 + 0.5) / GRID_W as f64 * 2.0 - 1.0;
                    let dy = (y as f64 + 0.5) / GRID_H as f64 * 2.0 - 1.0;
                    1.0 - 0.3 * (dx * dx + dy * dy)
                }
                Scope::Behind => 1.0,
            };
            pixels.push(([bgra[i + 2], bgra[i + 1], bgra[i]], weight));
        }
    }
    Sample {
        seed: dominant(&pixels),
        cols: FIELD_COLS,
        rows: FIELD_ROWS,
        field: field(bgra),
    }
}

/// 每个格子的平均色
fn field(bgra: &[u8]) -> Vec<String> {
    let mut out = Vec::with_capacity(FIELD_COLS * FIELD_ROWS);
    for row in 0..FIELD_ROWS {
        for col in 0..FIELD_COLS {
            let mut sum = [0f64; 3];
            let mut n = 0f64;
            for y in row * GRID_H / FIELD_ROWS..(row + 1) * GRID_H / FIELD_ROWS {
                for x in col * GRID_W / FIELD_COLS..(col + 1) * GRID_W / FIELD_COLS {
                    let i = (y * GRID_W + x) * 4;
                    sum[0] += f64::from(bgra[i + 2]);
                    sum[1] += f64::from(bgra[i + 1]);
                    sum[2] += f64::from(bgra[i]);
                    n += 1.0;
                }
            }
            out.push(hex(sum.map(|v| v / n)));
        }
    }
    out
}

/// 取主色（`#RRGGBB`）：按色相分 24 档、以彩度平方 × 权重投票，取得票最多的一档（连同两侧相邻档）的加权平均色；
/// 鲜明的像素太少（灰暗的画面）时退回整体平均色，界面会把它的彩度夹到最低值。
fn dominant(pixels: &[([u8; 3], f64)]) -> String {
    const BINS: usize = 24;
    let mut votes = [0f64; BINS];
    let mut sum = [[0f64; 3]; BINS];
    let mut avg = [0f64; 3];
    let mut total_weight = 0f64;
    let mut chromatic = 0f64;
    for &(p, weight) in pixels {
        let [r, g, b] = p.map(f64::from);
        for k in 0..3 {
            avg[k] += f64::from(p[k]) * weight;
        }
        total_weight += weight;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let c = max - min;
        // 太灰或太暗的像素不投票
        if c < 28.0 || max < 40.0 {
            continue;
        }
        chromatic += weight;
        let h = if max == r {
            ((g - b) / c).rem_euclid(6.0)
        } else if max == g {
            (b - r) / c + 2.0
        } else {
            (r - g) / c + 4.0
        } * 60.0;
        let bin = (h / (360.0 / BINS as f64)) as usize % BINS;
        let w = c * c * weight;
        votes[bin] += w;
        for k in 0..3 {
            sum[bin][k] += f64::from(p[k]) * w;
        }
    }
    if total_weight <= 0.0 {
        return hex([0.0; 3]);
    }
    if chromatic < total_weight * 0.05 {
        return hex(avg.map(|v| v / total_weight));
    }
    let score = |i: usize| votes[(i + BINS - 1) % BINS] + votes[i] + votes[(i + 1) % BINS];
    let best = (0..BINS)
        .max_by(|&a, &b| score(a).total_cmp(&score(b)))
        .unwrap_or(0);
    let mut total = 0f64;
    let mut color = [0f64; 3];
    for i in [(best + BINS - 1) % BINS, best, (best + 1) % BINS] {
        total += votes[i];
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
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
        CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HALFTONE, HDC, ReleaseDC,
        SRCCOPY, SelectObject, SetBrushOrgEx, SetStretchBltMode, StretchBlt,
    };
    use windows_sys::Win32::Storage::Xps::PrintWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FindWindowW, GWL_EXSTYLE, GetClassNameW, GetWindowLongPtrW, GetWindowRect,
        IsHungAppWindow, IsIconic, IsWindowVisible, PW_RENDERFULLCONTENT, WS_EX_LAYERED,
        WS_EX_NOACTIVATE, WS_EX_TRANSPARENT,
    };

    /// 最多叠这么多层窗口（离本窗口最近的那些），再多的被盖在下面也看不到
    const MAX_LAYERS: usize = 12;

    use super::{GRID_H, GRID_W};

    pub type Hwnd = HWND;

    /// 把 src 的一块区域缩放到 GRID_W × GRID_H 读出来（BGRA）
    ///
    /// SAFETY: src 必须是有效的设备上下文
    unsafe fn shrink(src: HDC, x: i32, y: i32, w: i32, h: i32) -> Option<Vec<u8>> {
        if w <= 0 || h <= 0 {
            return None;
        }
        // SAFETY: 标准 GDI 流程，所有句柄在返回前释放；位图内存由 DIB 段持有，读完再删
        unsafe {
            let mem = CreateCompatibleDC(src);
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: GRID_W as i32,
                biHeight: -(GRID_H as i32),
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
                    GRID_W as i32,
                    GRID_H as i32,
                    src,
                    x,
                    y,
                    w,
                    h,
                    SRCCOPY,
                );
                if ok != 0 {
                    let len = GRID_W * GRID_H * 4;
                    out = Some(std::slice::from_raw_parts(bits as *const u8, len).to_vec());
                }
                SelectObject(mem, old);
                DeleteObject(dib);
            }
            DeleteDC(mem);
            out
        }
    }

    /// 整张壁纸：让桌面窗口把自己（壁纸、动态壁纸和桌面图标）完整画进内存位图
    pub fn capture_wallpaper() -> Option<Vec<u8>> {
        let class: Vec<u16> = "Progman\0".encode_utf16().collect();
        // SAFETY: 同 shrink；PrintWindow 只往我们自己的内存位图里画
        unsafe {
            let desktop = FindWindowW(class.as_ptr(), null());
            if desktop.is_null() {
                return None;
            }
            let mut r: RECT = std::mem::zeroed();
            if GetWindowRect(desktop, &mut r) == 0 {
                return None;
            }
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            if w <= 0 || h <= 0 {
                return None;
            }
            let screen = GetDC(null_mut());
            if screen.is_null() {
                return None;
            }
            let mem = CreateCompatibleDC(screen);
            let bitmap = CreateCompatibleBitmap(screen, w, h);
            let mut out = None;
            if !bitmap.is_null() {
                let old = SelectObject(mem, bitmap);
                if PrintWindow(desktop, mem, PW_RENDERFULLCONTENT) != 0 {
                    out = shrink(mem, 0, 0, w, h);
                }
                SelectObject(mem, old);
                DeleteObject(bitmap);
            }
            DeleteDC(mem);
            ReleaseDC(null_mut(), screen);
            out
        }
    }

    fn intersect(a: &RECT, b: &RECT) -> Option<RECT> {
        let r = RECT {
            left: a.left.max(b.left),
            top: a.top.max(b.top),
            right: a.right.min(b.right),
            bottom: a.bottom.min(b.bottom),
        };
        (r.right > r.left && r.bottom > r.top).then_some(r)
    }

    /// 窗口在屏幕上看得见的范围：DWM 的可见边框，不含四周看不见的拖拽边
    ///
    /// SAFETY: hwnd 可以是任意窗口句柄，失效时返回 None
    unsafe fn visible_bounds(hwnd: HWND) -> Option<RECT> {
        // SAFETY: 只读窗口属性
        unsafe {
            let mut r: RECT = std::mem::zeroed();
            let got = DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS as u32,
                (&mut r as *mut RECT).cast(),
                size_of::<RECT>() as u32,
            );
            if got != 0 && GetWindowRect(hwnd, &mut r) == 0 {
                return None;
            }
            (r.right > r.left && r.bottom > r.top).then_some(r)
        }
    }

    /// 本窗口下面、和 area 有重叠的窗口，从最底下往上排（最多 MAX_LAYERS 个，取离本窗口最近的）。
    /// 跳过隐藏、最小化、被系统隐藏（cloaked，比如在别的虚拟桌面）、点击穿透或分层不激活的覆盖层、
    /// 没响应的窗口（让它画会卡住），以及桌面本身（桌面单独画在最底层）。
    ///
    /// SAFETY: window 是本进程的主窗口
    unsafe fn windows_below(window: HWND, area: &RECT) -> Vec<(HWND, RECT)> {
        struct Scan {
            me: HWND,
            area: RECT,
            below: bool,
            found: Vec<(HWND, RECT)>,
        }
        unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> i32 {
            // SAFETY: data 是下面传进来的 Scan 指针，只在 EnumWindows 期间使用
            unsafe {
                let scan = &mut *(data as *mut Scan);
                // EnumWindows 从最上面往下数，过了本窗口之后的才在它下面
                if hwnd == scan.me {
                    scan.below = true;
                    return 1;
                }
                if !scan.below || IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
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
                if cloaked != 0 || IsHungAppWindow(hwnd) != 0 {
                    return 1;
                }
                let mut class = [0u16; 32];
                let n = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
                let class = String::from_utf16_lossy(&class[..n.max(0) as usize]);
                if class == "Progman" || class == "WorkerW" {
                    return 1;
                }
                if let Some(r) = visible_bounds(hwnd)
                    && intersect(&r, &scan.area).is_some()
                {
                    scan.found.push((hwnd, r));
                }
                1
            }
        }
        let mut scan = Scan {
            me: window,
            area: *area,
            below: false,
            found: Vec::new(),
        };
        // SAFETY: 回调只在本调用期间运行
        unsafe {
            EnumWindows(Some(each), &mut scan as *mut Scan as LPARAM);
        }
        scan.found.truncate(MAX_LAYERS);
        scan.found.reverse();
        scan.found
    }

    /// 让 hwnd 把自己完整画进临时位图，再把它在 visible 里、和 area 重叠的部分贴到 canvas
    /// （canvas 的原点对应 area 的左上角）。画失败（比如权限更高的窗口）就跳过。
    ///
    /// SAFETY: canvas、screen 是有效的设备上下文
    unsafe fn paint(canvas: HDC, screen: HDC, hwnd: HWND, visible: &RECT, area: &RECT) {
        // SAFETY: 所有句柄在返回前释放
        unsafe {
            let Some(clip) = intersect(visible, area) else {
                return;
            };
            let mut whole: RECT = std::mem::zeroed();
            if GetWindowRect(hwnd, &mut whole) == 0 {
                return;
            }
            let (w, h) = (whole.right - whole.left, whole.bottom - whole.top);
            if w <= 0 || h <= 0 {
                return;
            }
            let mem = CreateCompatibleDC(screen);
            let bitmap = CreateCompatibleBitmap(screen, w, h);
            if !bitmap.is_null() {
                let old = SelectObject(mem, bitmap);
                if PrintWindow(hwnd, mem, PW_RENDERFULLCONTENT) != 0 {
                    BitBlt(
                        canvas,
                        clip.left - area.left,
                        clip.top - area.top,
                        clip.right - clip.left,
                        clip.bottom - clip.top,
                        mem,
                        clip.left - whole.left,
                        clip.top - whole.top,
                        SRCCOPY,
                    );
                }
                SelectObject(mem, old);
                DeleteObject(bitmap);
            }
            DeleteDC(mem);
        }
    }

    /// 窗口后方：桌面打底，再把本窗口下面的应用窗口从下往上叠，得到本窗口背后的样子。
    /// 不读屏幕，也不改本窗口的截屏属性。
    pub fn capture_behind(window: HWND) -> Option<Vec<u8>> {
        let class: Vec<u16> = "Progman\0".encode_utf16().collect();
        // SAFETY: window 是本进程自己的主窗口；其余同 shrink
        unsafe {
            let area = visible_bounds(window)?;
            let (w, h) = (area.right - area.left, area.bottom - area.top);
            let screen = GetDC(null_mut());
            if screen.is_null() {
                return None;
            }
            let canvas = CreateCompatibleDC(screen);
            let bitmap = CreateCompatibleBitmap(screen, w, h);
            let mut out = None;
            if !bitmap.is_null() {
                let old = SelectObject(canvas, bitmap);
                let desktop = FindWindowW(class.as_ptr(), null());
                let mut desk: RECT = std::mem::zeroed();
                if !desktop.is_null() && GetWindowRect(desktop, &mut desk) != 0 {
                    paint(canvas, screen, desktop, &desk, &area);
                }
                for (hwnd, visible) in windows_below(window, &area) {
                    paint(canvas, screen, hwnd, &visible, &area);
                }
                out = shrink(canvas, 0, 0, w, h);
                SelectObject(canvas, old);
                DeleteObject(bitmap);
            }
            DeleteDC(canvas);
            ReleaseDC(null_mut(), screen);
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(px: &[[u8; 3]]) -> Vec<([u8; 3], f64)> {
        px.iter().map(|&p| (p, 1.0)).collect()
    }

    #[test]
    fn picks_the_vivid_majority_over_grey() {
        let mut px = vec![[60, 62, 66]; 700];
        px.extend(vec![[200, 40, 120]; 200]); // 洋红
        px.extend(vec![[40, 160, 60]; 60]); // 少量绿
        assert_eq!(dominant(&flat(&px)), "#C82878");
    }

    #[test]
    fn grey_picture_falls_back_to_average() {
        let px = vec![[44, 51, 55]; 500];
        assert_eq!(dominant(&flat(&px)), "#2C3337");
    }

    #[test]
    fn hue_wraps_around_red() {
        // 色相 355° 与 5° 落在相邻两档的两端，应合并成一种红
        let mut px = vec![[220, 30, 45]; 100];
        px.extend(vec![[220, 45, 30]; 100]);
        px.extend(vec![[30, 30, 220]; 120]);
        let c = dominant(&flat(&px));
        assert!(c.starts_with("#DC"), "{c}");
    }

    #[test]
    fn weight_shifts_the_vote() {
        // 同样多的红和蓝，蓝的权重更高
        let mut px: Vec<([u8; 3], f64)> = vec![([220, 30, 30], 0.5); 100];
        px.extend(vec![([30, 30, 220], 1.0); 100]);
        assert_eq!(dominant(&px), "#1E1EDC");
    }

    #[test]
    fn field_keeps_where_colours_are() {
        // 左半边红（应用 A）、右半边蓝（应用 B）
        let mut bgra = vec![0u8; GRID_W * GRID_H * 4];
        for y in 0..GRID_H {
            for x in 0..GRID_W {
                let i = (y * GRID_W + x) * 4;
                let [r, g, b] = if x < GRID_W / 2 {
                    [220, 40, 40]
                } else {
                    [40, 80, 220]
                };
                bgra[i..i + 4].copy_from_slice(&[b, g, r, 255]);
            }
        }
        let s = summarize(&bgra, Scope::Behind);
        assert_eq!((s.cols, s.rows, s.field.len()), (8, 5, 40));
        for row in 0..5 {
            assert_eq!(s.field[row * 8], "#DC2828");
            assert_eq!(s.field[row * 8 + 7], "#2850DC");
        }
    }
}
