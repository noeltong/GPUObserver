//! 对话框基础：模态循环、扁平风格按钮（owner-draw）、原生控件辅助、文件打开对话框。
//! 对应 C# SettingsForm/ServerEditForm 的公共部分（FlatStyle.Flat 按钮、MessageBox、OpenFileDialog）。

use super::gfx::{self, FontCache, FontSpec, Argb, Rect};
use super::theme;
use super::win;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::Controls::{ODS_SELECTED, UDM_GETPOS32, UDM_SETPOS32};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetWindowLongPtrW, GetMessageW, IsWindow, TranslateMessage, GWL_USERDATA,
    MB_APPLMODAL, MB_DEFBUTTON2, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_ICONWARNING,
    MB_OK, MB_OKCANCEL, MB_TASKMODAL, MB_YESNO, MESSAGEBOX_RESULT, MESSAGEBOX_STYLE,
};
use windows::core::PCWSTR;

/// 扁平按钮样式（对应 C# MakeButton + FlatStyle.Flat）。
#[derive(Clone, Copy, PartialEq)]
pub enum FlatButtonStyle {
    Normal,
    Primary,
    Danger,
}

pub const BS_OWNERDRAW_FLAG: u32 = 0x0000_000B;

pub struct FlatButtonSpec {
    pub text: &'static str,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub style: FlatButtonStyle,
}

/// 创建 owner-draw 扁平按钮。
pub unsafe fn create_flat_button(parent: HWND, spec: &FlatButtonSpec, id: i32, dpi: u32) -> HWND {
    let (x, y) = (win::s(spec.x, dpi), win::s(spec.y, dpi));
    let (w, h) = (win::s(spec.w, dpi), win::s(spec.h, dpi));
    let hwnd = win::create_control(
        "BUTTON",
        spec.text,
        0x4000_0000 | 0x1000_0000 /* WS_CHILD | WS_VISIBLE */ | BS_OWNERDRAW_FLAG,
        0,
        x,
        y,
        w,
        h,
        parent,
        id,
    );
    set_button_style(hwnd, spec.style);
    hwnd
}

/// 按钮样式位存进 GWL_USERDATA（WM_DRAWITEM 时读取）。
pub unsafe fn set_button_style(hwnd: HWND, style: FlatButtonStyle) {
    let data: isize = match style {
        FlatButtonStyle::Normal => 0,
        FlatButtonStyle::Primary => 0x1,
        FlatButtonStyle::Danger => 0x2,
    };
    windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(hwnd, GWL_USERDATA, data);
    let _ = InvalidateRect(Some(hwnd), None, true);
}

/// WM_DRAWITEM 里绘制扁平按钮。
pub unsafe fn draw_flat_button(
    draw_item: *mut windows::Win32::UI::Controls::DRAWITEMSTRUCT,
    fonts: &mut FontCache,
) {
    if draw_item.is_null() {
        return;
    }
    let item = &*draw_item;
    let state = item.itemState.0;
    let style_bits = GetWindowLongPtrW(item.hwndItem, GWL_USERDATA) as usize;
    let is_primary = (style_bits & 0x1) != 0;
    let is_danger = (style_bits & 0x2) != 0;
    let disabled = (state & 0x4 /* ODS_DISABLED */) != 0;
    let hot = (state & 0x80 /* ODS_HOTLIGHT */) != 0;
    let selected = (state & ODS_SELECTED.0) != 0;

    let (mut bg, mut fg): (Argb, Argb) = if is_primary {
        if hot || selected {
            (theme::ACCENT_DARK, theme::WHITE)
        } else {
            (theme::ACCENT, theme::WHITE)
        }
    } else if selected {
        (theme::PRESSED, if is_danger { theme::RED } else { theme::TEXT })
    } else if hot {
        (theme::HOVER, if is_danger { theme::RED } else { theme::TEXT })
    } else {
        (theme::CARD_BACKGROUND, if is_danger { theme::RED } else { theme::TEXT })
    };
    if disabled {
        bg = theme::WINDOW_BACKGROUND;
        fg = theme::MUTED_TEXT;
    }

    let rect = Rect::new(
        item.rcItem.left as f32,
        item.rcItem.top as f32,
        (item.rcItem.right - item.rcItem.left) as f32,
        (item.rcItem.bottom - item.rcItem.top) as f32,
    );

    let hdc = item.hDC;
    if let Some(g) = gfx::Graphics::from_hdc(hdc) {
        g.fill_rounded(bg, rect, 8.0);
        if !is_primary && !disabled {
            g.draw_rounded(
                theme::BORDER_STRONG,
                1.0,
                Rect::new(rect.x + 0.5, rect.y + 0.5, rect.w - 1.0, rect.h - 1.0),
                8.0,
            );
        }
        let text = get_window_text(item.hwndItem);
        if let Some(font) = fonts.gdi_font(&FontSpec::ui(8.75)) {
            gfx::gdi_draw_text(hdc, &text, font, fg, rect, gfx::TextFormat::CENTER);
        }
    }
}

pub unsafe fn get_window_text(hwnd: HWND) -> String {
    let len = windows::Win32::UI::WindowsAndMessaging::GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; len as usize + 1];
    let copied = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut buffer);
    win::from_wide(&buffer[..copied.max(0) as usize])
}

pub unsafe fn set_window_text(hwnd: HWND, text: &str) {
    let wide = win::to_wide(text);
    windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
}

/// 模态循环：禁用所有者窗口直到 dialog 关闭（对应 WinForms ShowDialog）。
pub unsafe fn run_modal(dialog: HWND, owners: &[HWND]) {
    for owner in owners {
        if !owner.is_invalid() && IsWindow(Some(*owner)).as_bool() {
            let _ = EnableWindow(*owner, false);
        }
    }

    let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
    while IsWindow(Some(dialog)).as_bool() {
        let result = GetMessageW(&mut msg, None, 0, 0);
        if result.0 <= 0 {
            break;
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&mut msg);
    }

    for owner in owners {
        if !owner.is_invalid() && IsWindow(Some(*owner)).as_bool() {
            let _ = EnableWindow(*owner, true);
        }
    }
}

/// 结束模态：销毁对话框（模态循环随即退出），结果存在 GWL_HINSTANCE... 不，存在窗口指针 0。
pub unsafe fn end_modal(dialog: HWND, result: isize) {
    // 复用 DWLP_USER（索引 0x0? 对普通窗口 GWL_HINSTANCE 无害）——改用独立全局表更稳。
    // 这里直接把结果存到 GWL_USERDATA 由窗口过程在 WM_DESTROY 前读取。
    modal_results()
        .lock()
        .unwrap()
        .insert(dialog.0 as usize, result);
    windows::Win32::UI::WindowsAndMessaging::DestroyWindow(dialog);
}

static MODAL_RESULTS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<usize, isize>>> =
    std::sync::OnceLock::new();

fn modal_results() -> &'static std::sync::Mutex<std::collections::HashMap<usize, isize>> {
    MODAL_RESULTS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

pub unsafe fn modal_result(dialog: HWND) -> isize {
    *modal_results()
        .lock()
        .unwrap()
        .get(&(dialog.0 as usize))
        .unwrap_or(&0)
}

/// MessageBox 包装（对应 C# MessageBox.Show）。
pub fn message_box(
    parent: HWND,
    text: &str,
    title: &str,
    kind: MessageBoxKind,
    buttons: MessageBoxButtons,
) -> MessageBoxResult {
    use windows::Win32::UI::WindowsAndMessaging::MessageBoxW;
    let text_wide = win::to_wide(text);
    let title_wide = win::to_wide(title);
    let mut flags: MESSAGEBOX_STYLE = MB_APPLMODAL;
    if parent.is_invalid() {
        flags |= MB_TASKMODAL;
    }
    match kind {
        MessageBoxKind::Info => flags |= MB_ICONINFORMATION,
        MessageBoxKind::Warning => flags |= MB_ICONWARNING,
        MessageBoxKind::Error => flags |= MB_ICONERROR,
        MessageBoxKind::Question => flags |= MB_ICONQUESTION,
    }
    match buttons {
        MessageBoxButtons::Ok => flags |= MB_OK,
        MessageBoxButtons::YesNoCancelDefault2 => flags |= MB_YESNO | MB_DEFBUTTON2,
        MessageBoxButtons::OkCancel => flags |= MB_OKCANCEL,
    }
    unsafe {
        let answer = MessageBoxW(
            Some(parent),
            PCWSTR(text_wide.as_ptr()),
            PCWSTR(title_wide.as_ptr()),
            flags,
        );
        if answer == MESSAGEBOX_RESULT(6) {
            MessageBoxResult::Yes
        } else if answer == MESSAGEBOX_RESULT(7) {
            MessageBoxResult::No
        } else if answer == MESSAGEBOX_RESULT(1) {
            MessageBoxResult::Ok
        } else {
            MessageBoxResult::Cancel
        }
    }
}

pub enum MessageBoxKind {
    Info,
    Warning,
    Error,
    Question,
}

pub enum MessageBoxButtons {
    Ok,
    YesNoCancelDefault2,
    OkCancel,
}

pub enum MessageBoxResult {
    Ok,
    Yes,
    No,
    Cancel,
}

/// WinForms OpenFileDialog 对应物（IFileDialog COM）。
pub fn open_file_dialog(parent: HWND, title: &str, filter: &str) -> Option<String> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{FileOpenDialog, IFileDialog, SIGDN_FILESYSPATH};

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let com_initialized = hr.is_ok();

        let result = (|| -> Option<String> {
            let dialog: IFileDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            let title_wide = win::to_wide(title);
            dialog.SetTitle(PCWSTR(title_wide.as_ptr())).ok()?;

            // "名称|ext1;ext2|名称2|*" → COM 文件类型列表（宽字符串必须存活到 Show 之后）
            let mut storage: Vec<(Vec<u16>, Vec<u16>)> = Vec::new();
            let parts: Vec<&str> = filter.split('|').collect();
            let mut index = 0;
            while index + 1 < parts.len() {
                let name = parts[index];
                let pattern = parts[index + 1];
                let specs: Vec<String> = pattern
                    .split(';')
                    .filter(|spec| spec.contains('*'))
                    .map(str::to_string)
                    .collect();
                if !specs.is_empty() {
                    storage.push((win::to_wide(name), win::to_wide(&specs.join(";"))));
                }
                index += 2;
            }
            let types: Vec<windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC> = storage
                .iter()
                .map(|(name, spec)| windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC {
                    pszName: PCWSTR(name.as_ptr()),
                    pszSpec: PCWSTR(spec.as_ptr()),
                })
                .collect();
            if !types.is_empty() {
                let _ = dialog.SetFileTypes(&types);
            }
            dialog.Show(Some(parent)).ok()?;
            let path = dialog.GetResult().ok()?;
            let display = path.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            Some(display.to_string().ok()?)
        })();

        if com_initialized {
            CoUninitialize();
        }
        result
    }
}

/// 初始化公共控件（Button/UpDown 等）。
pub fn init_common_controls() {
    unsafe {
        let icc = windows::Win32::UI::Controls::INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<windows::Win32::UI::Controls::INITCOMMONCONTROLSEX>() as u32,
            dwICC: windows::Win32::UI::Controls::ICC_STANDARD_CLASSES,
        };
        let _ = windows::Win32::UI::Controls::InitCommonControlsEx(&icc);
    }
}

/// 取 EDIT 控件文本。
pub fn edit_get_text(hwnd: HWND) -> String {
    unsafe { get_window_text(hwnd) }
}

/// 取 UpDown(UDS_SETBUDDYINT) 当前整数值。
pub fn updown_get_value(updown: HWND) -> i32 {
    unsafe {
        let result = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            updown,
            UDM_GETPOS32,
            None,
            Some(LPARAM(0)),
        );
        result.0 as i32
    }
}

/// 设置 UpDown 当前整数值。
pub fn updown_set_value(updown: HWND, value: i32) {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            updown,
            UDM_SETPOS32,
            None,
            Some(LPARAM(value as isize)),
        );
    }
}
