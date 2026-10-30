//! 托盘图标与右键菜单：对应 C# TrayIconFactory + NotifyIcon + ContextMenuStrip。

use super::win;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::WindowsAndMessaging::HICON;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, SetForegroundWindow, SetMenuDefaultItem,
    TrackPopupMenu, LR_DEFAULTCOLOR,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
};
use windows::core::PCWSTR;

pub const WM_APP_TRAY: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;

pub const TRAY_ICON_ID: u32 = 1;

/// "TaskbarCreated" 广播消息 ID（Explorer 重启后托盘图标需要重建）。
pub fn taskbar_created_message() -> u32 {
    static CACHED: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| unsafe {
        let name = win::to_wide("TaskbarCreated");
        windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW(windows::core::PCWSTR(name.as_ptr()))
    })
}

/// 应用图标：优先从 exe 资源加载（RT_GROUP_ICON / ID 1，与 C# 清单资源一致）。
pub struct AppIcon(HICON);

impl AppIcon {
    pub fn raw(&self) -> HICON {
        self.0
    }
}

impl Drop for AppIcon {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyIcon(self.0);
        }
    }
}

/// 加载 exe 内嵌图标（对应 C# TrayIconFactory.LoadAppIcon）。
pub fn load_app_icon() -> Option<AppIcon> {
    unsafe {
        let instance = win::hinstance();
        let name = PCWSTR(1 as *const u16); // 资源 ID 1
        let handle = windows::Win32::UI::WindowsAndMessaging::LoadImageW(
            Some(instance.into()),
            name,
            windows::Win32::UI::WindowsAndMessaging::IMAGE_ICON,
            0,
            0,
            LR_DEFAULTCOLOR,
        );
        if let Ok(icon) = handle {
            return Some(AppIcon(HICON(icon.0)));
        }
        None
    }
}

/// 按目标像素尺寸加载同一图标：空状态大图标用，避免 32px 默认尺寸被放大发虚。
pub fn load_app_icon_sized(px: i32) -> Option<AppIcon> {
    unsafe {
        let instance = win::hinstance();
        let name = PCWSTR(1 as *const u16);
        let handle = windows::Win32::UI::WindowsAndMessaging::LoadImageW(
            Some(instance.into()),
            name,
            windows::Win32::UI::WindowsAndMessaging::IMAGE_ICON,
            px,
            px,
            LR_DEFAULTCOLOR,
        );
        if let Ok(icon) = handle {
            return Some(AppIcon(HICON(icon.0)));
        }
        None
    }
}

/// 托盘注册与消息（对应 NotifyIcon）。
pub struct TrayIcon {
    hwnd: HWND,
    added: bool,
    tooltip: String,
}

impl TrayIcon {
    pub fn new(hwnd: HWND, icon: HICON) -> TrayIcon {
        let mut tray = TrayIcon {
            hwnd,
            added: false,
            tooltip: super::i18n::t().app_name.to_string(),
        };
        tray.add(icon);
        tray
    }

    unsafe fn base_data(&self, icon: HICON) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW::default();
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.hwnd;
        data.uID = TRAY_ICON_ID;
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = WM_APP_TRAY;
        data.hIcon = icon;
        data
    }

    fn set_tip(data: &mut NOTIFYICONDATAW, tip: &str) {
        let mut buffer = [0u16; 128];
        for (dest, ch) in buffer.iter_mut().zip(tip.encode_utf16()) {
            *dest = ch;
        }
        data.szTip = buffer;
    }

    fn add(&mut self, icon: HICON) {
        unsafe {
            let mut data = self.base_data(icon);
            Self::set_tip(&mut data, &self.tooltip);
            if !Shell_NotifyIconW(NIM_ADD, &mut data).as_bool() {
                crate::util::error(
                    &format!(
                        "添加托盘图标失败（GetLastError={}）。",
                        windows::Win32::Foundation::GetLastError().0
                    ),
                    None,
                );
            } else {
                self.added = true;
            }
            let mut version_data = self.base_data(icon);
            version_data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            if !Shell_NotifyIconW(NIM_SETVERSION, &mut version_data).as_bool() {
                crate::util::error("设置托盘图标消息版本失败。", None);
            }
        }
    }

    /// Explorer 重启后重建图标（TaskbarCreated 广播）。
    pub fn readd(&mut self, icon: HICON) {
        if self.added {
            unsafe {
                let mut data = self.base_data(icon);
                let _ = Shell_NotifyIconW(NIM_DELETE, &mut data);
            }
            self.added = false;
        }
        self.add(icon);
    }

    pub fn set_tooltip(&mut self, icon: HICON, tip: &str) {
        if tip == self.tooltip {
            return;
        }
        // NOTIFYICONDATAW.szTip 上限 128 字符；托盘实际显示 63 字符。
        let truncated: String = tip.chars().take(63).collect();
        self.tooltip = truncated.clone();
        unsafe {
            let mut data = self.base_data(icon);
            Self::set_tip(&mut data, &truncated);
            let _ = Shell_NotifyIconW(NIM_MODIFY, &mut data);
        }
    }

    /// 气泡通知（Toast 失败时的回退，对应 NotifyIcon.ShowBalloonTip）。
    pub fn show_balloon(&mut self, icon: HICON, title: &str, body: &str, timeout_ms: u32) {
        unsafe {
            let mut data = self.base_data(icon);
            Self::set_tip(&mut data, &self.tooltip);
            data.uFlags |= NIF_INFO;
            let mut title_buffer = [0u16; 64];
            for (dest, ch) in title_buffer.iter_mut().zip(title.encode_utf16()) {
                *dest = ch;
            }
            data.szInfoTitle = title_buffer;
            let mut info_buffer = [0u16; 256];
            for (dest, ch) in info_buffer.iter_mut().zip(body.encode_utf16()) {
                *dest = ch;
            }
            data.szInfo = info_buffer;
            data.Anonymous.uTimeout = timeout_ms;
            data.dwInfoFlags = NIIF_INFO;
            let _ = Shell_NotifyIconW(NIM_MODIFY, &mut data);
        }
    }

    pub fn remove(&mut self, icon: HICON) {
        if !self.added {
            return;
        }
        unsafe {
            let mut data = self.base_data(icon);
            let _ = Shell_NotifyIconW(NIM_DELETE, &mut data);
            self.added = false;
        }
    }
}

/// 托盘右键菜单：打开面板 / 立即刷新 / — / 设置… / — / 退出。
/// 返回 Some(菜单命令)，None 表示取消。
pub enum TrayCommand {
    OpenPanel,
    RefreshNow,
    Settings,
    Exit,
}

pub fn show_tray_menu(hwnd: HWND) -> Option<TrayCommand> {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return None;
        };

        let t = super::i18n::t();
        let open = win::to_wide(t.tray_open_panel);
        let refresh = win::to_wide(t.tray_refresh_now);
        let settings = win::to_wide(t.tray_settings_menu);
        let exit = win::to_wide(t.tray_exit);

        AppendMenuW(menu, windows::Win32::UI::WindowsAndMessaging::MF_STRING, 1, PCWSTR(open.as_ptr()));
        AppendMenuW(menu, windows::Win32::UI::WindowsAndMessaging::MF_STRING, 2, PCWSTR(refresh.as_ptr()));
        AppendMenuW(menu, windows::Win32::UI::WindowsAndMessaging::MF_SEPARATOR, 0, None);
        AppendMenuW(menu, windows::Win32::UI::WindowsAndMessaging::MF_STRING, 3, PCWSTR(settings.as_ptr()));
        AppendMenuW(menu, windows::Win32::UI::WindowsAndMessaging::MF_SEPARATOR, 0, None);
        AppendMenuW(menu, windows::Win32::UI::WindowsAndMessaging::MF_STRING, 4, PCWSTR(exit.as_ptr()));
        let _ = SetMenuDefaultItem(menu, 1, 0);

        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let _ = SetForegroundWindow(hwnd);

        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_LEFTALIGN,
            cursor.x,
            cursor.y,
            None,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);

        match command.0 {
            1 => Some(TrayCommand::OpenPanel),
            2 => Some(TrayCommand::RefreshNow),
            3 => Some(TrayCommand::Settings),
            4 => Some(TrayCommand::Exit),
            _ => None,
        }
    }
}
