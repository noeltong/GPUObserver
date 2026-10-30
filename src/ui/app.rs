//! 应用调度：对应 C# TrayAppContext + Program。
//! 持有隐藏主窗口（托盘宿主）、弹窗、监控服务、事件队列，并接线所有交互。

use super::content::ContentState;
use super::popup::PopupState;
use super::tray::{show_tray_menu, TrayCommand, TrayIcon, WM_APP_TRAY};
use super::win;
use crate::config::{self, AppConfig};
use crate::monitor::service::{AppEvent, MonitorService};
use crate::monitor::snapshot::IdleNotificationArgs;
use super::tray::AppIcon;
use std::cell::RefCell;
use std::sync::mpsc::Receiver;
use uuid::Uuid;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer,
    PostQuitMessage, SetTimer, SetWindowLongPtrW, ShowWindow, TranslateMessage, GWLP_USERDATA,
    SW_HIDE, WINDOW_STYLE, WM_APP, WM_DESTROY, WM_TIMER, WS_CHILD, WS_CLIPCHILDREN, WS_VSCROLL,
    WS_OVERLAPPED, WS_POPUP, WS_VISIBLE,
};

pub const APP_WINDOW_CLASS: &str = "GPUOBSApp";
pub const POPUP_WINDOW_CLASS: &str = "GPUOBSFlyout";
pub const CONTENT_WINDOW_CLASS: &str = "GPUOBSContent";
pub const TOOLTIP_CLASS: &str = "GPUOBSTooltip";
pub const SETTINGS_WINDOW_CLASS: &str = "GPUOBSSettings";
pub const SERVER_EDIT_WINDOW_CLASS: &str = "GPUOBSServerEdit";

pub const WM_APP_EVENT: u32 = WM_APP + 0x50;

const TIMER_TRAY_TEXT: usize = 1;
const TIMER_STARTUP: usize = 2;
const TRAY_TEXT_INTERVAL: u32 = 2000;
const STARTUP_DELAY_MS: u32 = 180;

pub enum UiMsg {
    Event(AppEvent),
    Pipe(String),
}

thread_local! {
    static APP_PTR: RefCell<Option<*mut App>> = const { RefCell::new(None) };
}

/// 主线程内访问 App（所有窗口过程都在主线程）。
pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP_PTR.with(|cell| {
        cell.borrow().map(|ptr| {
            let app = unsafe { &mut *ptr };
            f(app)
        })
    })
}

/// 把 App 注册给窗口过程使用（在创建 App 之后、消息循环之前调用）。
pub fn register_app(app: &mut App) {
    APP_PTR.with(|cell| {
        *cell.borrow_mut() = Some(app as *mut App);
    });
}

pub struct App {
    pub config: AppConfig,
    pub monitors: MonitorService,
    pub popup: Option<Box<PopupState>>,
    pub content: Option<Box<ContentState>>,
    pub app_hwnd: HWND,
    pub runtime: tokio::runtime::Handle,
    pub tray: TrayIcon,
    pub tray_icon: AppIcon,
    pub msg_rx: Receiver<UiMsg>,
    start_hidden: bool,
    created_new: bool,
    load_warning: Option<String>,
    startup_handled: bool,
    pending_launch: Option<String>,
    exiting: bool,
    /// 模态对话框（设置/编辑服务器）打开中——防止模态循环重入。
    modal_open: bool,
}

impl App {
    /// 创建 App。event_sink / pipe_sink 由 main 提供：发送事件并 PostMessage 唤醒主线程。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runtime: tokio::runtime::Handle,
        start_hidden: bool,
        pending_launch: Option<String>,
        msg_rx: Receiver<UiMsg>,
        event_sink: std::sync::Arc<dyn Fn(AppEvent) + Send + Sync>,
    ) -> Result<Box<App>, String> {
        unsafe {
            let load = config::load();
            // 语言先于任何窗口/文案：加载失败回落默认 zh-CN。
            super::i18n::set_lang(load.config.language);
            let mut config_data = load.config;
            let created_new = load.created_new;
            let load_warning = load.warning.clone();
            if created_new {
                if let Err(err) = config::save(&mut config_data) {
                    crate::util::error("创建初始配置失败。", Some(&err.to_string()));
                }
            }

            // 隐藏主窗口（托盘宿主）
            let class = win::to_wide(APP_WINDOW_CLASS);
            let app_hwnd = CreateWindowExW(
                windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
                windows::core::PCWSTR(class.as_ptr()),
                None,
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(win::hinstance().into()),
                None,
            );
            let Ok(app_hwnd) = app_hwnd else {
                return Err(super::i18n::t().err_create_main_window.to_string());
            };
            let _ = ShowWindow(app_hwnd, SW_HIDE);

            let tray_icon = super::tray::load_app_icon().ok_or_else(|| super::i18n::t().err_load_app_icon.to_string())?;
            let tray = TrayIcon::new(app_hwnd, tray_icon.raw());

            let monitors = MonitorService::new(&runtime, event_sink);
            monitors.apply_config(config_data.clone());

            let mut app = Box::new(App {
                config: config_data,
                monitors,
                popup: None,
                content: None,
                app_hwnd,
                runtime,
                tray,
                tray_icon,
                msg_rx,
                start_hidden,
                created_new,
                load_warning,
                startup_handled: false,
                pending_launch,
                exiting: false,
                modal_open: false,
            });

            app.create_popup();
            if app.popup.is_some() {
                crate::util::info(&format!(
                    "启动完成：主窗口/托盘/弹窗/内容窗口均已创建（{} 台服务器）。",
                    app.config.servers.len()
                ));
            }

            SetTimer(Some(app_hwnd), TIMER_TRAY_TEXT, TRAY_TEXT_INTERVAL, None);
            SetTimer(Some(app_hwnd), TIMER_STARTUP, STARTUP_DELAY_MS, None);
            app.update_tray_text();

            if app.config.start_with_windows {
                crate::autostart::set_enabled(true);
            }

            Ok(app)
        }
    }

    unsafe fn create_popup(&mut self) {
        let dpi = super::gfx::screen_dpi();
        let popup_class = win::to_wide(POPUP_WINDOW_CLASS);
        let popup_hwnd = CreateWindowExW(
            // WS_EX_TOOLWINDOW | WS_EX_TOPMOST
            windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0x0000_0080 | 0x0000_0008),
            windows::core::PCWSTR(popup_class.as_ptr()),
            None,
            WINDOW_STYLE(WS_POPUP.0 | WS_CLIPCHILDREN.0),
            0,
            0,
            win::s(super::popup::POPUP_WIDTH, dpi),
            win::s(super::popup::FIXED_POPUP_HEIGHT, dpi),
            Some(self.app_hwnd),
            None,
            Some(win::hinstance().into()),
            None,
        );
        let Ok(popup_hwnd) = popup_hwnd else {
            crate::util::error("创建弹窗失败。", None);
            return;
        };

        let content_class = win::to_wide(CONTENT_WINDOW_CLASS);
        let content_hwnd = CreateWindowExW(
            windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
            windows::core::PCWSTR(content_class.as_ptr()),
            None,
            // WS_VSCROLL：滚动条必须来自窗口样式（ShowScrollBar 只能开关样式里存在的条），
            // 且此时 GetClientRect 自动排除滚动条区域，绘制无需再手工扣宽。
            WINDOW_STYLE(WS_CHILD.0 | WS_CLIPCHILDREN.0 | WS_VISIBLE.0 | WS_VSCROLL.0),
            0,
            win::s(47, dpi),
            10,
            10,
            Some(popup_hwnd),
            None,
            Some(win::hinstance().into()),
            None,
        );
        let Ok(content_hwnd) = content_hwnd else {
            crate::util::error("创建内容窗口失败。", None);
            return;
        };

        let mut content = Box::new(ContentState::new(content_hwnd, dpi));
        SetWindowLongPtrW(content_hwnd, GWLP_USERDATA, &mut *content as *mut ContentState as isize);

        let mut popup = Box::new(PopupState::new(popup_hwnd, content_hwnd, dpi));
        popup.layout_children();
        SetWindowLongPtrW(popup_hwnd, GWLP_USERDATA, &mut *popup as *mut PopupState as isize);

        self.popup = Some(popup);
        self.content = Some(content);
    }

    pub fn run(&mut self) {
        unsafe {
            let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
            loop {
                let result = GetMessageW(&mut msg, None, 0, 0);
                if result.0 <= 0 {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&mut msg);
            }
        }
    }

    // ---- 交互入口（由各窗口过程调用）----

    pub fn show_popup(&mut self, server_id: Option<Uuid>) {
        if self.exiting {
            return;
        }
        let config = self.config.clone();
        let states = self.monitors.get_states();
        if let Some(popup) = self.popup.as_deref_mut() {
            popup.show_flyout(server_id, &config, &states);
        }
    }

    pub fn toggle_popup(&mut self) {
        if self.exiting {
            return;
        }
        let visible = self.popup.as_ref().is_some_and(|p| p.is_visible());
        if visible {
            if let Some(popup) = self.popup.as_deref_mut() {
                popup.hide_flyout();
            }
            return;
        }
        let hidden_recently = self
            .popup
            .as_ref()
            .is_some_and(|p| p.was_hidden_within(300));
        if hidden_recently {
            return;
        }
        self.show_popup(None);
    }

    pub fn show_add_server(&mut self) {
        if self.exiting || self.modal_open {
            return;
        }
        self.modal_open = true;
        let was_visible = self.popup.as_ref().is_some_and(|p| p.is_visible());
        if let Some(popup) = self.popup.as_deref_mut() {
            popup.hide_flyout();
        }

        let result = super::server_edit::show_dialog(self.app_hwnd, None, self.runtime.clone());
        self.modal_open = false;
        if let Some(server) = result {
            let mut new_config = self.config.clone();
            new_config.servers.push(server);
            if self.save_and_apply(new_config) {
                let last_id = self.config.servers.last().map(|s| s.id);
                self.show_popup(last_id);
                return;
            }
        }

        if was_visible || self.config.servers.is_empty() {
            self.show_popup(None);
        }
    }

    pub fn show_settings(&mut self) {
        if self.exiting || self.modal_open {
            return;
        }
        self.modal_open = true;
        let was_visible = self.popup.as_ref().is_some_and(|p| p.is_visible());
        if let Some(popup) = self.popup.as_deref_mut() {
            popup.hide_flyout();
        }
        let first_server = self.config.servers.first().map(|s| (s.id, s.display_host()));

        let result = super::settings::show_dialog(self.app_hwnd, &self.config, &self.runtime, move || {
            let (server_id, server_label) = first_server
                .clone()
                .map(|(id, label)| (Some(id), Some(label)))
                .unwrap_or((None, None));
            super::app::with_app(|app| {
                let (hwnd, icon) = (app.app_hwnd, app.tray_icon.raw());
                super::toast::show_test(server_id, server_label.as_deref(), hwnd, icon);
            });
        });
        self.modal_open = false;

        if let Some(updated) = result {
            if self.save_and_apply(updated.clone()) {
                if !crate::autostart::set_enabled(updated.start_with_windows) {
                    super::dialog::message_box(
                        self.app_hwnd,
                        super::i18n::t().msg_autostart_failed,
                        super::i18n::t().app_name,
                        super::dialog::MessageBoxKind::Warning,
                        super::dialog::MessageBoxButtons::Ok,
                    );
                }
            }
        }

        if was_visible {
            self.show_popup(None);
        }
    }

    /// 对应 SaveAndApply：保存 + 应用 + 通知 + 更新托盘文本。
    /// 语言变化即时生效：切换全局语言后重绘常驻自绘窗口（popup/content），
    /// 对话框/托盘菜单/toast 本身按需重建，无需额外处理。
    pub fn save_and_apply(&mut self, new_config: AppConfig) -> bool {
        let mut new_config = new_config;
        if let Err(err) = config::save(&mut new_config) {
            crate::util::error("保存设置失败。", Some(&err.to_string()));
            let text = super::i18n::fmt(super::i18n::t().msg_save_failed, &[("err", &err.to_string())]);
            super::dialog::message_box(
                self.app_hwnd,
                &text,
                super::i18n::t().app_name,
                super::dialog::MessageBoxKind::Error,
                super::dialog::MessageBoxButtons::Ok,
            );
            return false;
        }

        let language_changed = new_config.language != self.config.language;
        if language_changed {
            super::i18n::set_lang(new_config.language);
        }
        self.config = new_config.clone();
        self.monitors.apply_config(new_config);
        self.notify_servers_changed();
        if language_changed {
            self.apply_language();
        }
        self.update_tray_text();
        true
    }

    /// 语言切换后的常驻窗口刷新：popup/content 是自绘窗口，重绘即换文案；
    /// 页脚文本由 sync_popup 立即重算（否则 1 秒定时器兜底）。
    fn apply_language(&mut self) {
        unsafe {
            if let Some(popup) = self.popup.as_deref() {
                let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(popup.hwnd), None, true);
            }
            if let Some(content) = self.content.as_deref() {
                let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(content.hwnd), None, true);
            }
        }
        self.sync_popup();
    }

    pub fn notify_servers_changed(&mut self) {
        let config = self.config.clone();
        let states = self.monitors.get_states();
        if let Some(popup) = self.popup.as_deref_mut() {
            if popup.is_visible() {
                popup.sync_servers(&config, &states);
                popup.layout_children();
            }
        }
    }

    pub fn sync_popup(&mut self) {
        let config = self.config.clone();
        let states = self.monitors.get_states();
        if let Some(popup) = self.popup.as_deref_mut() {
            popup.sync_servers(&config, &states);
        }
    }

    pub fn refresh_requested(&mut self, server_id: Option<Uuid>) {
        match server_id {
            Some(id) => self.monitors.refresh(id),
            None => self.monitors.refresh_all(),
        }
    }

    pub fn retry_server(&mut self, server_id: Uuid) {
        self.monitors.refresh(server_id);
    }

    pub fn show_balloon(&mut self, title: &str, body: &str, timeout_ms: u32) {
        self.tray
            .show_balloon(self.tray_icon.raw(), title, body, timeout_ms);
    }

    // ---- 事件处理 ----

    pub fn drain_messages(&mut self) {
        while let Ok(msg) = self.msg_rx.try_recv() {
            self.handle_ui_msg(msg);
        }
    }

    fn handle_ui_msg(&mut self, msg: UiMsg) {
        match msg {
            UiMsg::Pipe(text) => self.handle_pipe_message(&text),
            UiMsg::Event(event) => match event {
                AppEvent::ServersChanged => self.notify_servers_changed(),
                AppEvent::ServerChanged(_) => {
                    if self.popup.as_ref().is_some_and(|p| p.is_visible()) {
                        self.sync_popup();
                    }
                }
                AppEvent::HostKeyLearned(server_id, fingerprint) => {
                    self.handle_host_key_learned(server_id, fingerprint)
                }
                AppEvent::IdleDetected(args) => self.handle_idle_detected(args),
            },
        }
    }

    fn handle_idle_detected(&mut self, args: IdleNotificationArgs) {
        if self.exiting {
            return;
        }
        let Some(server) = self
            .config
            .servers
            .iter()
            .find(|item| item.id == args.server_id)
            .cloned()
        else {
            return;
        };
        if !self.config.notifications_enabled || !server.notifications_enabled {
            return;
        }
        super::toast::show_idle(&args, self.app_hwnd, self.tray_icon.raw());
    }

    fn handle_host_key_learned(&mut self, server_id: Uuid, fingerprint: String) {
        if self.exiting {
            return;
        }
        let mut updated = self.config.clone();
        let Some(server) = updated.servers.iter_mut().find(|item| item.id == server_id) else {
            return;
        };
        server.host_key_fingerprint_sha256 = Some(fingerprint);
        if let Err(err) = config::save(&mut updated) {
            crate::util::error("保存主机密钥指纹失败。", Some(&err.to_string()));
        } else {
            self.config = updated;
        }
    }

    fn handle_pipe_message(&mut self, message: &str) {
        if self.exiting {
            return;
        }
        const PREFIX: &str = "open-server:";
        if let Some(rest) = message.strip_prefix(PREFIX) {
            if let Ok(server_id) = Uuid::parse_str(rest.trim()) {
                if self.config.servers.iter().any(|server| server.id == server_id) {
                    self.show_popup(Some(server_id));
                    return;
                }
            }
        } else if message.eq_ignore_ascii_case("show") {
            self.show_popup(None);
            return;
        }

        if let Some(server_id) = super::toast::parse_launch_argument(message) {
            self.show_popup(Some(server_id));
        } else {
            self.show_popup(None);
        }
    }

    pub fn update_tray_text(&mut self) {
        if self.exiting {
            return;
        }
        let states = self.monitors.get_states();
        let errors = states
            .iter()
            .filter(|state| state.phase == crate::monitor::snapshot::MonitorPhase::Error)
            .count();
        let t = super::i18n::t();
        let n_text = states.len().to_string();
        let text = if states.is_empty() {
            t.tray_no_servers.to_string()
        } else {
            let mut text = super::i18n::fmt(t.tray_servers, &[("n", &n_text)]);
            if errors > 0 {
                text.push_str(&super::i18n::fmt(t.tray_errors_suffix, &[("n", &errors.to_string())]));
            }
            text
        };
        let truncated: String = text.chars().take(63).collect();
        self.tray.set_tooltip(self.tray_icon.raw(), &truncated);
    }

    fn handle_startup(&mut self) {
        if self.exiting || self.startup_handled {
            return;
        }
        self.startup_handled = true;

        let pending_server = self
            .pending_launch
            .as_deref()
            .and_then(super::toast::parse_launch_argument);

        if let Some(server_id) = pending_server {
            if self.config.servers.iter().any(|server| server.id == server_id) {
                self.show_popup(Some(server_id));
            } else if !self.start_hidden {
                self.show_popup(None);
            }
        } else if !self.start_hidden {
            self.show_popup(None);
            if self.created_new {
                self.show_add_server();
            }
        }

        if let Some(warning) = self.load_warning.clone() {
            self.show_balloon(super::i18n::t().app_name, &super::i18n::localize_backend(&warning), 8000);
        }
    }

    pub fn exit_app(&mut self) {
        if self.exiting {
            return;
        }
        self.exiting = true;
        unsafe {
            let _ = KillTimer(Some(self.app_hwnd), TIMER_TRAY_TEXT);
            let _ = KillTimer(Some(self.app_hwnd), TIMER_STARTUP);
        }
        self.monitors.shutdown();
        if let Some(popup) = self.popup.as_deref_mut() {
            popup.close_for_exit();
        }
        self.tray.remove(self.tray_icon.raw());
        unsafe {
            let _ = PostQuitMessage(0);
        }
    }
}

pub unsafe extern "system" fn app_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Explorer 重启广播：重建托盘图标。
    if msg == super::tray::taskbar_created_message() {
        with_app(|app| app.tray.readd(app.tray_icon.raw()));
        return LRESULT(0);
    }

    match msg {
        WM_APP_TRAY => {
            let event = (lparam.0 & 0xFFFF) as u32;
            match event {
                0x0202 => {
                    // WM_LBUTTONUP：左键切换弹窗
                    with_app(|app| app.toggle_popup());
                }
                0x007B | 0x0205 => {
                    // WM_CONTEXTMENU / WM_RBUTTONUP：右键菜单
                    if let Some(command) = show_tray_menu(hwnd) {
                        match command {
                            TrayCommand::OpenPanel => {
                                with_app(|app| app.show_popup(None));
                            }
                            TrayCommand::RefreshNow => {
                                with_app(|app| app.monitors.refresh_all());
                            }
                            TrayCommand::Settings => {
                                with_app(|app| app.show_settings());
                            }
                            TrayCommand::Exit => {
                                with_app(|app| app.exit_app());
                            }
                        }
                    }
                }
                0x0405 => {
                    // NIN_BALLOONUSERCLICK：点击气泡
                    if let Some(server_id) = super::toast::take_balloon_server_id() {
                        with_app(|app| app.show_popup(Some(server_id)));
                    } else {
                        with_app(|app| app.show_popup(None));
                    }
                }
                _ => {}
            }
            return LRESULT(0);
        }
        WM_APP_EVENT => {
            with_app(|app| app.drain_messages());
            return LRESULT(0);
        }
        WM_TIMER => {
            let id = wparam.0 as usize;
            match id {
                TIMER_TRAY_TEXT => {
                    with_app(|app| app.update_tray_text());
                }
                TIMER_STARTUP => {
                    with_app(|app| {
                        unsafe {
                            let _ = KillTimer(Some(hwnd), TIMER_STARTUP);
                        }
                        app.handle_startup();
                    });
                }
                _ => {}
            }
            return LRESULT(0);
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}
