//! 设置窗口：对应 C# SettingsForm。ClientSize 680×580，FixedDialog。

use super::dialog::{self, FlatButtonSpec, FlatButtonStyle};
use super::gfx::FontCache;
use super::theme;
use super::win;
use crate::config::{AppConfig, ServerConfig};
use std::cell::RefCell;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_SELECTED};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetDlgItem, GetWindowLongPtrW,
    SendMessageW, SetForegroundWindow, SetTimer, SetWindowLongPtrW, ShowWindow, CW_USEDEFAULT,
    GWLP_USERDATA, SW_SHOW, WINDOW_STYLE, WM_COMMAND, WM_CTLCOLORSTATIC, WM_DRAWITEM,
    WM_ERASEBKGND, WM_KEYDOWN, WM_MEASUREITEM, WM_PAINT, WM_TIMER, WS_CHILD, WS_VISIBLE,
};

// 界面语言是 Rust 版新增选项（有意偏离 C# 版 680×580）：
// "常规"组内多一行"语言"下拉框，底部说明与保存/取消下移，ClientSize 变为 680×614。
const CLIENT_W: i32 = 680;
const CLIENT_H: i32 = 614;

// 控件 ID
const IDC_SERVER_LIST: i32 = 100;
const IDC_ADD: i32 = 101;
const IDC_EDIT: i32 = 102;
const IDC_DELETE: i32 = 103;
const IDC_TEST_NOTIFY: i32 = 104;
const IDC_TEST_HINT: i32 = 111;
const IDC_SAVE: i32 = 105;
const IDC_CANCEL: i32 = 106;
const IDC_REFRESH_EDIT: i32 = 107;
const IDC_REFRESH_UPDOWN: i32 = 108;
const IDC_NOTIFICATIONS: i32 = 109;
const IDC_AUTOSTART: i32 = 110;
const IDC_LANGUAGE: i32 = 112;

struct SettingsState {
    hwnd: HWND,
    dpi: u32,
    fonts: FontCache,
    servers: Vec<ServerConfig>,
    refresh: i32,
    notifications: bool,
    autostart: bool,
    result: Option<AppConfig>,
    base: AppConfig,
    ui_font: windows::Win32::Graphics::Gdi::HFONT,
}

thread_local! {
    static TEST_ACTION: RefCell<Option<Box<dyn FnMut() + 'static>>> = RefCell::new(None);
}

/// 弹出设置窗口（模态）。返回保存后的配置。
pub fn show_dialog(
    owner: HWND,
    config: &AppConfig,
    _runtime: &tokio::runtime::Handle,
    test_notification: impl FnMut() + 'static,
) -> Option<AppConfig> {
    TEST_ACTION.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(test_notification));
    });

    unsafe {
        let dpi = super::gfx::screen_dpi();
        // WS_CLIPCHILDREN：父窗口自绘 WM_PAINT 时不得覆盖子控件区域，
        // 否则 owner-draw 子控件（按钮/列表行）收不到绘制消息。
        let style = (windows::Win32::UI::WindowsAndMessaging::WS_OVERLAPPEDWINDOW.0
            | windows::Win32::UI::WindowsAndMessaging::WS_CLIPCHILDREN.0)
            & !(windows::Win32::UI::WindowsAndMessaging::WS_MAXIMIZEBOX.0
                | windows::Win32::UI::WindowsAndMessaging::WS_THICKFRAME.0);
        let (outer_w, outer_h) = win::client_to_outer(
            win::s(CLIENT_W, dpi),
            win::s(CLIENT_H, dpi),
            style,
            0,
            dpi,
        );

        let class = win::to_wide(super::app::SETTINGS_WINDOW_CLASS);
        let title = win::to_wide(super::i18n::t().settings_window_title);
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
        let hwnd = match hwnd {
            Ok(hwnd) => hwnd,
            Err(err) => {
                crate::util::error(
                    &format!(
                        "设置窗口创建失败：hr=0x{:08X}（{}），GetLastError={}。",
                        err.code().0,
                        err,
                        windows::Win32::Foundation::GetLastError().0
                    ),
                    None,
                );
                TEST_ACTION.with(|cell| {
                    *cell.borrow_mut() = None;
                });
                return None;
            }
        };

        let mut fonts = FontCache::new(dpi);
        let ui_font = fonts.gdi_font(&super::gfx::FontSpec::ui(9.0)).unwrap_or_default();
        win::round_corners(hwnd);

        let state = Box::new(SettingsState {
            hwnd,
            dpi,
            fonts,
            servers: config.servers.clone(),
            refresh: config.refresh_seconds.clamp(2, 300),
            notifications: config.notifications_enabled,
            autostart: config.start_with_windows,
            result: None,
            base: config.clone(),
            ui_font,
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);

        build_controls(hwnd, dpi, ui_font, config);
        // 对应 C# ShowDialog：必须显式显示——无 WS_VISIBLE 时窗口不可见，
        // 模态循环会等一个看不见的窗口，表现为"程序消失"。
        ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));

        dialog::run_modal(hwnd, &[owner]);

        let result = take_result(hwnd);
        TEST_ACTION.with(|cell| {
            *cell.borrow_mut() = None;
        });
        result
    }
}

unsafe fn EnableWindowAndRestore() -> bool {
    true
}

unsafe fn state_of(hwnd: HWND) -> *mut SettingsState {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsState
}

unsafe fn state_dpi(hwnd: HWND) -> u32 {
    let ptr = state_of(hwnd);
    if ptr.is_null() {
        96
    } else {
        (*ptr).fonts.dpi()
    }
}

unsafe fn take_result(hwnd: HWND) -> Option<AppConfig> {
    let ptr = state_of(hwnd);
    if ptr.is_null() {
        return None;
    }
    let result = (*ptr).result.clone();
    drop(Box::from_raw(ptr));
    result
}

/// 创建自动加宽的 STATIC 标签：宽度按文本实测（对应 C# AutoSize=true），
/// 颜色存入 GWLP_USERDATA 供 WM_CTLCOLORSTATIC 使用。
unsafe fn create_label(parent: HWND, text: &str, x: i32, y: i32, font: windows::Win32::Graphics::Gdi::HFONT, color: super::gfx::Argb, dpi: u32) -> HWND {
    let text_w = win::measure_text_width(font, text);
    let hwnd = win::create_control(
        "STATIC",
        text,
        WS_CHILD.0 | WS_VISIBLE.0,
        0,
        win::s(x, dpi),
        win::s(y, dpi),
        text_w + win::s(8, dpi),
        win::s(20, dpi),
        parent,
        0,
    );
    SendMessageW(hwnd, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
    windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(hwnd, GWLP_USERDATA, color as isize);
    hwnd
}

unsafe fn build_controls(hwnd: HWND, dpi: u32, ui_font: windows::Win32::Graphics::Gdi::HFONT, config: &AppConfig) {
    let label_font_small = {
        let mut fonts = FontCache::new(dpi);
        fonts.gdi_font(&super::gfx::FontSpec::ui(8.25)).unwrap_or_default()
    };

    let t = super::i18n::t();
    create_label(hwnd, t.settings_heading, 24, 16, ui_font, theme::TEXT, dpi);
    create_label(hwnd, t.settings_subtitle, 24, 48, label_font_small, theme::MUTED_TEXT, dpi);
    create_label(hwnd, t.settings_group_servers, 24, 82, ui_font, theme::TEXT, dpi);

    // 服务器列表：自绘固定行高（卡片内嵌，无边框）
    // WS_CHILD|WS_VISIBLE|WS_VSCROLL|LBS_NOTIFY|LBS_NOINTEGRALHEIGHT|LBS_HASSTRINGS|LBS_OWNERDRAWFIXED
    // （LBS_HASSTRINGS 必须有：否则 LB_ADDSTRING 的参数被当作 itemData 而非字符串）
    let list = win::create_control(
        "LISTBOX",
        "",
        WS_CHILD.0 | WS_VISIBLE.0 | 0x0020_0000 | 0x0001 | 0x0100 | 0x0040 | 0x0010,
        0,
        win::s(25, dpi),
        win::s(107, dpi),
        win::s(408, dpi),
        win::s(208, dpi),
        hwnd,
        IDC_SERVER_LIST,
    );
    SendMessageW(list, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(ui_font.0 as usize)), Some(LPARAM(1)));

    let _ = dialog::create_flat_button(hwnd, &FlatButtonSpec { text: t.btn_add_server, x: 450, y: 106, w: 150, h: 32, style: FlatButtonStyle::Normal }, IDC_ADD, dpi);
    let _ = dialog::create_flat_button(hwnd, &FlatButtonSpec { text: t.btn_edit, x: 450, y: 146, w: 150, h: 32, style: FlatButtonStyle::Normal }, IDC_EDIT, dpi);
    let _ = dialog::create_flat_button(hwnd, &FlatButtonSpec { text: t.btn_delete, x: 450, y: 186, w: 150, h: 32, style: FlatButtonStyle::Danger }, IDC_DELETE, dpi);
    let _ = dialog::create_flat_button(hwnd, &FlatButtonSpec { text: t.btn_send_test_notify, x: 450, y: 420, w: 150, h: 32, style: FlatButtonStyle::Normal }, IDC_TEST_NOTIFY, dpi);
    // 测试通知回显（对应 C# _testHint）
    {
        let hint = win::create_control("STATIC", "", WS_CHILD.0 | WS_VISIBLE.0, 0, win::s(450, dpi), win::s(456, dpi), win::s(200, dpi), win::s(34, dpi), hwnd, IDC_TEST_HINT);
        SendMessageW(hint, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(label_font_small.0 as usize)), Some(LPARAM(1)));
        windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(hint, GWLP_USERDATA, theme::MUTED_TEXT as isize);
    }

    create_label(hwnd, t.settings_hint, 24, 324, label_font_small, theme::MUTED_TEXT, dpi);
    create_label(hwnd, t.settings_group_general, 24, 356, ui_font, theme::TEXT, dpi);
    create_label(hwnd, t.settings_refresh, 24, 390, ui_font, theme::SECONDARY_TEXT, dpi);

    // 刷新间隔：EDIT + UpDown（WS_BORDER=0x00800000，ES_NUMBER=0x2000）
    let refresh_edit = win::create_control(
        "EDIT",
        &config.refresh_seconds.clamp(2, 300).to_string(),
        WS_CHILD.0 | WS_VISIBLE.0 | 0x0080_0000 | 0x2000,
        0,
        win::s(120, dpi),
        win::s(386, dpi),
        win::s(80, dpi),
        win::s(26, dpi),
        hwnd,
        IDC_REFRESH_EDIT,
    );
    SendMessageW(refresh_edit, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(ui_font.0 as usize)), Some(LPARAM(1)));

    let updown_class = win::to_wide("msctls_updown32");
    let created = CreateWindowExW(
        windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
        windows::core::PCWSTR(updown_class.as_ptr()),
        None,
        windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(
            WS_CHILD.0 | WS_VISIBLE.0 | 0x0001 /* UDS_WRAP */ | 0x0002 /* UDS_SETBUDDYINT */ | 0x0004 /* UDS_ALIGNRIGHT */ | 0x0008 /* UDS_AUTOBUDDY */ | 0x0100 /* UDS_ARROWKEYS */,
        ),
        0,
        0,
        0,
        0,
        Some(hwnd),
        Some(windows::Win32::UI::WindowsAndMessaging::HMENU(IDC_REFRESH_UPDOWN as *mut _)),
        Some(win::hinstance().into()),
        None,
    );
    let Ok(updown) = created else {
        return;
    };
    let _ = SendMessageW(updown,
        windows::Win32::UI::Controls::UDM_SETRANGE32,Some(WPARAM(2)), Some(LPARAM(300)));
    dialog::updown_set_value(updown, config.refresh_seconds.clamp(2, 300));

    create_label(hwnd, t.settings_unit_seconds, 206, 390, ui_font, theme::SECONDARY_TEXT, dpi);
    create_label(hwnd, t.settings_refresh_note, 238, 390, label_font_small, theme::MUTED_TEXT, dpi);

    let notifications = win::create_control(
        "BUTTON",
        t.settings_enable_notifications,
        WS_CHILD.0 | WS_VISIBLE.0 | 0x0000_0003 /* BS_AUTOCHECKBOX */,
        0,
        win::s(24, dpi),
        win::s(424, dpi),
        win::s(220, dpi),
        win::s(22, dpi),
        hwnd,
        IDC_NOTIFICATIONS,
    );
    SendMessageW(notifications, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(ui_font.0 as usize)), Some(LPARAM(1)));
    SendMessageW(notifications,
        windows::Win32::UI::WindowsAndMessaging::BM_SETCHECK,Some(WPARAM(config.notifications_enabled as usize)), Some(LPARAM(0)));

    // 320：英文文案（"Start GPU Observer when Windows starts"）在 220 宽内被截断。
    let autostart = win::create_control(
        "BUTTON",
        t.settings_autostart,
        WS_CHILD.0 | WS_VISIBLE.0 | 0x0000_0003,
        0,
        win::s(24, dpi),
        win::s(452, dpi),
        win::s(320, dpi),
        win::s(22, dpi),
        hwnd,
        IDC_AUTOSTART,
    );
    SendMessageW(autostart, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(ui_font.0 as usize)), Some(LPARAM(1)));
    SendMessageW(autostart,
        windows::Win32::UI::WindowsAndMessaging::BM_SETCHECK,Some(WPARAM(config.start_with_windows as usize)), Some(LPARAM(0)));

    // 语言下拉框（Rust 版新增；选项固定用各自母语显示）。
    create_label(hwnd, t.settings_language, 24, 484, ui_font, theme::SECONDARY_TEXT, dpi);
    let lang_combo = win::create_control(
        "COMBOBOX",
        "",
        WS_CHILD.0 | WS_VISIBLE.0 | 0x0002 /* CBS_DROPDOWNLIST */ | 0x0020_0000 /* WS_VSCROLL */,
        0,
        win::s(120, dpi),
        win::s(480, dpi),
        win::s(120, dpi),
        win::s(200, dpi),
        hwnd,
        IDC_LANGUAGE,
    );
    SendMessageW(lang_combo, windows::Win32::UI::WindowsAndMessaging::WM_SETFONT,Some(WPARAM(ui_font.0 as usize)), Some(LPARAM(1)));
    for item in [t.lang_zh, t.lang_en] {
        let wide = win::to_wide(item);
        SendMessageW(lang_combo, windows::Win32::UI::WindowsAndMessaging::CB_ADDSTRING,Some(WPARAM(0)), Some(LPARAM(wide.as_ptr() as isize)));
    }
    SendMessageW(lang_combo,
        windows::Win32::UI::WindowsAndMessaging::CB_SETCURSEL,Some(WPARAM(if config.language == crate::config::Language::En { 1 } else { 0 })), Some(LPARAM(0)));

    create_label(hwnd, t.settings_storage_note, 24, 516, label_font_small, theme::MUTED_TEXT, dpi);

    let _ = dialog::create_flat_button(hwnd, &FlatButtonSpec { text: t.btn_save, x: 570, y: 546, w: 86, h: 34, style: FlatButtonStyle::Primary }, IDC_SAVE, dpi);
    let _ = dialog::create_flat_button(hwnd, &FlatButtonSpec { text: t.btn_cancel, x: 474, y: 546, w: 86, h: 34, style: FlatButtonStyle::Normal }, IDC_CANCEL, dpi);

    refresh_server_list(hwnd, 0);
    update_button_states(hwnd);
}

unsafe fn list_hwnd(hwnd: HWND) -> HWND {
    GetDlgItem(Some(hwnd), IDC_SERVER_LIST).unwrap_or_default()
}

fn server_row_label(server: &ServerConfig) -> String {
    let label = if server.name.as_deref().is_some_and(|n| !n.trim().is_empty()) {
        format!("{} · {}", server.name.as_deref().unwrap_or(""), server.display_user_host())
    } else {
        server.display_user_host()
    };
    let suffix = if server.notifications_enabled {
        String::new()
    } else {
        format!("  ·  {}", super::i18n::t().settings_list_notify_off)
    };
    format!("{label}{suffix}")
}

unsafe fn refresh_server_list(hwnd: HWND, select_index: usize) {
    let state = state_of(hwnd);
    if state.is_null() {
        return;
    }
    let list = list_hwnd(hwnd);
    SendMessageW(list, windows::Win32::UI::WindowsAndMessaging::LB_RESETCONTENT,Some(WPARAM(0)), Some(LPARAM(0)));
    for server in &(*state).servers {
        let text = win::to_wide(&server_row_label(server));
        SendMessageW(list,
            windows::Win32::UI::WindowsAndMessaging::LB_ADDSTRING,Some(WPARAM(0)), Some(LPARAM(text.as_ptr() as isize)));
    }
    if !(*state).servers.is_empty() {
        let index = select_index.min((*state).servers.len() - 1);
        SendMessageW(list,
            windows::Win32::UI::WindowsAndMessaging::LB_SETCURSEL,Some(WPARAM(index)), Some(LPARAM(0)));
    }
    update_button_states(hwnd);
}

unsafe fn selected_index(hwnd: HWND) -> i32 {
    let list = list_hwnd(hwnd);
    SendMessageW(
        list,
        windows::Win32::UI::WindowsAndMessaging::LB_GETCURSEL,
        Some(WPARAM(0)), Some(LPARAM(0)),
    )
    .0 as i32
}

unsafe fn update_button_states(hwnd: HWND) {
    let has_selection = selected_index(hwnd) >= 0;
    let edit = GetDlgItem(Some(hwnd), IDC_EDIT).unwrap_or_default();
    let delete = GetDlgItem(Some(hwnd), IDC_DELETE).unwrap_or_default();
    let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(edit, has_selection);
    let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(delete, has_selection);
}

unsafe fn add_server(hwnd: HWND) {
    let server = super::server_edit::show_dialog(hwnd, None, runtime_handle());
    let Some(server) = server else {
        return;
    };
    let state = state_of(hwnd);
    if state.is_null() {
        return;
    }
    let select = (*state).servers.len();
    (*state).servers.push(server);
    refresh_server_list(hwnd, select);
}

unsafe fn edit_server(hwnd: HWND) {
    let state = state_of(hwnd);
    if state.is_null() {
        return;
    }
    let index = selected_index(hwnd) as usize;
    let servers = &mut (*state).servers;
    if index >= servers.len() {
        return;
    }
    let existing = servers[index].clone();
    let Some(updated) = super::server_edit::show_dialog(hwnd, Some(existing), runtime_handle()) else {
        return;
    };
    servers[index] = updated;
    refresh_server_list(hwnd, index);
}

unsafe fn delete_server(hwnd: HWND) {
    let state = state_of(hwnd);
    if state.is_null() {
        return;
    }
    let index = selected_index(hwnd) as usize;
    let servers = &mut (*state).servers;
    if index >= servers.len() {
        return;
    }
    let server = servers[index].clone();
    let t = super::i18n::t();
    let answer = dialog::message_box(
        hwnd,
        &super::i18n::fmt(t.msg_delete_server, &[("server", &server.display_user_host())]),
        t.msg_delete_server_title,
        dialog::MessageBoxKind::Question,
        dialog::MessageBoxButtons::YesNoCancelDefault2,
    );
    if !matches!(answer, dialog::MessageBoxResult::Yes) {
        return;
    }
    (*state).servers.remove(index);
    refresh_server_list(hwnd, index.min((*state).servers.len().saturating_sub(1)));
}

fn runtime_handle() -> tokio::runtime::Handle {
    // 主线程没有 tokio 运行时上下文，Handle::current() 会 panic 并直接杀死进程
    // （extern "system" wndproc 中 panic = abort）。runtime 句柄必须从 App 取。
    super::app::with_app(|app| app.runtime.clone())
        .expect("App 未注册，无法获取 tokio runtime 句柄")
}

unsafe fn save_and_close(hwnd: HWND) {
    let state = state_of(hwnd);
    if state.is_null() {
        return;
    }
    let notifications_hwnd = GetDlgItem(Some(hwnd), IDC_NOTIFICATIONS).unwrap_or_default();
    let autostart_hwnd = GetDlgItem(Some(hwnd), IDC_AUTOSTART).unwrap_or_default();
    let refresh_updown = GetDlgItem(Some(hwnd), IDC_REFRESH_UPDOWN).unwrap_or_default();

    let notifications = SendMessageW(
        notifications_hwnd,
        windows::Win32::UI::WindowsAndMessaging::BM_GETCHECK,
        Some(WPARAM(0)), Some(LPARAM(0)),
    )
    .0 != 0;
    let autostart = SendMessageW(
        autostart_hwnd,
        windows::Win32::UI::WindowsAndMessaging::BM_GETCHECK,
        Some(WPARAM(0)), Some(LPARAM(0)),
    )
    .0 != 0;
    let refresh = dialog::updown_get_value(refresh_updown).clamp(2, 300);

    let mut result = (*state).base.clone();
    result.servers = (*state).servers.clone();
    result.refresh_seconds = refresh;
    result.notifications_enabled = notifications;
    result.start_with_windows = autostart;
    let language_selected = SendMessageW(
        GetDlgItem(Some(hwnd), IDC_LANGUAGE).unwrap_or_default(),
        windows::Win32::UI::WindowsAndMessaging::CB_GETCURSEL,
        Some(WPARAM(0)), Some(LPARAM(0)),
    )
    .0;
    result.language = if language_selected == 1 {
        crate::config::Language::En
    } else {
        crate::config::Language::ZhCn
    };
    (*state).result = Some(result);

    DestroyWindow(hwnd);
}

pub unsafe extern "system" fn settings_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => {
            // 窗口背景统一为主题色（类刷为空，否则默认白底与标签底色不一致）
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut _);
            let mut rect = windows::Win32::Foundation::RECT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rect);
            windows::Win32::Graphics::Gdi::FillRect(hdc, &rect, get_background_brush());
            return LRESULT(1);
        }
        WM_PAINT => {
            // 背景 + 服务器列表的白色圆角卡片（列表子窗口嵌入其内）
            let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
            let hdc = windows::Win32::Graphics::Gdi::BeginPaint(hwnd, &mut ps);
            if !hdc.is_invalid() {
                let mut rect = windows::Win32::Foundation::RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                windows::Win32::Graphics::Gdi::FillRect(hdc, &rect, get_background_brush());
                if let Some(g) = super::gfx::Graphics::from_hdc(hdc) {
                    let dpi = state_dpi(hwnd);
                    let card = super::gfx::Rect::new(
                        win::s(22, dpi) as f32,
                        win::s(104, dpi) as f32,
                        win::s(414, dpi) as f32,
                        win::s(214, dpi) as f32,
                    );
                    g.fill_rounded(theme::CARD_BACKGROUND, card, win::s(8, dpi) as f32);
                    g.draw_rounded(
                        theme::BORDER,
                        1.0,
                        super::gfx::Rect::new(card.x + 0.5, card.y + 0.5, card.w - 1.0, card.h - 1.0),
                        win::s(8, dpi) as f32,
                    );
                }
                let _ = windows::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);
            }
            return LRESULT(0);
        }
        WM_MEASUREITEM => {
            // 自绘列表行高
            let measure = lparam.0 as *mut windows::Win32::UI::Controls::MEASUREITEMSTRUCT;
            if !measure.is_null() && (*measure).CtlType.0 == 2 /* ODT_LISTBOX */ {
                let dpi = state_dpi(hwnd);
                (*measure).itemHeight = win::s(44, dpi) as u32;
                return LRESULT(1);
            }
        }
        WM_DRAWITEM => {
            let draw_item = lparam.0 as *mut DRAWITEMSTRUCT;
            if !draw_item.is_null() {
                let state = state_of(hwnd);
                match (*draw_item).CtlType.0 {
                    4 /* ODT_BUTTON */ => {
                        if !state.is_null() {
                            dialog::draw_flat_button(draw_item, &mut (*state).fonts);
                        }
                        return LRESULT(1);
                    }
                    2 /* ODT_LISTBOX */ => {
                        if !state.is_null() {
                            draw_server_list_item(draw_item, &mut (*state).fonts);
                        }
                        return LRESULT(1);
                    }
                    _ => {}
                }
            }
        }
        WM_CTLCOLORSTATIC => {
            // 标签文字颜色按创建时存的颜色（GWLP_USERDATA），背景与窗口一致
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut _);
            let ctl = windows::Win32::Foundation::HWND(lparam.0 as *mut _);
            let color = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(ctl, GWLP_USERDATA) as u32;
            let argb = if color == 0 { theme::TEXT } else { color };
            let _ = windows::Win32::Graphics::Gdi::SetTextColor(hdc, theme::argb_to_colorref(argb));
            let _ = windows::Win32::Graphics::Gdi::SetBkColor(hdc, theme::argb_to_colorref(theme::WINDOW_BACKGROUND));
            let brush = get_background_brush();
            return LRESULT(brush.0 as isize);
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as i32;
            let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
            match id {
                IDC_ADD => {
                    if code == 0 /* BN_CLICKED */ {
                        add_server(hwnd);
                        return LRESULT(0);
                    }
                }
                IDC_EDIT => {
                    if code == 0 {
                        edit_server(hwnd);
                        return LRESULT(0);
                    }
                }
                IDC_DELETE => {
                    if code == 0 {
                        delete_server(hwnd);
                        return LRESULT(0);
                    }
                }
                IDC_TEST_NOTIFY => {
                    if code == 0 {
                        dialog::set_window_text(
                            GetDlgItem(Some(hwnd), IDC_TEST_HINT).unwrap_or_default(),
                            super::i18n::t().settings_test_sent,
                        );
                        TEST_ACTION.with(|cell| {
                            if let Some(action) = cell.borrow_mut().as_mut() {
                                action();
                            }
                        });
                        return LRESULT(0);
                    }
                }
                IDC_SAVE => {
                    if code == 0 {
                        save_and_close(hwnd);
                        return LRESULT(0);
                    }
                }
                IDC_CANCEL => {
                    if code == 0 {
                        DestroyWindow(hwnd);
                        return LRESULT(0);
                    }
                }
                IDC_SERVER_LIST => {
                    if code == 1 /* LBN_SELCHANGE */ {
                        update_button_states(hwnd);
                        return LRESULT(0);
                    }
                    if code == 2 /* LBN_DBLCLK */ {
                        edit_server(hwnd);
                        return LRESULT(0);
                    }
                }
                _ => {}
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

fn get_background_brush() -> windows::Win32::Graphics::Gdi::HBRUSH {
    background_brush()
}

/// 供各对话框复用的窗口背景刷（RGB 247,248,250）。
pub fn background_brush() -> windows::Win32::Graphics::Gdi::HBRUSH {
    static BRUSH: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let brush = BRUSH.get_or_init(|| unsafe {
        windows::Win32::Graphics::Gdi::CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00FAF8F7))
            .0 as usize
    });
    windows::Win32::Graphics::Gdi::HBRUSH(*brush as *mut _)
}

/// 白色卡片刷（RGB 255,255,255，同 theme::CARD_BACKGROUND）：
/// 自绘列表行必须自己铺底，见 draw_server_list_item 的说明。
fn get_card_brush() -> windows::Win32::Graphics::Gdi::HBRUSH {
    static BRUSH: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let brush = BRUSH.get_or_init(|| unsafe {
        windows::Win32::Graphics::Gdi::CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00FF_FFFF))
            .0 as usize
    });
    windows::Win32::Graphics::Gdi::HBRUSH(*brush as *mut _)
}

/// 自绘服务器列表行：第一行服务器名（加粗），第二行 user@host；
/// 通知关闭时行尾带琥珀色“通知关闭”。选中行 AccentSoft 圆角底。
unsafe fn draw_server_list_item(
    draw_item: *mut DRAWITEMSTRUCT,
    fonts: &mut FontCache,
) {
    let item = &*draw_item;
    let selected = (item.itemState.0 & ODS_SELECTED.0) != 0;

    let row = super::gfx::Rect::new(
        item.rcItem.left as f32,
        item.rcItem.top as f32,
        (item.rcItem.right - item.rcItem.left) as f32,
        (item.rcItem.bottom - item.rcItem.top) as f32,
    );

    // 先整行铺白底——这一步不能省：LBS_OWNERDRAWFIXED 的列表框 Windows 不会擦除行背景
    // （MSDN WM_ERASEBKGND：“If the list box has the LBS_OWNERDRAW style, the background
    // is not erased.”）。少了它，失去选中的行只重画文字、上一轮的 AccentSoft 高亮永久残留，
    // 表现为“点过多少行就亮多少行”。用 GDI FillRect 保证像素级覆盖整个 rcItem。
    windows::Win32::Graphics::Gdi::FillRect(item.hDC, &item.rcItem, get_card_brush());

    // 空列表时 Windows 仍会发一次 itemID == -1（0xFFFF_FFFF）的绘制请求，铺完底即可返回；
    // 继续往下会把 LB_ERR(-1) 当成文本长度，算出 usize::MAX + 1。
    if (item.itemID as i32) < 0 {
        return;
    }

    // 取行文本（LBS_HASSTRINGS 语义存的是字符串）
    let len = SendMessageW(
        item.hwndItem,
        windows::Win32::UI::WindowsAndMessaging::LB_GETTEXTLEN,
        Some(WPARAM(item.itemID as usize)),
        Some(LPARAM(0)),
    ).0;
    if len < 0 {
        return;
    }
    let mut buffer = vec![0u16; len as usize + 1];
    SendMessageW(
        item.hwndItem,
        windows::Win32::UI::WindowsAndMessaging::LB_GETTEXT,
        Some(WPARAM(item.itemID as usize)),
        Some(LPARAM(buffer.as_mut_ptr() as isize)),
    );
    let text = win::from_wide(&buffer);

    let Some(g) = super::gfx::Graphics::from_hdc(item.hDC) else {
        return;
    };
    g.set_smoothing_anti_alias(true);

    if selected {
        g.fill_rounded(
            theme::ACCENT_SOFT,
            super::gfx::Rect::new(row.x + 2.0, row.y + 2.0, row.w - 4.0, row.h - 4.0),
            6.0,
        );
    }

    // 拆分“名称 · user@host”与“通知关闭”徽标（标记文本与写入侧同走翻译表，
    // 对话框生命周期内语言不变，两侧必然一致）。
    let marker = format!("  ·  {}", super::i18n::t().settings_list_notify_off);
    let (main, notice_off) = match text.find(&marker) {
        Some(index) => (&text[..index], true),
        None => (&text[..], false),
    };
    let (name, host) = match main.find(" · ") {
        Some(index) => (main[..index].trim(), Some(main[index + 3..].trim())),
        None => (main.trim(), None),
    };

    let dpi = fonts.dpi();
    let name_font = fonts.gdiplus_font(&super::gfx::FontSpec::ui_bold(9.0))
        .unwrap_or(std::ptr::null_mut());
    let host_font = fonts.gdiplus_font(&super::gfx::FontSpec::ui(8.25))
        .unwrap_or(std::ptr::null_mut());
    let badge_font = fonts.gdiplus_font(&super::gfx::FontSpec::ui(8.0))
        .unwrap_or(std::ptr::null_mut());
    let padding = win::s(12, dpi) as f32;

    match host {
        Some(host) => {
            g.draw_string(
                name,
                name_font,
                theme::TEXT,
                super::gfx::Rect::new(row.x + padding, row.y + win::s(6, dpi) as f32, row.w - padding * 2.0, win::s(18, dpi) as f32),
                super::gfx::TextFormat { end_ellipsis: true, ..super::gfx::TextFormat::LEFT_VCENTER },
            );
            g.draw_string(
                host,
                host_font,
                if selected { theme::ACCENT_DARK } else { theme::SECONDARY_TEXT },
                super::gfx::Rect::new(row.x + padding, row.y + win::s(24, dpi) as f32, row.w - padding * 2.0, win::s(16, dpi) as f32),
                super::gfx::TextFormat { end_ellipsis: true, ..super::gfx::TextFormat::LEFT_VCENTER },
            );
        }
        None => {
            g.draw_string(
                name,
                name_font,
                theme::TEXT,
                super::gfx::Rect::new(row.x + padding, row.y + win::s(6, dpi) as f32, row.w - padding * 2.0, win::s(30, dpi) as f32),
                super::gfx::TextFormat { end_ellipsis: true, ..super::gfx::TextFormat::LEFT_VCENTER },
            );
        }
    }

    if notice_off {
        let badge_text = super::i18n::t().settings_list_notify_off;
        g.draw_string(
            badge_text,
            badge_font,
            theme::AMBER,
            super::gfx::Rect::new(row.x + row.w - win::s(90, dpi) as f32, row.y + win::s(6, dpi) as f32, win::s(80, dpi) as f32, win::s(18, dpi) as f32),
            super::gfx::TextFormat { right: true, vcenter: true, ..Default::default() },
        );
    }
}
