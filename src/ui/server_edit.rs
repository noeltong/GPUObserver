//! 服务器编辑窗口：对应 C# ServerEditForm。ClientSize 600×588。

use super::dialog::{self, FlatButtonSpec, FlatButtonStyle};
use super::gfx::FontCache;
use super::theme;
use super::win;
use crate::config::{ServerAuthType, ServerConfig};
use crate::secrets;
use crate::ssh;
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, EM_SETPASSWORDCHAR};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW, SendMessageW,
    SetForegroundWindow, SetTimer, SetWindowLongPtrW, ShowWindow, CW_USEDEFAULT, GWLP_USERDATA, SW_HIDE, SW_SHOW, WINDOW_STYLE, WM_COMMAND, WM_CTLCOLORSTATIC,
    WM_DRAWITEM, WM_ERASEBKGND, WM_KEYDOWN, WM_TIMER, WM_USER, WS_CHILD, WS_VISIBLE,
};

const CLIENT_W: i32 = 600;
const CLIENT_H: i32 = 588;

const IDC_NAME: i32 = 200;
const IDC_HOST: i32 = 201;
const IDC_PORT_EDIT: i32 = 202;
const IDC_PORT_UPDOWN: i32 = 203;
const IDC_USERNAME: i32 = 204;
const IDC_AUTH_TYPE: i32 = 205;
const IDC_PASSWORD: i32 = 206;
const IDC_SHOW_PASSWORD: i32 = 207;
const IDC_KEY_PATH: i32 = 208;
const IDC_BROWSE: i32 = 209;
const IDC_PASSPHRASE: i32 = 210;
const IDC_SHOW_PASSPHRASE: i32 = 211;
const IDC_NOTIFY: i32 = 212;
const IDC_FINGERPRINT: i32 = 213;
const IDC_CLEAR_FINGERPRINT: i32 = 214;
const IDC_TEST: i32 = 215;
const IDC_TEST_RESULT: i32 = 216;
const IDC_ERROR: i32 = 217;
const IDC_SAVE: i32 = 218;
const IDC_CANCEL: i32 = 219;

const TIMER_TEST_POLL: usize = 1;
const WM_APP_TEST_DONE: u32 = WM_USER + 0x77;

struct EditState {
    hwnd: HWND,
    dpi: u32,
    fonts: FontCache,
    existing: Option<ServerConfig>,
    fingerprint_value: Option<String>,
    loading: bool,
    testing: Arc<Mutex<bool>>,
    test_rx: Option<std::sync::mpsc::Receiver<ssh::ConnectionTestResult>>,
    result: Option<ServerConfig>,
}

pub fn show_dialog(
    owner: HWND,
    existing: Option<ServerConfig>,
    runtime: tokio::runtime::Handle,
) -> Option<ServerConfig> {
    // 存 runtime 供测试连接使用（thread_local，避免穿过窗口指针）。
    RUNTIME.with(|cell| {
        *cell.borrow_mut() = Some(runtime.clone());
    });

    unsafe {
        let dpi = super::gfx::screen_dpi();
        // WS_CLIPCHILDREN：同设置窗，保护 owner-draw 子控件。
        let style = (windows::Win32::UI::WindowsAndMessaging::WS_OVERLAPPEDWINDOW.0
            | windows::Win32::UI::WindowsAndMessaging::WS_CLIPCHILDREN.0)
            & !(windows::Win32::UI::WindowsAndMessaging::WS_MAXIMIZEBOX.0
                | windows::Win32::UI::WindowsAndMessaging::WS_THICKFRAME.0);
        let (outer_w, outer_h) = win::client_to_outer(win::s(CLIENT_W, dpi), win::s(CLIENT_H, dpi), style, 0, dpi);

        let class = win::to_wide(super::app::SERVER_EDIT_WINDOW_CLASS);
        let title = win::to_wide(if existing.is_none() {
            super::i18n::t().se_add_title
        } else {
            super::i18n::t().se_edit_title
        });
        // 有属主窗口不能用 CW_USEDEFAULT（会落到属主位置附近），显式按屏幕居中。
        let (center_x, center_y) = win::center_on_cursor_monitor(outer_w, outer_h);
        let hwnd = CreateWindowExW(
            windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
            windows::core::PCWSTR(class.as_ptr()),
            windows::core::PCWSTR(title.as_ptr()),
            WINDOW_STYLE(style),
            center_x,
            center_y,
            outer_w,
            outer_h,
            Some(owner),
            None,
            Some(win::hinstance().into()),
            None,
        );
        let Ok(hwnd) = hwnd else {
            return None;
        };
        win::round_corners(hwnd);

        let mut fonts = FontCache::new(dpi);
        let ui_font = fonts.gdi_font(&super::gfx::FontSpec::ui(9.0)).unwrap_or_default();

        let state = Box::new(EditState {
            hwnd,
            dpi,
            fonts,
            existing: existing.clone(),
            fingerprint_value: normalize_fingerprint(
                existing.as_ref().and_then(|s| s.host_key_fingerprint_sha256.as_deref()),
            ),
            loading: true,
            testing: Arc::new(Mutex::new(false)),
            test_rx: None,
            result: None,
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);

        build_controls(hwnd, dpi, ui_font, existing.as_ref());
        load_values(hwnd);

        // 对应 C# ShowDialog：必须显式显示，否则模态循环等一个看不见的窗口。
        ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));

        dialog::run_modal(hwnd, &[owner]);

        let result = take_result(hwnd);
        RUNTIME.with(|cell| {
            *cell.borrow_mut() = None;
        });
        result
    }
}

thread_local! {
    static RUNTIME: std::cell::RefCell<Option<tokio::runtime::Handle>> = const { std::cell::RefCell::new(None) };
}

unsafe fn state_of(hwnd: HWND) -> *mut EditState {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut EditState
}

unsafe fn take_result(hwnd: HWND) -> Option<ServerConfig> {
    let ptr = state_of(hwnd);
    if ptr.is_null() {
        return None;
    }
    let result = (*ptr).result.clone();
    drop(Box::from_raw(ptr));
    result
}

unsafe fn dlg(hwnd: HWND, id: i32) -> HWND {
    windows::Win32::UI::WindowsAndMessaging::GetDlgItem(Some(hwnd), id).unwrap_or_default()
}

unsafe fn build_controls(hwnd: HWND, dpi: u32, ui_font: windows::Win32::Graphics::Gdi::HFONT, existing: Option<&ServerConfig>) {
    macro_rules! label {
        ($text:expr, $x:expr, $y:expr) => {{
            let hwnd_label = win::create_control("STATIC", $text, WS_CHILD.0 | WS_VISIBLE.0, 0, win::s($x, dpi), win::s($y, dpi), win::measure_text_width(ui_font, $text) + win::s(8, dpi), win::s(20, dpi), hwnd, 0);
            SendMessageW(hwnd_label, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(ui_font.0 as usize)), Some(LPARAM(1)));
            windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(hwnd_label, GWLP_USERDATA, theme::SECONDARY_TEXT as isize);
            hwnd_label
        }};
    }

    // 标题/副标题
    let t = super::i18n::t();
    {
        let title_text = if existing.is_none() { t.se_add_title } else { t.se_edit_title };
        let mut bold_fonts = FontCache::new(dpi);
        let bold = bold_fonts.gdi_font(&super::gfx::FontSpec::ui_bold(15.0)).unwrap_or_default();
        let hwnd_label = win::create_control("STATIC", title_text, WS_CHILD.0 | WS_VISIBLE.0, 0, win::s(24, dpi), win::s(16, dpi), win::measure_text_width(bold, title_text) + win::s(8, dpi), win::s(28, dpi), hwnd, 0);
        SendMessageW(hwnd_label, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(bold.0 as usize)), Some(LPARAM(1)));
        windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(hwnd_label, GWLP_USERDATA, theme::TEXT as isize);

        let sub_text = t.se_subtitle;
        let mut small = FontCache::new(dpi);
        let small_font = small.gdi_font(&super::gfx::FontSpec::ui(8.25)).unwrap_or_default();
        let sub = win::create_control("STATIC", sub_text, WS_CHILD.0 | WS_VISIBLE.0, 0, win::s(24, dpi), win::s(48, dpi), win::measure_text_width(small_font, sub_text) + win::s(8, dpi), win::s(18, dpi), hwnd, 0);
        SendMessageW(sub, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(small_font.0 as usize)), Some(LPARAM(1)));
        windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(sub, GWLP_USERDATA, theme::MUTED_TEXT as isize);
    }

    label!(t.se_name, 24, 86);
    macro_rules! edit_box {
        ($id:expr, $x:expr, $y:expr, $w:expr) => {{
            let hwnd_edit = win::create_control("EDIT", "", WS_CHILD.0 | WS_VISIBLE.0 | 0x0080_0000 /* WS_BORDER */, 0, win::s($x, dpi), win::s($y, dpi), win::s($w, dpi), win::s(26, dpi), hwnd, $id);
            SendMessageW(hwnd_edit, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(ui_font.0 as usize)), Some(LPARAM(1)));
            hwnd_edit
        }};
    }

    let _ = edit_box!(IDC_NAME, 120, 82, 456);
    // 占位符（EM_SETCUEBANNER）
    let cue = win::to_wide(t.se_name_cue);
    SendMessageW(dlg(hwnd, IDC_NAME), 0x1501 /* EM_SETCUEBANNER */,Some(WPARAM(0)), Some(LPARAM(cue.as_ptr() as isize)));

    label!(t.se_host, 24, 122);
    let _ = edit_box!(IDC_HOST, 120, 118, 456);

    label!(t.se_port, 24, 158);
    let port_edit = win::create_control("EDIT", "22", WS_CHILD.0 | WS_VISIBLE.0 | 0x0080_0000 /* WS_BORDER */ | 0x2000 /* ES_NUMBER */, 0, win::s(120, dpi), win::s(154, dpi), win::s(80, dpi), win::s(26, dpi), hwnd, IDC_PORT_EDIT);
    SendMessageW(port_edit, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(ui_font.0 as usize)), Some(LPARAM(1)));
    let updown_class = win::to_wide("msctls_updown32");
    let created = CreateWindowExW(
        windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
        windows::core::PCWSTR(updown_class.as_ptr()),
        None,
        windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(
            WS_CHILD.0 | WS_VISIBLE.0 | 0x0002 | 0x0004 | 0x0008 | 0x0100,
        ),
        0,
        0,
        0,
        0,
        Some(hwnd),
        Some(windows::Win32::UI::WindowsAndMessaging::HMENU(IDC_PORT_UPDOWN as *mut _)),
        Some(win::hinstance().into()),
        None,
    );
    let Ok(updown) = created else {
        return;
    };
    let _ = SendMessageW(updown, windows::Win32::UI::Controls::UDM_SETRANGE32,Some(WPARAM(1)), Some(LPARAM(65535)));
    dialog::updown_set_value(updown, 22);

    label!(t.se_username, 216, 158);
    let _ = edit_box!(IDC_USERNAME, 288, 154, 288);

    label!(t.se_auth, 24, 194);
    let combo = win::create_control(
        "COMBOBOX",
        "",
        WS_CHILD.0 | WS_VISIBLE.0 | 0x0002 /* CBS_DROPDOWNLIST */ | 0x0020_0000 /* WS_VSCROLL */,
        0,
        win::s(120, dpi),
        win::s(190, dpi),
        win::s(160, dpi),
        win::s(200, dpi),
        hwnd,
        IDC_AUTH_TYPE,
    );
    SendMessageW(combo, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(ui_font.0 as usize)), Some(LPARAM(1)));
    for item in [t.se_auth_password, t.se_auth_key_file] {
        let wide = win::to_wide(item);
        SendMessageW(combo, windows::Win32::UI::WindowsAndMessaging::CB_ADDSTRING,Some(WPARAM(0)), Some(LPARAM(wide.as_ptr() as isize)));
    }

    label!(t.se_auth_password, 24, 234);
    let password = win::create_control("EDIT", "", WS_CHILD.0 | WS_VISIBLE.0 | 0x0080_0000 /* WS_BORDER */ | 0x0020 /* ES_PASSWORD */, 0, win::s(120, dpi), win::s(230, dpi), win::s(340, dpi), win::s(26, dpi), hwnd, IDC_PASSWORD);
    SendMessageW(password, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(ui_font.0 as usize)), Some(LPARAM(1)));
    let show_password = win::create_control("BUTTON", t.btn_show, WS_CHILD.0 | WS_VISIBLE.0 | 0x0000_0003, 0, win::s(470, dpi), win::s(233, dpi), win::s(60, dpi), win::s(20, dpi), hwnd, IDC_SHOW_PASSWORD);
    SendMessageW(show_password, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(ui_font.0 as usize)), Some(LPARAM(1)));

    label!(t.se_auth_key_file, 24, 234);
    let _ = edit_box!(IDC_KEY_PATH, 120, 230, 300);
    let _ = dialog::create_flat_button(hwnd, &FlatButtonSpec { text: t.btn_browse, x: 428, y: 230, w: 70, h: 26, style: FlatButtonStyle::Normal }, IDC_BROWSE, dpi);

    label!(t.se_key_passphrase, 24, 270);
    let passphrase = win::create_control("EDIT", "", WS_CHILD.0 | WS_VISIBLE.0 | 0x0080_0000 /* WS_BORDER */ | 0x0020 /* ES_PASSWORD */, 0, win::s(120, dpi), win::s(266, dpi), win::s(340, dpi), win::s(26, dpi), hwnd, IDC_PASSPHRASE);
    SendMessageW(passphrase, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(ui_font.0 as usize)), Some(LPARAM(1)));
    let show_passphrase = win::create_control("BUTTON", t.btn_show, WS_CHILD.0 | WS_VISIBLE.0 | 0x0000_0003, 0, win::s(470, dpi), win::s(269, dpi), win::s(60, dpi), win::s(20, dpi), hwnd, IDC_SHOW_PASSPHRASE);
    SendMessageW(show_passphrase, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(ui_font.0 as usize)), Some(LPARAM(1)));

    // 552：英文文案（"…all current-user processes have exited"）在 440 宽内被截断。
    let notify = win::create_control("BUTTON", t.se_notify_idle, WS_CHILD.0 | WS_VISIBLE.0 | 0x0000_0003, 0, win::s(24, dpi), win::s(310, dpi), win::s(552, dpi), win::s(22, dpi), hwnd, IDC_NOTIFY);
    SendMessageW(notify, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(ui_font.0 as usize)), Some(LPARAM(1)));
    SendMessageW(notify, windows::Win32::UI::WindowsAndMessaging::BM_SETCHECK,Some(WPARAM(1)), Some(LPARAM(0)));

    label!(t.se_host_key, 24, 346);
    let fingerprint = win::create_control("EDIT", t.se_fingerprint_placeholder, WS_CHILD.0 | WS_VISIBLE.0 | 0x0080_0000 /* WS_BORDER */ | 0x0800 /* ES_READONLY */, 0, win::s(120, dpi), win::s(342, dpi), win::s(300, dpi), win::s(26, dpi), hwnd, IDC_FINGERPRINT);
    {
        let mut mono = FontCache::new(dpi);
        let mono_font = mono.gdi_font(&super::gfx::FontSpec::mono(8.25)).unwrap_or_default();
        SendMessageW(fingerprint, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(mono_font.0 as usize)), Some(LPARAM(1)));
    }
    let _ = dialog::create_flat_button(hwnd, &FlatButtonSpec { text: t.btn_retrust, x: 428, y: 342, w: 148, h: 26, style: FlatButtonStyle::Normal }, IDC_CLEAR_FINGERPRINT, dpi);

    let _ = dialog::create_flat_button(hwnd, &FlatButtonSpec { text: t.btn_test_connection, x: 120, y: 382, w: 110, h: 32, style: FlatButtonStyle::Normal }, IDC_TEST, dpi);
    let test_result = win::create_control("STATIC", "", WS_CHILD.0 | WS_VISIBLE.0, 0, win::s(242, dpi), win::s(378, dpi), win::s(334, dpi), win::s(48, dpi), hwnd, IDC_TEST_RESULT);
    {
        let mut small = FontCache::new(dpi);
        let small_font = small.gdi_font(&super::gfx::FontSpec::ui(8.25)).unwrap_or_default();
        SendMessageW(test_result, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(small_font.0 as usize)), Some(LPARAM(1)));
    }

    let error_label = win::create_control("STATIC", "", WS_CHILD.0 | WS_VISIBLE.0, 0, win::s(24, dpi), win::s(430, dpi), win::s(552, dpi), win::s(20, dpi), hwnd, IDC_ERROR);
    {
        let mut small = FontCache::new(dpi);
        let small_font = small.gdi_font(&super::gfx::FontSpec::ui(8.25)).unwrap_or_default();
        SendMessageW(error_label, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(small_font.0 as usize)), Some(LPARAM(1)));
    }

    let note = win::create_control("STATIC", t.se_note, WS_CHILD.0 | WS_VISIBLE.0, 0, win::s(24, dpi), win::s(456, dpi), win::s(552, dpi), win::s(34, dpi), hwnd, 0);
    {
        let mut small = FontCache::new(dpi);
        let small_font = small.gdi_font(&super::gfx::FontSpec::ui(8.25)).unwrap_or_default();
        SendMessageW(note, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(small_font.0 as usize)), Some(LPARAM(1)));
    }

    let _ = dialog::create_flat_button(hwnd, &FlatButtonSpec { text: t.btn_save, x: 480, y: 536, w: 96, h: 34, style: FlatButtonStyle::Primary }, IDC_SAVE, dpi);
    let _ = dialog::create_flat_button(hwnd, &FlatButtonSpec { text: t.btn_cancel, x: 384, y: 536, w: 86, h: 34, style: FlatButtonStyle::Normal }, IDC_CANCEL, dpi);
}

unsafe fn load_values(hwnd: HWND) {
    let state = state_of(hwnd);
    if state.is_null() {
        return;
    }
    let existing = (*state).existing.clone();

    if let Some(existing) = &existing {
        set_text(dlg(hwnd, IDC_NAME), existing.name.as_deref().unwrap_or(""));
        set_text(dlg(hwnd, IDC_HOST), &existing.host);
        dialog::updown_set_value(dlg(hwnd, IDC_PORT_UPDOWN), existing.port.clamp(1, 65535));
        set_text(dlg(hwnd, IDC_USERNAME), &existing.username);
        SendMessageW(dlg(hwnd, IDC_AUTH_TYPE),
            windows::Win32::UI::WindowsAndMessaging::CB_SETCURSEL,Some(WPARAM(if existing.auth_type == ServerAuthType::PrivateKey { 1 } else { 0 })), Some(LPARAM(0)));
        set_text(dlg(hwnd, IDC_KEY_PATH), existing.private_key_path.as_deref().unwrap_or(""));
        SendMessageW(dlg(hwnd, IDC_NOTIFY),
            windows::Win32::UI::WindowsAndMessaging::BM_SETCHECK,Some(WPARAM(existing.notifications_enabled as usize)), Some(LPARAM(0)));

        let mut error_text = String::new();
        if existing.protected_password.as_deref().is_some_and(|p| !p.is_empty()) {
            match secrets::try_unprotect(existing.protected_password.as_deref()) {
                Some(password) => set_text(dlg(hwnd, IDC_PASSWORD), &password),
                None => error_text = super::i18n::t().se_err_saved_password.to_string(),
            }
        }
        if existing.protected_key_passphrase.as_deref().is_some_and(|p| !p.is_empty()) {
            match secrets::try_unprotect(existing.protected_key_passphrase.as_deref()) {
                Some(passphrase) => set_text(dlg(hwnd, IDC_PASSPHRASE), &passphrase),
                None => {
                    if error_text.is_empty() {
                        error_text = super::i18n::t().se_err_saved_passphrase.to_string();
                    }
                }
            }
        }
        if !error_text.is_empty() {
            set_text(dlg(hwnd, IDC_ERROR), &error_text);
        }
    } else {
        SendMessageW(dlg(hwnd, IDC_AUTH_TYPE),
            windows::Win32::UI::WindowsAndMessaging::CB_SETCURSEL,Some(WPARAM(0)), Some(LPARAM(0)));
    }

    update_fingerprint_display(hwnd);
    update_auth_visibility(hwnd);
    (*state).loading = false;
}

unsafe fn set_text(hwnd: HWND, text: &str) {
    dialog::set_window_text(hwnd, text);
}

unsafe fn get_text(hwnd: HWND) -> String {
    dialog::get_window_text(hwnd)
}

unsafe fn update_auth_visibility(hwnd: HWND) {
    let selected = SendMessageW(
        dlg(hwnd, IDC_AUTH_TYPE),
        windows::Win32::UI::WindowsAndMessaging::CB_GETCURSEL,
        Some(WPARAM(0)), Some(LPARAM(0)),
    )
    .0;
    let use_key = selected == 1;

    for (id, show) in [
        (IDC_PASSWORD, !use_key),
        (IDC_SHOW_PASSWORD, !use_key),
        (IDC_KEY_PATH, use_key),
        (IDC_BROWSE, use_key),
        (IDC_PASSPHRASE, use_key),
        (IDC_SHOW_PASSPHRASE, use_key),
    ] {
        let hwnd_control = dlg(hwnd, id);
        if !hwnd_control.is_invalid() {
            let _ = ShowWindow(hwnd_control, if show { SW_SHOW } else { SW_HIDE });
        }
    }
    // 标签是 STATIC，按创建顺序定位：密码/私钥文件共用 24,234；私钥口令在 24,270。
    // 它们没有 ID —— 在创建时保存；这里通过窗口枚举太复杂，改为对每组用可见性随认证方式切换。
    // 解决：密码/私钥文件标签二选一显示 —— 按文本查找（创建与查找都走翻译表，必然一致）。
    let t = super::i18n::t();
    toggle_label_by_text(hwnd, t.se_auth_password, !use_key);
    toggle_label_by_text(hwnd, t.se_auth_key_file, use_key);
    toggle_label_by_text(hwnd, t.se_key_passphrase, use_key);
}

unsafe fn toggle_label_by_text(hwnd_parent: HWND, text: &str, show: bool) {
    let mut child = windows::Win32::UI::WindowsAndMessaging::GetWindow(
        hwnd_parent,
        windows::Win32::UI::WindowsAndMessaging::GW_CHILD,
    )
    .ok();
    while let Some(current) = child {
        if get_window_class(current) == "Static" && get_text(current) == text {
            let _ = ShowWindow(current, if show { SW_SHOW } else { SW_HIDE });
        }
        child = windows::Win32::UI::WindowsAndMessaging::GetWindow(
            current,
            windows::Win32::UI::WindowsAndMessaging::GW_HWNDNEXT,
        )
        .ok();
    }
}

unsafe fn get_window_class(hwnd: HWND) -> String {
    let mut buffer = [0u16; 64];
    let copied = windows::Win32::UI::WindowsAndMessaging::GetClassNameW(hwnd, &mut buffer);
    win::from_wide(&buffer[..copied.max(0) as usize])
}

unsafe fn update_fingerprint_display(hwnd: HWND) {
    let state = state_of(hwnd);
    if state.is_null() {
        return;
    }
    let text = match &(*state).fingerprint_value {
        Some(value) if !value.trim().is_empty() => format!("SHA256:{value}"),
        _ => super::i18n::t().se_fingerprint_placeholder.to_string(),
    };
    set_text(dlg(hwnd, IDC_FINGERPRINT), &text);
    let clear = dlg(hwnd, IDC_CLEAR_FINGERPRINT);
    let has = (*state)
        .fingerprint_value
        .as_deref()
        .is_some_and(|v| !v.trim().is_empty());
    let _ = EnableWindow(clear, has);
}

fn normalize_fingerprint(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value
        .strip_prefix("SHA256:")
        .or_else(|| value.strip_prefix("sha256:"))
    {
        Some(rest.trim().to_string())
    } else {
        Some(value.to_string())
    }
}

unsafe fn invalidate_fingerprint_on_target_change(hwnd: HWND) {
    let state = state_of(hwnd);
    if state.is_null() || (*state).loading {
        return;
    }
    if (*state).fingerprint_value.is_none() {
        return;
    }
    (*state).fingerprint_value = None;
    update_fingerprint_display(hwnd);
}

unsafe fn try_build_config(hwnd: HWND) -> Option<ServerConfig> {
    let state = state_of(hwnd);
    if state.is_null() {
        return None;
    }
    set_text(dlg(hwnd, IDC_ERROR), "");

    let host = get_text(dlg(hwnd, IDC_HOST)).trim().to_string();
    if host.is_empty() {
        set_text(dlg(hwnd, IDC_ERROR), super::i18n::t().se_err_host_required);
        return None;
    }

    let username = get_text(dlg(hwnd, IDC_USERNAME)).trim().to_string();
    if username.is_empty() {
        set_text(dlg(hwnd, IDC_ERROR), super::i18n::t().se_err_username_required);
        return None;
    }

    let auth_selected = SendMessageW(
        dlg(hwnd, IDC_AUTH_TYPE),
        windows::Win32::UI::WindowsAndMessaging::CB_GETCURSEL,
        Some(WPARAM(0)), Some(LPARAM(0)),
    )
    .0;
    let use_key = auth_selected == 1;
    let key_path = get_text(dlg(hwnd, IDC_KEY_PATH)).trim().to_string();
    if use_key {
        if key_path.is_empty() {
            set_text(dlg(hwnd, IDC_ERROR), super::i18n::t().se_err_key_required);
            return None;
        }
        if !std::path::Path::new(&key_path).is_file() {
            set_text(dlg(hwnd, IDC_ERROR), super::i18n::t().se_err_key_missing);
            return None;
        }
        if key_path.to_lowercase().ends_with(".ppk") {
            set_text(dlg(hwnd, IDC_ERROR), super::i18n::t().se_err_ppk);
            return None;
        }
    } else if get_text(dlg(hwnd, IDC_PASSWORD)).is_empty() {
        set_text(dlg(hwnd, IDC_ERROR), super::i18n::t().se_err_password_required);
        return None;
    }

    let mut config = (*state)
        .existing
        .clone()
        .unwrap_or_else(ServerConfig::default);
    let name_text = get_text(dlg(hwnd, IDC_NAME));
    config.name = if name_text.trim().is_empty() {
        None
    } else {
        Some(name_text.trim().to_string())
    };
    config.host = host;
    config.port = dialog::updown_get_value(dlg(hwnd, IDC_PORT_UPDOWN));
    config.username = username;
    config.auth_type = if use_key {
        ServerAuthType::PrivateKey
    } else {
        ServerAuthType::Password
    };
    let password = get_text(dlg(hwnd, IDC_PASSWORD));
    config.protected_password = secrets::protect(&password);
    config.private_key_path = if key_path.is_empty() { None } else { Some(key_path) };
    let passphrase = get_text(dlg(hwnd, IDC_PASSPHRASE));
    config.protected_key_passphrase = secrets::protect(&passphrase);
    config.notifications_enabled = SendMessageW(
        dlg(hwnd, IDC_NOTIFY),
        windows::Win32::UI::WindowsAndMessaging::BM_GETCHECK,
        Some(WPARAM(0)), Some(LPARAM(0)),
    )
    .0 != 0;
    config.host_key_fingerprint_sha256 = (*state).fingerprint_value.clone();
    Some(config)
}

unsafe fn start_test(hwnd: HWND) {
    let state = state_of(hwnd);
    if state.is_null() {
        return;
    }
    if *(*state).testing.lock().unwrap() {
        return;
    }
    let Some(config) = try_build_config(hwnd) else {
        return;
    };

    *(*state).testing.lock().unwrap() = true;
    let _ = EnableWindow(dlg(hwnd, IDC_TEST), false);
    let _ = EnableWindow(dlg(hwnd, IDC_SAVE), false);
    set_text(dlg(hwnd, IDC_TEST_RESULT), super::i18n::t().se_testing);

    let (tx, rx) = std::sync::mpsc::channel::<ssh::ConnectionTestResult>();
    (*state).test_rx = Some(rx);

    RUNTIME.with(|cell| {
        if let Some(handle) = cell.borrow().as_ref() {
            handle.spawn(async move {
                let result = ssh::test_connection(&config).await;
                let _ = tx.send(result);
            });
        }
    });

    SetTimer(Some(hwnd), TIMER_TEST_POLL, 100, None);
}

unsafe fn poll_test(hwnd: HWND) {
    let state = state_of(hwnd);
    if state.is_null() {
        return;
    }
    let result = match (*state).test_rx.as_ref() {
        Some(rx) => match rx.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(hwnd), TIMER_TEST_POLL);
                None
            }
        },
        None => {
            windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(hwnd), TIMER_TEST_POLL);
            None
        }
    };

    let Some(result) = result else {
        return;
    };
    windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(hwnd), TIMER_TEST_POLL);

    if result.fingerprint.as_deref().is_some_and(|f| !f.trim().is_empty()) {
        (*state).fingerprint_value = normalize_fingerprint(result.fingerprint.as_deref());
        update_fingerprint_display(hwnd);
    }

    let t = super::i18n::t();
    // 成功消息用结构化字段现拼（后端拼的中文消息不参与）；其余经 localize_backend 映射。
    let mut text = if result.connected && result.error.is_none() {
        let n_text = result.gpu_count.to_string();
        let ram_text = if result.ram_available { t.se_test_ram_ok } else { "" };
        super::i18n::fmt(t.se_test_success, &[("n", &n_text), ("ram", ram_text)])
    } else {
        super::i18n::localize_backend(&result.message)
    };
    let is_error_with_hostkey = result
        .error
        .as_ref()
        .is_some_and(|e| e.kind == crate::monitor::snapshot::MonitorErrorKind::HostKey
            && e.detail.as_deref().is_some_and(|d| !d.trim().is_empty()));
    if is_error_with_hostkey {
        let detail = result.error.as_ref().and_then(|e| e.detail.clone()).unwrap_or_default();
        text = format!("{text}\r\n{}", super::i18n::fmt(t.se_test_new_fingerprint, &[("fp", &detail)]));
    } else if result.error.is_some() && result.connected {
        text.push_str(t.se_test_retry_hint);
    }
    set_text(dlg(hwnd, IDC_TEST_RESULT), &text);

    *(*state).testing.lock().unwrap() = false;
    let _ = EnableWindow(dlg(hwnd, IDC_TEST), true);
    let _ = EnableWindow(dlg(hwnd, IDC_SAVE), true);
    let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, true);
}

pub unsafe extern "system" fn server_edit_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_DRAWITEM => {
            let draw_item = lparam.0 as *mut DRAWITEMSTRUCT;
            if !draw_item.is_null() && (*draw_item).CtlType.0 == 4 {
                let state = state_of(hwnd);
                if !state.is_null() {
                    dialog::draw_flat_button(draw_item, &mut (*state).fonts);
                }
                return LRESULT(1);
            }
        }
        WM_ERASEBKGND => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut _);
            let mut rect = windows::Win32::Foundation::RECT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rect);
            windows::Win32::Graphics::Gdi::FillRect(hdc, &rect, super::settings::background_brush());
            return LRESULT(1);
        }
        WM_CTLCOLORSTATIC => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut _);
            let ctl = windows::Win32::Foundation::HWND(lparam.0 as *mut _);
            let color = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(ctl, GWLP_USERDATA) as u32;
            let argb = if color == 0 { theme::TEXT } else { color };
            let _ = windows::Win32::Graphics::Gdi::SetTextColor(hdc, theme::argb_to_colorref(argb));
            let _ = windows::Win32::Graphics::Gdi::SetBkColor(hdc, theme::argb_to_colorref(theme::WINDOW_BACKGROUND));
            return LRESULT(super::settings::background_brush().0 as isize);
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as i32;
            let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
            match id {
                IDC_SHOW_PASSWORD if code == 0 => {
                    let checked = SendMessageW(
                        dlg(hwnd, IDC_SHOW_PASSWORD),
                        windows::Win32::UI::WindowsAndMessaging::BM_GETCHECK,
                        Some(WPARAM(0)), Some(LPARAM(0)),
                    )
                    .0 != 0;
                    SendMessageW(dlg(hwnd, IDC_PASSWORD),
                        EM_SETPASSWORDCHAR,Some(WPARAM(if checked { 0 } else { 0x25CF })), Some(LPARAM(0)));
                    let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(dlg(hwnd, IDC_PASSWORD)), None, true);
                    return LRESULT(0);
                }
                IDC_SHOW_PASSPHRASE if code == 0 => {
                    let checked = SendMessageW(
                        dlg(hwnd, IDC_SHOW_PASSPHRASE),
                        windows::Win32::UI::WindowsAndMessaging::BM_GETCHECK,
                        Some(WPARAM(0)), Some(LPARAM(0)),
                    )
                    .0 != 0;
                    SendMessageW(dlg(hwnd, IDC_PASSPHRASE),
                        EM_SETPASSWORDCHAR,Some(WPARAM(if checked { 0 } else { 0x25CF })), Some(LPARAM(0)));
                    let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(dlg(hwnd, IDC_PASSPHRASE)), None, true);
                    return LRESULT(0);
                }
                IDC_AUTH_TYPE if code == 1 /* CBN_SELCHANGE */ => {
                    update_auth_visibility(hwnd);
                    return LRESULT(0);
                }
                IDC_BROWSE if code == 0 => {
                    let t = super::i18n::t();
                    let filter = format!(
                        "{}|*.pem;*.key;id_rsa*;id_ed25519*;*.openssh;*.ppk|{}|*.*",
                        t.se_filter_keys,
                        t.se_filter_all
                    );
                    if let Some(path) = dialog::open_file_dialog(hwnd, t.se_pick_key_title, &filter) {
                        set_text(dlg(hwnd, IDC_KEY_PATH), &path);
                    }
                    return LRESULT(0);
                }
                IDC_CLEAR_FINGERPRINT if code == 0 => {
                    let t = super::i18n::t();
                    let answer = dialog::message_box(
                        hwnd,
                        t.msg_retrust_body,
                        t.msg_retrust_title,
                        dialog::MessageBoxKind::Warning,
                        dialog::MessageBoxButtons::YesNoCancelDefault2,
                    );
                    if matches!(answer, dialog::MessageBoxResult::Yes) {
                        let state = state_of(hwnd);
                        if !state.is_null() {
                            (*state).fingerprint_value = None;
                            update_fingerprint_display(hwnd);
                        }
                    }
                    return LRESULT(0);
                }
                IDC_TEST if code == 0 => {
                    start_test(hwnd);
                    return LRESULT(0);
                }
                IDC_SAVE if code == 0 => {
                    let state = state_of(hwnd);
                    let testing = !state.is_null() && *(*state).testing.lock().unwrap();
                    if !testing {
                        if let Some(config) = try_build_config(hwnd) {
                            if !state.is_null() {
                                (*state).result = Some(config);
                            }
                            DestroyWindow(hwnd);
                        }
                    }
                    return LRESULT(0);
                }
                IDC_CANCEL if code == 0 => {
                    DestroyWindow(hwnd);
                    return LRESULT(0);
                }
                IDC_HOST if code == 0x0300 /* EN_CHANGE */ => {
                    invalidate_fingerprint_on_target_change(hwnd);
                }
                IDC_PORT_EDIT if code == 0x0300 => {
                    invalidate_fingerprint_on_target_change(hwnd);
                }
                _ => {}
            }
        }
        WM_TIMER => {
            if wparam.0 as usize == TIMER_TEST_POLL {
                poll_test(hwnd);
                return LRESULT(0);
            }
        }
        WM_KEYDOWN => {
            if wparam.0 as u32 == 0x1B {
                DestroyWindow(hwnd);
                return LRESULT(1);
            }
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}
