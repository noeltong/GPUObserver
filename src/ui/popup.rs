//! 托盘弹窗：对应 C# PopupForm + TabStrip + IconButton。
//! 520×715（无服务器时 520×300）逻辑像素，无边框、TopMost、工具窗口、投影、圆角。

use super::content::{ContentState, WM_APP_ESCAPE};
use super::gfx::{self, FontCache, FontSpec, Graphics, Rect, TextFormat};
use super::theme;
use super::win;
use crate::config::AppConfig;
use crate::monitor::snapshot::{MonitorPhase, ServerViewState};
use uuid::Uuid;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateRoundRectRgn, EndPaint, InvalidateRect, MonitorFromPoint, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TrackMouseEvent, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, KillTimer, PostMessageW, SetForegroundWindow,
    SetTimer, SetWindowPos, ShowWindow, GWLP_USERDATA, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SWP_NOMOVE, WA_INACTIVE,
    WM_ACTIVATE, WM_APP, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_PAINT, WM_TIMER, WS_POPUP,
};

pub const POPUP_WIDTH: i32 = 520;
pub const EMPTY_POPUP_HEIGHT: i32 = 300;
pub const FIXED_POPUP_HEIGHT: i32 = 715;

pub const WM_APP_UNSUPPRESS: u32 = WM_APP + 0x41;
pub const WM_APP_SYNC: u32 = WM_APP + 0x42;

const TIMER_UI: usize = 1;
const TIMER_TOOLTIP: usize = 2;
const TOOLTIP_DELAY_MS: u32 = 500;

#[derive(Clone)]
pub struct TabItem {
    pub id: Uuid,
    pub label: String,
    pub phase: MonitorPhase,
}

/// TabStrip 状态（C# TabStrip Control 的移植）。
pub struct TabStripState {
    pub items: Vec<TabItem>,
    pub bounds: Rect,
    cached_widths: Vec<f32>,
    tab_bounds: Vec<(Uuid, Rect)>,
    selected_id: Option<Uuid>,
    visible_start: usize,
    visible_count: usize,
    hover_index: i32,
    hover_arrow: i32,
    overflow: bool,
    left_arrow: Rect,
    right_arrow: Rect,
}

impl TabStripState {
    fn new() -> Self {
        TabStripState {
            items: Vec::new(),
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            cached_widths: Vec::new(),
            tab_bounds: Vec::new(),
            selected_id: None,
            visible_start: 0,
            visible_count: 0,
            hover_index: -1,
            hover_arrow: 0,
            overflow: false,
            left_arrow: Rect::new(0.0, 0.0, 0.0, 0.0),
            right_arrow: Rect::new(0.0, 0.0, 0.0, 0.0),
        }
    }

    fn set_items(&mut self, items: Vec<TabItem>, selected_id: Option<Uuid>, dpi: u32) {
        let selection_changed = selected_id != self.selected_id;
        self.items = items;
        self.selected_id = if selected_id.is_some_and(|id| self.items.iter().any(|item| item.id == id)) {
            selected_id
        } else {
            self.items.first().map(|item| item.id)
        };
        if selection_changed {
            self.ensure_selected_visible();
        }
        self.recalculate_layout(dpi);
    }

    fn ensure_selected_visible(&mut self) {
        if self.items.is_empty() {
            self.visible_start = 0;
            return;
        }
        let selected_index = match self.selected_id {
            Some(id) => self.items.iter().position(|item| item.id == id).unwrap_or(0),
            None => 0,
        };
        self.visible_start = selected_index.min(self.items.len().saturating_sub(1));
    }

    /// C# MeasureTab：TextRenderer 宽度 + S(32)，夹在 [S(58), S(140)]。
    fn measure_tab(item: &TabItem, dpi: u32, fonts: &mut FontCache) -> f32 {
        let text_width = fonts
            .gdi_font(&FontSpec::ui(9.25))
            .and_then(|font| {
                gfx::with_screen_dc(|dc| gfx::measure_text_offscreen(dc, &item.label, font)).map(|(w, _)| w)
            })
            .unwrap_or(0.0) as i32;
        let raw = text_width + win::s(32, dpi);
        raw.clamp(win::s(58, dpi), win::s(140, dpi)) as f32
    }

    fn recalculate_layout(&mut self, dpi: u32) {
        self.tab_bounds.clear();
        self.overflow = false;
        self.visible_count = 0;
        if self.items.is_empty() || self.bounds.w <= 0.0 || self.cached_widths.len() != self.items.len() {
            return;
        }

        let gap = win::s(4, dpi) as f32;
        let total: f32 =
            self.cached_widths.iter().sum::<f32>() + (self.items.len().saturating_sub(1)) as f32 * gap;
        self.overflow = total > self.bounds.w;

        let arrow_width = win::s(22, dpi) as f32;
        self.left_arrow = Rect::new(
            self.bounds.x,
            self.bounds.y + win::s(4, dpi) as f32,
            arrow_width,
            self.bounds.h - win::s(8, dpi) as f32,
        );
        self.right_arrow = Rect::new(
            self.bounds.x + self.bounds.w - arrow_width,
            self.bounds.y + win::s(4, dpi) as f32,
            arrow_width,
            self.bounds.h - win::s(8, dpi) as f32,
        );

        if self.overflow {
            self.visible_start = self.visible_start.min(self.items.len().saturating_sub(1));
        } else {
            self.visible_start = 0;
        }

        let content_left = if self.overflow { self.bounds.x + arrow_width } else { self.bounds.x };
        let content_right = if self.overflow {
            self.bounds.x + self.bounds.w - arrow_width
        } else {
            self.bounds.x + self.bounds.w
        };
        let mut x = content_left;
        let strip_top = self.bounds.y + 2.0;
        let strip_height = self.bounds.h - 4.0;

        for index in self.visible_start..self.items.len() {
            let mut width = self.cached_widths[index];
            if !self.tab_bounds.is_empty() && x + width > content_right {
                break;
            }
            if self.tab_bounds.is_empty() && x + width > content_right {
                width = (win::s(40, dpi) as f32).max(content_right - x);
            }
            self.tab_bounds
                .push((self.items[index].id, Rect::new(x, strip_top, width, strip_height)));
            x += width + gap;
        }

        self.visible_count = self.tab_bounds.len();
    }

    fn hit_test(&self, x: i32, y: i32) -> (i32, i32) {
        let (fx, fy) = (x as f32, y as f32);
        if self.overflow {
            if self.left_arrow.contains(fx, fy) {
                return (-1, -1);
            }
            if self.right_arrow.contains(fx, fy) {
                return (-1, 1);
            }
        }
        for (id, bounds) in &self.tab_bounds {
            if bounds.contains(fx, fy) {
                let index = self
                    .items
                    .iter()
                    .position(|item| item.id == *id)
                    .map(|i| i as i32)
                    .unwrap_or(-1);
                return (index, 0);
            }
        }
        (-1, 0)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum HeaderButton {
    Add,
    Settings,
    Refresh,
}

pub struct PopupState {
    pub hwnd: HWND,
    pub content_hwnd: HWND,
    pub dpi: u32,
    pub fonts: FontCache,
    pub tabs: TabStripState,
    pub selected_server_id: Option<Uuid>,
    suppress_deactivate: bool,
    hidden_at_ms: i64,
    allow_close: bool,
    footer_text: String,
    refresh_enabled: bool,
    header_hover: Option<HeaderButton>,
    add_bounds: Rect,
    settings_bounds: Rect,
    refresh_bounds: Rect,
    tooltip_pending: Option<String>,
    tooltip_visible_text: Option<String>,
    tooltip_hwnd: Option<HWND>,
}

impl PopupState {
    pub fn new(hwnd: HWND, content_hwnd: HWND, dpi: u32) -> Self {
        PopupState {
            hwnd,
            content_hwnd,
            dpi,
            fonts: FontCache::new(dpi),
            tabs: TabStripState::new(),
            selected_server_id: None,
            suppress_deactivate: false,
            hidden_at_ms: 0,
            allow_close: false,
            footer_text: String::new(),
            refresh_enabled: false,
            header_hover: None,
            add_bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            settings_bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            refresh_bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            tooltip_pending: None,
            tooltip_visible_text: None,
            tooltip_hwnd: None,
        }
    }

    fn s(&self, v: i32) -> i32 {
        win::s(v, self.dpi)
    }

    pub fn set_dpi(&mut self, dpi: u32) {
        if dpi != self.dpi {
            self.dpi = dpi;
            self.fonts = FontCache::new(dpi);
        }
    }

    pub fn is_visible(&self) -> bool {
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(self.hwnd).as_bool() }
    }

    pub fn was_hidden_within(&self, window_ms: i64) -> bool {
        if self.hidden_at_ms == 0 {
            return false;
        }
        tick_now() - self.hidden_at_ms <= window_ms
    }

    pub fn calculate_logical_height(&self, config: &AppConfig) -> i32 {
        if config.servers.is_empty() {
            EMPTY_POPUP_HEIGHT
        } else {
            FIXED_POPUP_HEIGHT
        }
    }

    /// 对应 PopupForm.ApplySizeAndPosition：按光标所在屏幕与任务栏边贴角。
    /// GetMonitorInfoW 失败或工作区退化时，回退主屏工作区右下角，绝不停在 (0,0)。
    pub fn apply_size_and_position(&mut self, config: &AppConfig) {
        unsafe {
            let mut cursor = POINT::default();
            let _ = GetCursorPos(&mut cursor);

            let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let monitor_ok = windows::Win32::Graphics::Gdi::GetMonitorInfoW(monitor, &mut info).as_bool()
                && info.rcWork.right > info.rcWork.left
                && info.rcWork.bottom > info.rcWork.top;

            let (working, bounds, monitor_dpi) = if monitor_ok {
                (info.rcWork, info.rcMonitor, dpi_of_monitor(monitor))
            } else {
                crate::util::error(
                    &format!(
                        "弹窗定位：光标显示器信息不可用（GetLastError={}），回退主屏工作区。",
                        windows::Win32::Foundation::GetLastError().0
                    ),
                    None,
                );
                let mut work = RECT::default();
                let _ = windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
                    windows::Win32::UI::WindowsAndMessaging::SPI_GETWORKAREA,
                    0,
                    Some(&mut work as *mut RECT as *mut core::ffi::c_void),
                    windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                );
                (work, work, 96)
            };

            let dpi = monitor_dpi;
            let scale = dpi as f64 / 96.0;

            let logical_height = self.calculate_logical_height(config);
            let margin = (10.0 * scale).round() as i32;
            let mut width = (POPUP_WIDTH as f64 * scale).round() as i32;
            let mut height = (logical_height as f64 * scale).round() as i32;
            width = width.min(((260.0 * scale) as i32).max(working.right - working.left - margin * 2));
            height = height.min(((220.0 * scale) as i32).max(working.bottom - working.top - margin * 2));

            let taskbar_bottom = working.bottom < bounds.bottom;
            let taskbar_top = working.top > bounds.top;
            let taskbar_left = working.left > bounds.left;
            let taskbar_right = working.right < bounds.right;

            let (mut x, mut y) = if taskbar_bottom {
                (working.right - width - margin, working.bottom - height - margin)
            } else if taskbar_top {
                (working.right - width - margin, working.top + margin)
            } else if taskbar_left {
                (working.left + margin, working.bottom - height - margin)
            } else if taskbar_right {
                (working.right - width - margin, working.bottom - height - margin)
            } else {
                let below = cursor.y > bounds.top + (bounds.bottom - bounds.top) / 2;
                if below {
                    (working.right - width - margin, cursor.y - height - margin * 2)
                } else {
                    (working.right - width - margin, cursor.y + margin * 2)
                }
            };

            let min_x = working.left + margin;
            let max_x = min_x.max(working.right - width - margin);
            let min_y = working.top + margin;
            let max_y = min_y.max(working.bottom - height - margin);
            x = x.clamp(min_x, max_x);
            y = y.clamp(min_y, max_y);

            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );

            // 定位诊断：弹窗显示由用户触发、频率低，可全量记录。
            crate::util::info(&format!(
                "弹窗定位：光标=({}, {})，工作区=({},{})-({},{})，任务栏边={}，目标=({}, {})+{}×{}。",
                cursor.x,
                cursor.y,
                working.left,
                working.top,
                working.right,
                working.bottom,
                if taskbar_bottom {
                    "下"
                } else if taskbar_top {
                    "上"
                } else if taskbar_left {
                    "左"
                } else if taskbar_right {
                    "右"
                } else {
                    "自动隐藏"
                },
                x,
                y,
                width,
                height,
            ));
        }
    }

    /// 对应 LayoutChildren。
    pub fn layout_children(&mut self) {
        unsafe {
            let mut client = RECT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(self.hwnd, &mut client);
            let width = client.right;
            let height = client.bottom;

            let header_y = self.s(7);
            let button = self.s(30);
            self.settings_bounds = Rect::new((width - self.s(10) - button) as f32, header_y as f32, button as f32, button as f32);
            self.add_bounds = Rect::new(
                (self.settings_bounds.x as i32 - self.s(4) - button) as f32,
                header_y as f32,
                button as f32,
                button as f32,
            );
            self.tabs.bounds = Rect::new(
                self.s(10) as f32,
                header_y as f32,
                (self.add_bounds.x as i32 - self.s(10) - self.s(4)).max(0) as f32,
                button as f32,
            );
            self.tabs.recalculate_layout(self.dpi);

            let footer_top = height - self.s(38);
            let content_height = (footer_top - self.s(47)).max(0);
            let _ = SetWindowPos(
                self.content_hwnd,
                None,
                0,
                self.s(47),
                width,
                content_height,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            self.refresh_bounds = Rect::new((width - self.s(10) - self.s(28)) as f32, (height - self.s(32)) as f32, self.s(28) as f32, self.s(28) as f32);

            let _ = InvalidateRect(Some(self.hwnd), None, true);
        }
    }

    fn apply_rounded_region(&mut self) {
        unsafe {
            let mut client = RECT::default();
            if windows::Win32::UI::WindowsAndMessaging::GetClientRect(self.hwnd, &mut client).is_ok() {
                let w = client.right;
                let h = client.bottom;
                if w > 0 && h > 0 {
                    let r = self.s(10);
                    let region = CreateRoundRectRgn(0, 0, w + 1, h + 1, r, r);
                if !region.is_invalid() {
                        let _ = windows::Win32::Graphics::Gdi::SetWindowRgn(self.hwnd, Some(region), true);
                    }
                }
            }
        }
    }

    /// 对应 ShowFlyout。
    pub fn show_flyout(&mut self, server_id: Option<Uuid>, config: &AppConfig, states: &[ServerViewState]) {
        if self.allow_close {
            return;
        }

        if let Some(id) = server_id {
            if config.servers.iter().any(|server| server.id == id) {
                self.selected_server_id = Some(id);
            }
        }

        self.sync_servers(config, states);

        if !self.is_visible() {
            self.apply_size_and_position(config);
            self.set_dpi(gfx::dpi_for_window(self.hwnd));
            self.layout_children();
            unsafe {
                let _ = ShowWindow(self.hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE);
            }
            self.apply_rounded_region();
            self.suppress_deactivate = true;
            unsafe {
                let _ = PostMessageW(Some(self.hwnd), WM_APP_UNSUPPRESS, WPARAM(0), LPARAM(0));
            }
            self.log_first_show_position();
        } else {
            // 已可见：不重复定位，但记录当前矩形，便于排查"可见但位置不对"。
            unsafe {
                let mut rect = RECT::default();
                if windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.hwnd, &mut rect).is_ok() {
                    crate::util::info(&format!(
                        "弹窗已可见，跳过定位。当前矩形 ({}, {})-({}, {})。",
                        rect.left, rect.top, rect.right, rect.bottom
                    ));
                }
            }
        }

        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
            // 仅提层（对应 C# BringToFront）：必须带 SWP_NOMOVE|SWP_NOSIZE，
            // 否则 x=0,y=0 会把已定位到任务栏旁的弹窗移到 (0,0)。
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(self.hwnd));
            SetTimer(Some(self.hwnd), TIMER_UI, 1000, None);
        }
    }

    pub fn hide_flyout(&mut self) {
        if !self.is_visible() {
            return;
        }
        self.hide_tooltip();
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_UI);
            let _ = ShowWindow(self.hwnd, windows::Win32::UI::WindowsAndMessaging::SW_HIDE);
        }
        self.hidden_at_ms = tick_now();
    }

    /// 诊断：首次显示时记录窗口矩形与屏幕工作区，验证贴角定位。
    fn log_first_show_position(&mut self) {
        static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if LOGGED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        unsafe {
            let mut rect = RECT::default();
            if windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.hwnd, &mut rect).is_ok() {
                crate::util::info(&format!(
                    "弹窗首次显示：窗口矩形 ({}, {})-({}, {})，DPI={}。",
                    rect.left, rect.top, rect.right, rect.bottom, self.dpi
                ));
            }
        }
    }

    pub fn close_for_exit(&mut self) {
        self.allow_close = true;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_UI);
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }

    pub fn select_server(&mut self, server_id: Option<Uuid>, config: &AppConfig, states: &[ServerViewState]) {
        self.selected_server_id = server_id;
        self.sync_servers(config, states);
    }

    /// 对应 SyncServers + RefreshContent。
    pub fn sync_servers(&mut self, config: &AppConfig, states: &[ServerViewState]) {
        if self.selected_server_id.is_none()
            || config
                .servers
                .iter()
                .all(|server| Some(server.id) != self.selected_server_id)
        {
            self.selected_server_id = config.servers.first().map(|server| server.id);
        }

        let items: Vec<TabItem> = config
            .servers
            .iter()
            .map(|server| {
                let phase = states
                    .iter()
                    .find(|state| state.server_id == server.id)
                    .map(|state| state.phase)
                    .unwrap_or(MonitorPhase::Stopped);
                TabItem {
                    id: server.id,
                    label: server.display_label(),
                    phase,
                }
            })
            .collect();

        let mut cached = Vec::with_capacity(items.len());
        for item in &items {
            cached.push(TabStripState::measure_tab(item, self.dpi, &mut self.fonts));
        }
        self.tabs.cached_widths = cached;
        let selected = self.selected_server_id;
        self.tabs.set_items(items, selected, self.dpi);

        let server = self
            .selected_server_id
            .and_then(|id| config.servers.iter().find(|item| item.id == id).cloned());
        let state = server
            .as_ref()
            .and_then(|server| states.iter().find(|item| item.server_id == server.id).cloned());

        self.refresh_enabled = server.is_some();
        self.footer_text = if server.is_none() {
            super::i18n::t().popup_footer_no_servers.to_string()
        } else if let Some(latest) = state.as_ref().and_then(|s| s.latest()) {
            let time = theme::format_clock(latest.captured_at);
            super::i18n::fmt(super::i18n::t().popup_footer_last_update, &[("time", &time)])
        } else {
            super::i18n::t().popup_footer_waiting.to_string()
        };

        unsafe {
            let content_state = content_state_of(self.content_hwnd);
            if !content_state.is_null() {
                (*content_state).set_dpi(self.dpi);
                (*content_state).set_data(server, state, config.refresh_seconds);
            }
        }

        self.adjust_height_if_needed(config);
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, true);
        }
    }

    fn adjust_height_if_needed(&mut self, config: &AppConfig) {
        if !self.is_visible() {
            return;
        }
        unsafe {
            let mut client = RECT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(self.hwnd, &mut client);
            let mut outer = RECT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.hwnd, &mut outer);
            let scale = (self.dpi as f64 / 96.0).max(0.5);
            let chrome = (outer.bottom - outer.top) - (client.bottom - client.top);
            let current_logical = ((outer.bottom - outer.top - chrome) as f64 / scale).round();
            let target = self.calculate_logical_height(config) as f64;
            if (target - current_logical).abs() < 40.0 {
                return;
            }
            self.apply_size_and_position(config);
        }
    }

    fn header_button_at(&self, x: i32, y: i32) -> Option<HeaderButton> {
        let (fx, fy) = (x as f32, y as f32);
        if self.add_bounds.contains(fx, fy) {
            Some(HeaderButton::Add)
        } else if self.settings_bounds.contains(fx, fy) {
            Some(HeaderButton::Settings)
        } else if self.refresh_bounds.contains(fx, fy) {
            Some(HeaderButton::Refresh)
        } else {
            None
        }
    }

    fn hide_tooltip(&mut self) {
        if let Some(hwnd) = self.tooltip_hwnd.take() {
            unsafe {
                let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_HIDE);
            }
        }
        self.tooltip_visible_text = None;
        self.tooltip_pending = None;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_TOOLTIP);
        }
    }

    fn update_tooltip_hint(&mut self, tip: Option<String>) {
        match tip {
            Some(text) => {
                if self.tooltip_visible_text.as_deref() == Some(text.as_str()) {
                    return;
                }
                if self.tooltip_pending.as_deref() != Some(text.as_str()) {
                    self.tooltip_pending = Some(text);
                    unsafe {
                        SetTimer(Some(self.hwnd), TIMER_TOOLTIP, TOOLTIP_DELAY_MS, None);
                    }
                }
            }
            None => self.hide_tooltip(),
        }
    }

    fn show_tooltip_if_pending(&mut self) {
        let Some(text) = self.tooltip_pending.clone() else {
            return;
        };
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_TOOLTIP);
            let tip_hwnd = match self.tooltip_hwnd {
                Some(hwnd) => hwnd,
                None => {
                    let class = win::to_wide(super::app::TOOLTIP_CLASS);
                    let created = CreateWindowExW(
                        windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0x0000_0080 | 0x0000_0008), // TOOLWINDOW | TOPMOST
                        windows::core::PCWSTR(class.as_ptr()),
                        None,
                        WS_POPUP,
                        0,
                        0,
                        10,
                        10,
                        Some(self.hwnd),
                        None,
                        Some(win::hinstance().into()),
                        None,
                    );
                    let Ok(hwnd) = created else {
                        return;
                    };
                    self.tooltip_hwnd = Some(hwnd);
                    hwnd
                }
            };

            let font = self.fonts.gdi_font(&FontSpec::ui(8.0));
            let (tw, th) = font
                .and_then(|f| gfx::with_screen_dc(|dc| gfx::measure_text_offscreen(dc, &text, f)))
                .unwrap_or((60.0, 16.0));
            let w = tw as i32 + 12;
            let h = th as i32 + 8;

            let mut cursor = POINT::default();
            let _ = GetCursorPos(&mut cursor);
            TOOLTIP_TEXT.lock().unwrap().replace(text.clone());
            let _ = SetWindowPos(
                tip_hwnd,
                Some(HWND_TOPMOST),
                cursor.x + 14,
                cursor.y + 16,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(Some(tip_hwnd), None, true);
            self.tooltip_visible_text = Some(text);
        }
    }

    fn paint(&mut self, hdc: windows::Win32::Graphics::Gdi::HDC) {
        let dpi = self.dpi;
        let hover = self.header_hover;
        let refresh_enabled = self.refresh_enabled;
        let add_bounds = self.add_bounds;
        let settings_bounds = self.settings_bounds;
        let refresh_bounds = self.refresh_bounds;
        let footer_text = self.footer_text.clone();
        let tabs_snapshot = self.tabs.items.clone();
        let tab_bounds = self.tabs.tab_bounds.clone();
        let selected_id = self.tabs.selected_id;
        let tab_hover = self.tabs.hover_index;
        let overflow = self.tabs.overflow;
        let left_arrow = self.tabs.left_arrow;
        let right_arrow = self.tabs.right_arrow;
        let visible_start = self.tabs.visible_start;
        let visible_count = self.tabs.visible_count;
        let arrow_hover = self.tabs.hover_arrow;

        gfx::double_buffer_paint(self.hwnd, hdc, |mem_dc, width, height| {
            let Some(g) = Graphics::from_hdc(mem_dc) else {
                return;
            };
            g.clear(theme::WINDOW_BACKGROUND);
            g.set_smoothing_anti_alias(true);

            let scale = dpi as f32 / 96.0;
            g.draw_rounded(
                theme::BORDER_STRONG,
                scale.max(1.0),
                Rect::new(0.5, 0.5, width as f32 - 1.0, height as f32 - 1.0),
                win::sf(10.0, dpi),
            );
            let s12 = win::s(12, dpi);
            g.draw_line(theme::BORDER, 1.0, s12 as f32, win::s(45, dpi) as f32, (width - s12) as f32, win::s(45, dpi) as f32);
            g.draw_line(
                theme::BORDER,
                1.0,
                s12 as f32,
                (height - win::s(35, dpi)) as f32,
                (width - s12) as f32,
                (height - win::s(35, dpi)) as f32,
            );

            // 头部图标按钮（IconButton 30×30，GDI 字形居中）
            let mut fonts = FontCache::new(dpi);
            let glyph_font = fonts.gdi_font(&FontSpec::icon(10.0));
            let draw_icon = |bounds: Rect, glyph: &str, hovered: bool, enabled: bool| {
                if hovered && enabled {
                    g.fill_rounded(theme::HOVER, bounds, 7.0);
                }
                let color = if !enabled {
                    theme::MUTED_TEXT
                } else {
                    theme::TEXT
                };
                if let Some(font) = glyph_font {
                    gfx::gdi_draw_text(mem_dc, glyph, font, color, bounds, TextFormat::CENTER);
                }
            };
            draw_icon(add_bounds, theme::GLYPH_ADD, hover == Some(HeaderButton::Add), true);
            draw_icon(settings_bounds, theme::GLYPH_SETTINGS, hover == Some(HeaderButton::Settings), true);
            draw_icon(refresh_bounds, theme::GLYPH_REFRESH, hover == Some(HeaderButton::Refresh), refresh_enabled);

            // TabStrip
            if !tabs_snapshot.is_empty() {
                if overflow {
                    let left_enabled = visible_start > 0;
                    if arrow_hover == -1 && left_enabled {
                        g.fill_rounded(theme::HOVER, left_arrow, win::sf(6.0, dpi));
                    }
                    let right_enabled = visible_start + visible_count < tabs_snapshot.len();
                    if arrow_hover == 1 && right_enabled {
                        g.fill_rounded(theme::HOVER, right_arrow, win::sf(6.0, dpi));
                    }
                    let arrow_font = fonts.gdi_font(&FontSpec::icon(7.5));
                    if let Some(font) = arrow_font {
                        gfx::gdi_draw_text(mem_dc, theme::GLYPH_LEFT, font, if left_enabled { theme::SECONDARY_TEXT } else { theme::MUTED_TEXT }, left_arrow, TextFormat::CENTER);
                        gfx::gdi_draw_text(mem_dc, theme::GLYPH_RIGHT, font, if right_enabled { theme::SECONDARY_TEXT } else { theme::MUTED_TEXT }, right_arrow, TextFormat::CENTER);
                    }
                }

                let label_font = fonts.gdi_font(&FontSpec::ui(9.25));
                for (id, bounds) in &tab_bounds {
                    let Some(index) = tabs_snapshot.iter().position(|item| item.id == *id) else {
                        continue;
                    };
                    let item = &tabs_snapshot[index];
                    let selected = Some(item.id) == selected_id;
                    let hovered = index as i32 == tab_hover;

                    if selected {
                        g.fill_rounded(theme::CARD_BACKGROUND, *bounds, win::sf(8.0, dpi));
                        g.draw_rounded(
                            theme::BORDER_STRONG,
                            1.0,
                            Rect::new(bounds.x + 0.5, bounds.y + 0.5, bounds.w - 1.0, bounds.h - 1.0),
                            win::sf(8.0, dpi),
                        );
                    } else if hovered {
                        g.fill_rounded(theme::HOVER, *bounds, win::sf(8.0, dpi));
                    }

                    theme::draw_status_dot(
                        &g,
                        bounds.x + win::s(11, dpi) as f32,
                        bounds.y + bounds.h / 2.0,
                        win::s(6, dpi) as f32,
                        theme::phase_color(item.phase),
                    );
                    if let Some(font) = label_font {
                        gfx::gdi_draw_text(
                            mem_dc,
                            &item.label,
                            font,
                            if selected { theme::TEXT } else { theme::SECONDARY_TEXT },
                            Rect::new(
                                bounds.x + win::s(20, dpi) as f32,
                                bounds.y,
                                (bounds.w as i32 - win::s(28, dpi)).max(0) as f32,
                                bounds.h,
                            ),
                            TextFormat { end_ellipsis: true, ..TextFormat::LEFT_VCENTER },
                        );
                    }
                }
            }

            // 页脚
            g.draw_string(
                &footer_text,
                fonts.gdiplus_font(&FontSpec::ui(8.25)).unwrap_or(std::ptr::null_mut()),
                theme::MUTED_TEXT,
                Rect::new(
                    win::s(14, dpi) as f32,
                    (height - win::s(30, dpi)) as f32,
                    (width - win::s(72, dpi)).max(0) as f32,
                    win::s(22, dpi) as f32,
                ),
                TextFormat::LEFT_VCENTER,
            );
        });
    }
}

static TOOLTIP_TEXT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn tick_now() -> i64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() as i64 }
}

pub unsafe fn content_state_of(hwnd: HWND) -> *mut ContentState {
    windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ContentState
}

fn dpi_of_monitor(monitor: windows::Win32::Graphics::Gdi::HMONITOR) -> u32 {
    unsafe {
        let mut dpi_x: u32 = 96;
        let mut dpi_y: u32 = 96;
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() && dpi_x > 0 {
            dpi_x
        } else {
            96
        }
    }
}

unsafe fn popup_state_of(hwnd: HWND) -> *mut PopupState {
    windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PopupState
}

pub unsafe extern "system" fn tooltip_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !hdc.is_invalid() {
                if let Some(text) = TOOLTIP_TEXT.lock().unwrap().clone() {
                    let mut client = RECT::default();
                    let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut client);
                    let width = client.right - client.left;
                    let height = client.bottom - client.top;
                    if let Some(g) = Graphics::from_hdc(hdc) {
                        g.clear(theme::CARD_BACKGROUND);
                        g.draw_rounded(
                            theme::BORDER_STRONG,
                            1.0,
                            Rect::new(0.5, 0.5, width as f32 - 1.0, height as f32 - 1.0),
                            4.0,
                        );
                        let mut fonts = FontCache::new(gfx::dpi_for_window(hwnd));
                        if let Some(font) = fonts.gdi_font(&FontSpec::ui(8.0)) {
                            gfx::gdi_draw_text(
                                hdc,
                                &text,
                                font,
                                theme::TEXT,
                                Rect::new(6.0, 2.0, width as f32 - 12.0, height as f32 - 4.0),
                                TextFormat { vcenter: true, ..Default::default() },
                            );
                        }
                    }
                }
                EndPaint(hwnd, &ps);
            }
            return LRESULT(0);
        }
        WM_ERASEBKGND => return LRESULT(1),
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

pub unsafe extern "system" fn popup_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = popup_state_of(hwnd);

    match msg {
        WM_PAINT => {
            static PAINTED_ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !PAINTED_ONCE.swap(true, std::sync::atomic::Ordering::SeqCst) {
                crate::util::info("弹窗首次 WM_PAINT 绘制。");
            }
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !hdc.is_invalid() {
                if !state_ptr.is_null() {
                    (*state_ptr).paint(hdc);
                }
                EndPaint(hwnd, &ps);
            }
            return LRESULT(0);
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_ACTIVATE => {
            let low = (wparam.0 & 0xFFFF) as u32;
            if !state_ptr.is_null() && low == WA_INACTIVE {
                let state = &mut *state_ptr;
                if state.is_visible() && !state.suppress_deactivate {
                    state.hide_flyout();
                }
            }
            return LRESULT(0);
        }
        WM_APP_UNSUPPRESS => {
            if !state_ptr.is_null() {
                (*state_ptr).suppress_deactivate = false;
            }
            return LRESULT(0);
        }
        WM_APP_ESCAPE => {
            if !state_ptr.is_null() {
                (*state_ptr).hide_flyout();
            }
            return LRESULT(1);
        }
        WM_APP_SYNC => {
            let _ = super::app::with_app(|app| app.sync_popup());
            return LRESULT(0);
        }
        WM_KEYDOWN => {
            if wparam.0 as u32 == 0x1B && !state_ptr.is_null() {
                (*state_ptr).hide_flyout();
                return LRESULT(1);
            }
        }
        WM_TIMER => {
            let id = wparam.0 as usize;
            if !state_ptr.is_null() {
                match id {
                    TIMER_UI => {
                        if (*state_ptr).is_visible() {
                            let _ = PostMessageW(Some(hwnd), WM_APP_SYNC, WPARAM(0), LPARAM(0));
                        }
                    }
                    TIMER_TOOLTIP => {
                        (*state_ptr).show_tooltip_if_pending();
                    }
                    _ => {}
                }
            }
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            if !state_ptr.is_null() {
                let x = (lparam.0 & 0xFFFF) as u16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i32;
                let state = &mut *state_ptr;
                let new_hover = state.header_button_at(x, y);
                let (tab_hover, arrow) = state.tabs.hit_test(x, y);
                if new_hover != state.header_hover
                    || tab_hover != state.tabs.hover_index
                    || arrow != state.tabs.hover_arrow
                {
                    state.header_hover = new_hover;
                    state.tabs.hover_index = tab_hover;
                    state.tabs.hover_arrow = arrow;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }

                let tip = if let Some(button) = new_hover {
                    let t = super::i18n::t();
                    match button {
                        HeaderButton::Add => Some(t.popup_tip_add.to_string()),
                        HeaderButton::Settings => Some(t.popup_tip_settings.to_string()),
                        HeaderButton::Refresh => Some(t.popup_tip_refresh.to_string()),
                    }
                } else if tab_hover >= 0 {
                    state.tabs.items.get(tab_hover as usize).map(|item| item.label.clone())
                } else {
                    None
                };
                state.update_tooltip_hint(tip);

                let mut event = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut event);
            }
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            if !state_ptr.is_null() {
                let state = &mut *state_ptr;
                state.header_hover = None;
                state.tabs.hover_index = -1;
                state.tabs.hover_arrow = 0;
                state.hide_tooltip();
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            if !state_ptr.is_null() {
                let x = (lparam.0 & 0xFFFF) as u16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i32;
                let state = &mut *state_ptr;

                if let Some(button) = state.header_button_at(x, y) {
                    let enabled = button != HeaderButton::Refresh || state.refresh_enabled;
                    if enabled {
                        match button {
                            HeaderButton::Add => {
                                let _ = super::app::with_app(|app| app.show_add_server());
                            }
                            HeaderButton::Settings => {
                                let _ = super::app::with_app(|app| app.show_settings());
                            }
                            HeaderButton::Refresh => {
                                let selected = state.selected_server_id;
                                let _ = super::app::with_app(|app| app.refresh_requested(selected));
                            }
                        }
                    }
                    return LRESULT(0);
                }

                if state.tabs.overflow {
                    let (fx, fy) = (x as f32, y as f32);
                    if state.tabs.left_arrow.contains(fx, fy) && state.tabs.visible_start > 0 {
                        state.tabs.visible_start -= 1;
                        state.tabs.recalculate_layout(state.dpi);
                        let _ = InvalidateRect(Some(hwnd), None, false);
                        return LRESULT(0);
                    }
                    if state.tabs.right_arrow.contains(fx, fy)
                        && state.tabs.visible_start + state.tabs.visible_count < state.tabs.items.len()
                    {
                        state.tabs.visible_start += 1;
                        state.tabs.recalculate_layout(state.dpi);
                        let _ = InvalidateRect(Some(hwnd), None, false);
                        return LRESULT(0);
                    }
                }

                let (index, _) = state.tabs.hit_test(x, y);
                if index >= 0 {
                    let id = state.tabs.items[index as usize].id;
                    if state.selected_server_id != Some(id) {
                        state.selected_server_id = Some(id);
                        let _ = InvalidateRect(Some(hwnd), None, false);
                        super::app::with_app(|app| app.sync_popup());
                    }
                }
            }
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            if !state_ptr.is_null() {
                let delta = (wparam.0 >> 16) as u16 as i16 as i32;
                let state = &mut *state_ptr;
                if state.tabs.overflow {
                    if delta > 0 && state.tabs.visible_start > 0 {
                        state.tabs.visible_start -= 1;
                        state.tabs.recalculate_layout(state.dpi);
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    } else if delta < 0
                        && state.tabs.visible_start + state.tabs.visible_count < state.tabs.items.len()
                    {
                        state.tabs.visible_start += 1;
                        state.tabs.recalculate_layout(state.dpi);
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
            }
            return LRESULT(0);
        }
        WM_DPICHANGED => {
            if !state_ptr.is_null() {
                let dpi = (wparam.0 as usize & 0xFFFF) as u32;
                let state = &mut *state_ptr;
                state.set_dpi(dpi);
                // 与 C# DpiChanged 一致：重新按光标屏幕定位并重排。
                let _ = super::app::with_app(|app| {
                    let config = app.config.clone();
                    state.apply_size_and_position(&config);
                });
                state.layout_children();
                state.apply_rounded_region();
            }
            return LRESULT(0);
        }
        _ => {}
    }

    DefWindowProcW(hwnd, msg, wparam, lparam)
}
