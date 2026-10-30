//! Win32 基础设施：窗口类注册、DPI 缩放、字符串与控件辅助。

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassExW, HMENU, WNDCLASSEXW,
    WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::PCWSTR;

/// 逻辑像素缩放：S(v) = round(v * dpi / 96)（与 C# 一致）。
pub fn s(value: i32, dpi: u32) -> i32 {
    ((value as f64) * (dpi as f64) / 96.0).round() as i32
}

/// 浮点版缩放（弹窗内容绘制使用，与 C# 的 DeviceDpi/96f 对应）。
pub fn sf(value: f32, dpi: u32) -> f32 {
    (value * (dpi as f32) / 96.0).round()
}

pub type WndProcType = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

pub fn hinstance() -> HINSTANCE {
    unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .ok()
            .map(|h| h.into())
            .unwrap_or_default()
    }
}

/// 注册窗口类（已注册时返回 true 由调用方忽略错误）。
pub fn register_class(name: &str, proc: WndProcType, extra_bytes: usize, style: u32) -> bool {
    let wide = to_wide(name);
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: windows::Win32::UI::WindowsAndMessaging::WNDCLASS_STYLES(style),
            lpfnWndProc: Some(proc),
            cbWndExtra: extra_bytes as i32,
            hInstance: hinstance(),
            hCursor: windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                None,
                windows::Win32::UI::WindowsAndMessaging::IDC_ARROW,
            )
            .unwrap_or_default(),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH::default(),
            lpszClassName: PCWSTR(wide.as_ptr()),
            ..Default::default()
        };
        let atom = RegisterClassExW(&wc);
        atom != 0
    }
}

pub fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(value: &[u16]) -> String {
    let end = value.iter().position(|c| *c == 0).unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

/// 由客户区尺寸计算外框尺寸（按窗口 DPI，用于 FixedDialog 窗口）。
pub fn client_to_outer(
    client_w: i32,
    client_h: i32,
    style: u32,
    ex_style: u32,
    dpi: u32,
) -> (i32, i32) {
    let mut rect = RECT { left: 0, top: 0, right: client_w, bottom: client_h };
    unsafe {
        let _ = AdjustWindowRectExForDpi(
            &mut rect,
            windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(style),
            false,
            windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(ex_style),
            dpi,
        );
    }
    (rect.right - rect.left, rect.bottom - rect.top)
}

/// 用指定字体测量单行文本的像素宽度（供 STATIC 标签自动加宽，字体已是 DPI 像素高）。
pub fn measure_text_width(font: windows::Win32::Graphics::Gdi::HFONT, text: &str) -> i32 {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        GetDC, GetTextExtentPoint32W, ReleaseDC, SelectObject,
    };
    if text.is_empty() {
        return 0;
    }
    let wide = to_wide(text);
    unsafe {
        let hdc = GetDC(None);
        if hdc.is_invalid() {
            return 0;
        }
        let old = SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(font.0 as *mut core::ffi::c_void));
        let mut size = SIZE::default();
        let _ = GetTextExtentPoint32W(hdc, &wide, &mut size);
        SelectObject(hdc, old);
        ReleaseDC(None, hdc);
        size.cx
    }
}

/// 顶层窗口 DWM 圆角（Win11；旧系统静默失败无害）。
pub fn round_corners(hwnd: HWND) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWM_WINDOW_CORNER_PREFERENCE, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_ROUND,
    };
    unsafe {
        let preference = DWM_WINDOW_CORNER_PREFERENCE(DWMWCP_ROUND.0);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
    }
}

/// 创建子控件（EDIT/STATIC/BUTTON/LISTBOX 等）的便捷入口。
pub unsafe fn create_control(
    class: &str,
    text: &str,
    style: u32,
    ex_style: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    parent: HWND,
    id: i32,
) -> HWND {
    let class_wide = to_wide(class);
    let text_wide = to_wide(text);
    CreateWindowExW(
        WINDOW_EX_STYLE(ex_style),
        PCWSTR(class_wide.as_ptr()),
        PCWSTR(text_wide.as_ptr()),
        WINDOW_STYLE(style),
        x,
        y,
        w,
        h,
        Some(parent),
        Some(HMENU(id as *mut core::ffi::c_void)),
        Some(hinstance().into()),
        None,
    )
    .unwrap_or_default()
}

/// 默认窗口过程转发。
pub unsafe fn default_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// 居中数学：窗口（outer_w × outer_h）在工作区（left,top,right,bottom）内居中。
/// 窗口大于工作区时贴工作区左上角（Windows 居中的惯例处理）。
fn center_in_work_area(work: (i32, i32, i32, i32), outer_w: i32, outer_h: i32) -> (i32, i32) {
    let (left, top, right, bottom) = work;
    let area_w = (right - left).max(0);
    let area_h = (bottom - top).max(0);
    let x = left + (area_w - outer_w).max(0) / 2;
    let y = top + (area_h - outer_h).max(0) / 2;
    (x, y)
}

/// 对话框居中定位（对应 C# StartPosition.CenterScreen）：
/// 取光标所在显示器的工作区，返回外框尺寸（outer_w × outer_h）的居中坐标。
/// 有属主窗口不能用 CW_USEDEFAULT（会落到属主位置附近），必须显式给坐标。
pub fn center_on_cursor_monitor(outer_w: i32, outer_h: i32) -> (i32, i32) {
    unsafe {
        let mut cursor = windows::Win32::Foundation::POINT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut cursor);
        let monitor = windows::Win32::Graphics::Gdi::MonitorFromPoint(
            cursor,
            windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTONEAREST,
        );
        let mut info = windows::Win32::Graphics::Gdi::MONITORINFO {
            cbSize: std::mem::size_of::<windows::Win32::Graphics::Gdi::MONITORINFO>() as u32,
            ..Default::default()
        };
        if windows::Win32::Graphics::Gdi::GetMonitorInfoW(monitor, &mut info).as_bool() {
            let work = (info.rcWork.left, info.rcWork.top, info.rcWork.right, info.rcWork.bottom);
            return center_in_work_area(work, outer_w, outer_h);
        }
        // 兜底：主显示器工作区（SPI_GETWORKAREA）
        let mut work = RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
            windows::Win32::UI::WindowsAndMessaging::SPI_GETWORKAREA,
            0,
            Some(&mut work as *mut RECT as *mut core::ffi::c_void),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        center_in_work_area((work.left, work.top, work.right, work.bottom), outer_w, outer_h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_places_window_at_middle_of_work_area() {
        // 1920×1080 工作区，680×580 对话框
        let (x, y) = center_in_work_area((0, 0, 1920, 1040), 680, 580);
        assert_eq!(x, (1920 - 680) / 2);
        assert_eq!(y, (1040 - 580) / 2);
    }

    #[test]
    fn center_offsets_from_nonzero_origin() {
        // 副显示器在主显示器右侧（1920, 0）起
        let (x, y) = center_in_work_area((1920, 0, 3840, 1040), 680, 580);
        assert_eq!(x, 1920 + (1920 - 680) / 2);
        assert_eq!(y, (1040 - 580) / 2);
    }

    #[test]
    fn center_clamps_oversized_window_to_work_area_origin() {
        let (x, y) = center_in_work_area((0, 0, 800, 600), 1000, 900);
        assert_eq!((x, y), (0, 0));
    }
}
