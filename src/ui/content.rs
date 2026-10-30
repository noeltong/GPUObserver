//! 服务器内容面板：对应 C# ServerContentPanel + Sparkline。
//! 弹窗里的滚动子窗口：头部/横幅/内存卡/GPU 卡/进程列表/无数据卡/空状态。

use super::gfx::{self, FontCache, FontSpec, Graphics, Rect, TextFormat};
use super::theme;
use super::win;
use crate::config::ServerConfig;
use crate::monitor::queue::{self, RETENTION_SECS};
use crate::monitor::snapshot::{GpuTelemetry, MonitorPhase, RamTelemetry, ServerSnapshot, ServerViewState};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::UI::Controls::{SetScrollInfo, ShowScrollBar, WM_MOUSELEAVE};
use windows::Win32::UI::WindowsAndMessaging::{SB_VERT, SCROLL_WINDOW_FLAGS};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, SetFocus, TME_LEAVE, TrackMouseEvent, TRACKMOUSEEVENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DrawIconEx, GetClientRect, GetScrollInfo, GetSystemMetrics, GWLP_USERDATA,
    MA_ACTIVATE, SB_LINEDOWN, SB_LINEUP, SB_PAGEUP, SB_PAGEDOWN, SB_THUMBPOSITION, SB_THUMBTRACK,
    ScrollWindowEx, SCROLLINFO, SIF_ALL, SIF_PAGE, SIF_POS, SIF_RANGE,
    SM_CXVSCROLL, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_PAINT, WM_VSCROLL,
};

pub const WM_APP_ESCAPE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x40;

#[derive(Clone, Copy, PartialEq)]
enum ActionKind {
    None,
    AddServer,
    Retry,
}

pub struct ContentState {
    pub hwnd: HWND,
    pub dpi: u32,
    pub fonts: FontCache,
    pub app_icon: Option<super::tray::AppIcon>,
    pub server: Option<ServerConfig>,
    pub state: Option<ServerViewState>,
    pub refresh_seconds: i32,
    pub now: DateTime<Utc>,
    pub scroll_pos: i32,
    pub content_height: i32,
    action_kind: ActionKind,
    action_bounds: Rect,
    action_hovered: bool,
    scrollbar_visible: bool,
}

impl ContentState {
    pub fn new(hwnd: HWND, dpi: u32) -> Self {
        ContentState {
            hwnd,
            dpi,
            fonts: FontCache::new(dpi),
            app_icon: None,
            server: None,
            state: None,
            refresh_seconds: 5,
            now: Utc::now(),
            scroll_pos: 0,
            content_height: 0,
            action_kind: ActionKind::None,
            action_bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            action_hovered: false,
            scrollbar_visible: false,
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

    /// 对应 ServerContentPanel.SetData。
    pub fn set_data(
        &mut self,
        server: Option<ServerConfig>,
        state: Option<ServerViewState>,
        refresh_seconds: i32,
    ) {
        self.server = server;
        self.now = Utc::now();
        self.state = state.map(|mut state| {
            let cutoff = self.now - queue::retention();
            state.history.retain(|sample| sample.captured_at >= cutoff);
            state.history.sort_by_key(|sample| sample.captured_at);
            state
        });
        self.refresh_seconds = refresh_seconds;
        self.update_scroll_size();
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    pub fn client_size(&self) -> (i32, i32) {
        let mut rect = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rect);
        }
        (rect.right, rect.bottom)
    }

    fn scrollbar_width(&self) -> i32 {
        unsafe { GetSystemMetrics(SM_CXVSCROLL).max(0) }
    }

    fn fresh_latest(&self) -> Option<Arc<ServerSnapshot>> {
        let latest = self.state.as_ref()?.latest()?;
        if self.now - latest.captured_at > queue::retention() {
            None
        } else {
            Some(latest.clone())
        }
    }

    fn measure_content_height(&self) -> i32 {
        if self.server.is_none() {
            return 0;
        }
        let Some(latest) = self.fresh_latest() else {
            return self.s(56) + self.s(148) + self.s(16);
        };
        let mut height = self.s(56);
        if self
            .state
            .as_ref()
            .is_some_and(|state| state.phase == MonitorPhase::Error && state.error.is_some())
        {
            height += self.s(42);
        }
        if latest.warning.as_deref().is_some_and(|w| !w.trim().is_empty()) {
            height += self.s(40);
        }
        if latest.ram.is_some() {
            height += self.s(62);
        }
        height += (latest.gpus.len().max(3) as i32) * (self.s(158) + self.s(10));
        height += self.s(24);
        let rows = latest.processes.len().max(1) as i32;
        height += self.s(10) + rows * self.s(24) + self.s(10) + self.s(10);
        height + self.s(8)
    }

    fn update_scroll_size(&mut self) {
        let (_, client_h) = self.client_size();
        self.content_height = self.measure_content_height();
        let need_scroll = self.content_height > client_h && client_h > 0;
        if need_scroll != self.scrollbar_visible {
            self.scrollbar_visible = need_scroll;
            unsafe {
                ShowScrollBar(self.hwnd, SB_VERT, need_scroll);
                let _ = InvalidateRect(Some(self.hwnd), None, true);
            }
        }

        unsafe {
            let max_pos = (self.content_height - client_h).max(0);
            self.scroll_pos = self.scroll_pos.clamp(0, max_pos);
            let info = SCROLLINFO {
                cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
                nMin: 0,
                nMax: self.content_height.max(1),
                nPage: client_h.max(1) as u32,
                nPos: self.scroll_pos,
                nTrackPos: 0,
            };
            SetScrollInfo(self.hwnd, SB_VERT, &info, true);
        }
    }

    fn on_mouse_move(&mut self, x: i32, y: i32) {
        let cx = x as f32;
        let cy = y as f32 + self.scroll_pos as f32;
        let hovered = self.action_kind != ActionKind::None && self.action_bounds.contains(cx, cy);
        if hovered != self.action_hovered {
            self.action_hovered = hovered;
            unsafe {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
            }
        }
    }

    fn take_action(&mut self, x: i32, y: i32) -> Option<ActionKind> {
        let cx = x as f32;
        let cy = y as f32 + self.scroll_pos as f32;
        if self.action_kind == ActionKind::None {
            return None;
        }
        if self.action_bounds.contains(cx, cy) {
            Some(self.action_kind)
        } else {
            None
        }
    }

    fn scroll_by(&mut self, delta: i32) {
        let (_, client_h) = self.client_size();
        let max_pos = (self.content_height - client_h).max(0);
        let new_pos = (self.scroll_pos + delta).clamp(0, max_pos);
        if new_pos != self.scroll_pos {
            unsafe {
                ScrollWindowEx(
                    self.hwnd,
                    0,
                    self.scroll_pos - new_pos,
                    None,
                    None,
                    None,
                    None,
                    SCROLL_WINDOW_FLAGS(0x0002), // SW_INVALIDATE
                );
                self.scroll_pos = new_pos;
                let info = SCROLLINFO {
                    cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                    fMask: SIF_POS,
                    nMin: 0,
                    nMax: 0,
                    nPage: 0,
                    nPos: self.scroll_pos,
                    nTrackPos: 0,
                };
                SetScrollInfo(self.hwnd, SB_VERT, &info, true);
                let _ = InvalidateRect(Some(self.hwnd), None, true);
            }
        }
    }

    fn scroll_to(&mut self, pos: i32) {
        let (_, client_h) = self.client_size();
        let max_pos = (self.content_height - client_h).max(0);
        self.scroll_by(pos.clamp(0, max_pos) - self.scroll_pos);
    }
}

unsafe fn state_of(hwnd: HWND) -> *mut ContentState {
    windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ContentState
}

pub unsafe extern "system" fn content_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = state_of(hwnd);

    match msg {
        WM_PAINT => {
            static PAINTED_ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !PAINTED_ONCE.swap(true, std::sync::atomic::Ordering::SeqCst) {
                crate::util::info("内容面板首次 WM_PAINT 绘制。");
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
        WM_VSCROLL => {
            if !state_ptr.is_null() {
                let code = (wparam.0 & 0xFFFF) as u32;
                let state = &mut *state_ptr;
                let (_, client_h) = state.client_size();
                let page = client_h.max(1);
                match code as i32 {
                    x if x == SB_LINEUP.0 => state.scroll_by(-30),
                    x if x == SB_LINEDOWN.0 => state.scroll_by(30),
                    x if x == SB_PAGEUP.0 => state.scroll_by(-page),
                    x if x == SB_PAGEDOWN.0 => state.scroll_by(page),
                    x if x == SB_THUMBTRACK.0 || x == SB_THUMBPOSITION.0 => {
                        let mut info = SCROLLINFO {
                            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                            fMask: SIF_ALL,
                            ..Default::default()
                        };
                        if GetScrollInfo(hwnd, SB_VERT, &mut info).is_ok() {
                            state.scroll_to(info.nTrackPos);
                        }
                    }
                    _ => {}
                }
            }
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            if !state_ptr.is_null() {
                let delta = (wparam.0 >> 16) as u16 as i16 as i32;
                (*state_ptr).scroll_by(-delta * 90 / 120);
            }
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            if !state_ptr.is_null() {
                // 对应 C# OnMouseEnter → Focus()：悬停即获得焦点，滚轮/Esc 生效。
                if GetFocus() != hwnd {
                    let _ = SetFocus(Some(hwnd));
                }
                let x = (lparam.0 & 0xFFFF) as u16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i32;
                (*state_ptr).on_mouse_move(x, y);
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
                if state.action_hovered {
                    state.action_hovered = false;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            if !state_ptr.is_null() {
                let x = (lparam.0 & 0xFFFF) as u16 as i32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i32;
                match (*state_ptr).take_action(x, y) {
                    Some(ActionKind::None) | None => {}
                    Some(ActionKind::AddServer) => {
                        super::app::with_app(|app| app.show_add_server());
                    }
                    Some(ActionKind::Retry) => {
                        if let Some(server_id) = (*state_ptr).server.as_ref().map(|s| s.id) {
                            super::app::with_app(|app| app.retry_server(server_id));
                        }
                    }
                    None => {}
                }
            }
            return LRESULT(0);
        }
        WM_MOUSEACTIVATE => return LRESULT(MA_ACTIVATE as isize),
        WM_KEYDOWN => {
            if wparam.0 as u32 == 0x1B && !state_ptr.is_null() {
                if let Ok(parent) = windows::Win32::UI::WindowsAndMessaging::GetParent(hwnd) {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        Some(parent),
                        WM_APP_ESCAPE,
                        WPARAM(0),
                        LPARAM(0),
                    );
                    return LRESULT(1);
                }
            }
        }
        _ => {}
    }

    DefWindowProcW(hwnd, msg, wparam, lparam)
}

impl ContentState {
    fn paint(&mut self, hdc: windows::Win32::Graphics::Gdi::HDC) {
        gfx::double_buffer_paint(self.hwnd, hdc, |mem_dc, width, _height| {
            let Some(g) = Graphics::from_hdc(mem_dc) else {
                return;
            };
            g.clear(theme::WINDOW_BACKGROUND);
            g.set_smoothing_anti_alias(true);
            g.set_text_hint_anti_alias_grid_fit();
            g.translate_transform(0.0, -(self.scroll_pos as f32));

            let x = self.s(14) as f32;
            // 客户区已排除 WS_VSCROLL 滚动条区域，这里只扣左右边距 → 左右各 14px 对称。
            let width = (self.s(200).max(width - self.s(14) * 2)) as f32;
            let mut y = self.s(8) as f32;
            self.action_bounds = Rect::new(0.0, 0.0, 0.0, 0.0);
            self.action_kind = ActionKind::None;

            if self.server.is_none() {
                self.draw_empty(&g, mem_dc);
                return;
            }

            let latest = self.fresh_latest();
            self.draw_header(&g, x, &mut y, width, latest.as_deref());
            let Some(latest) = latest else {
                self.draw_no_data_card(&g, x, y, width);
                return;
            };

            let phase = self.state.as_ref().map(|s| s.phase);
            if phase == Some(MonitorPhase::Error)
                && self
                    .state
                    .as_ref()
                    .and_then(|s| s.error.as_ref())
                    .is_some()
            {
                let message = self
                    .state
                    .as_ref()
                    .and_then(|s| s.error.as_ref().map(|e| e.message.clone()))
                    .map(|message| super::i18n::localize_backend(&message))
                    .unwrap_or_default();
                self.draw_error_banner(&g, x, &mut y, width, &message);
            }
            if latest.warning.as_deref().is_some_and(|w| !w.trim().is_empty()) {
                let warning = super::i18n::localize_backend(latest.warning.as_deref().unwrap_or(""));
                self.draw_warning_banner(&g, x, &mut y, width, &warning);
            }
            if let Some(ram) = &latest.ram {
                self.draw_ram_card(&g, x, &mut y, width, ram);
            }

            let gpu_area_start_y = y;
            let mut sorted_gpus: Vec<GpuTelemetry> = latest.gpus.clone();
            sorted_gpus.sort_by_key(|gpu| gpu.index);
            for gpu in &sorted_gpus {
                self.draw_gpu_card(&g, x, &mut y, width, gpu);
            }
            let minimum_gpu_area_height = (3 * (self.s(158) + self.s(10))) as f32;
            y = y.max(gpu_area_start_y + minimum_gpu_area_height);
            self.draw_processes(&g, x, &mut y, width, &latest);
        });
    }

    fn font(&mut self, spec: FontSpec) -> super::gfx::GpFontHandle {
        self.fonts.gdiplus_font(&spec).unwrap_or(std::ptr::null_mut())
    }

    fn draw_header(&mut self, g: &Graphics, x: f32, y: &mut f32, width: f32, latest: Option<&ServerSnapshot>) {
        let server = self.server.clone().expect("server present");
        g.draw_string(
            &server.display_user_host(),
            self.font(FontSpec::ui_bold(9.5)),
            theme::TEXT,
            Rect::new(x, *y, width - self.s(112) as f32, self.s(22) as f32),
            TextFormat { end_ellipsis: true, ..TextFormat::LEFT_VCENTER },
        );

        let phase = self.state.as_ref().map(|s| s.phase).unwrap_or(MonitorPhase::Stopped);
        self.draw_status_pill(
            g,
            Rect::new(x + width - self.s(104) as f32, *y, self.s(104) as f32, self.s(22) as f32),
            phase,
        );

        let t = super::i18n::t();
        let n_text = self.refresh_seconds.to_string();
        let subtitle = match latest {
            None => super::i18n::fmt(t.content_refresh_wait, &[("n", &n_text)]),
            Some(snapshot) => {
                let time = theme::format_clock(snapshot.captured_at);
                super::i18n::fmt(t.content_refresh_updated, &[("n", &n_text), ("time", &time)])
            }
        };
        g.draw_string(
            &subtitle,
            self.font(FontSpec::ui(8.0)),
            theme::MUTED_TEXT,
            Rect::new(x, *y + self.s(23) as f32, width, self.s(16) as f32),
            TextFormat { end_ellipsis: true, ..TextFormat::LEFT_VCENTER },
        );

        *y += self.s(46) as f32;
        g.draw_line(theme::BORDER, 1.0, x, *y, x + width, *y);
        *y += self.s(10) as f32;
    }

    fn draw_status_pill(&mut self, g: &Graphics, bounds: Rect, phase: MonitorPhase) {
        let color = theme::phase_color(phase);
        g.fill_rounded(theme::phase_soft(phase), bounds, bounds.h / 2.0);
        theme::draw_status_dot(
            g,
            bounds.x + self.s(12) as f32,
            bounds.y + bounds.h / 2.0,
            self.s(6) as f32,
            color,
        );
        g.draw_string(
            theme::phase_label(phase),
            self.font(FontSpec::ui(8.0)),
            color,
            Rect::new(bounds.x + self.s(21) as f32, bounds.y, bounds.w - self.s(27) as f32, bounds.h),
            TextFormat::LEFT_VCENTER,
        );
    }

    fn draw_error_banner(&mut self, g: &Graphics, x: f32, y: &mut f32, width: f32, message: &str) {
        let bounds = Rect::new(x, *y, width, self.s(34) as f32);
        g.fill_rounded(theme::RED_SOFT, bounds, self.s(8) as f32);
        g.draw_string(
            theme::GLYPH_WARNING,
            self.font(FontSpec::icon(9.0)),
            theme::RED,
            Rect::new(x + self.s(9) as f32, *y, self.s(24) as f32, bounds.h),
            TextFormat::CENTER,
        );
        let retry_width = self.s(42) as f32;
        g.draw_string(
            &super::i18n::fmt(super::i18n::t().content_error_banner, &[("message", message)]),
            self.font(FontSpec::ui(8.0)),
            theme::RED,
            Rect::new(
                x + self.s(33) as f32,
                *y,
                width - self.s(33) as f32 - retry_width - self.s(12) as f32,
                bounds.h,
            ),
            TextFormat { end_ellipsis: true, ..TextFormat::LEFT_VCENTER },
        );
        let retry_rect = Rect::new(x + width - retry_width - self.s(10) as f32, *y, retry_width, bounds.h);
        g.draw_string(
            super::i18n::t().btn_retry,
            self.font(FontSpec::ui(8.0)),
            if self.action_hovered { theme::ACCENT_DARK } else { theme::ACCENT },
            retry_rect,
            TextFormat::CENTER,
        );
        self.action_bounds = retry_rect;
        self.action_kind = ActionKind::Retry;
        *y += bounds.h + self.s(8) as f32;
    }

    fn draw_warning_banner(&mut self, g: &Graphics, x: f32, y: &mut f32, width: f32, message: &str) {
        let bounds = Rect::new(x, *y, width, self.s(32) as f32);
        g.fill_rounded(theme::AMBER_SOFT, bounds, self.s(8) as f32);
        g.draw_string(
            theme::GLYPH_WARNING,
            self.font(FontSpec::icon(9.0)),
            theme::AMBER,
            Rect::new(x + self.s(9) as f32, *y, self.s(24) as f32, bounds.h),
            TextFormat::CENTER,
        );
        g.draw_string(
            message,
            self.font(FontSpec::ui(8.0)),
            theme::AMBER,
            Rect::new(x + self.s(33) as f32, *y, width - self.s(45) as f32, bounds.h),
            TextFormat { end_ellipsis: true, ..TextFormat::LEFT_VCENTER },
        );
        *y += bounds.h + self.s(8) as f32;
    }

    fn draw_ram_card(&mut self, g: &Graphics, x: f32, y: &mut f32, width: f32, ram: &RamTelemetry) {
        let bounds = Rect::new(x, *y, width, self.s(52) as f32);
        theme::draw_card(g, bounds, self.s(10) as f32);

        g.draw_string(
            super::i18n::t().content_ram,
            self.font(FontSpec::ui_bold(9.0)),
            theme::TEXT,
            Rect::new(x + self.s(12) as f32, *y + self.s(8) as f32, self.s(60) as f32, self.s(18) as f32),
            TextFormat::LEFT_VCENTER,
        );
        g.draw_string(
            &format!(
                "{} / {}",
                theme::format_memory(Some(ram.used_mib)),
                theme::format_memory(Some(ram.total_mib))
            ),
            self.font(FontSpec::ui(8.0)),
            theme::SECONDARY_TEXT,
            Rect::new(x + self.s(58) as f32, *y + self.s(8) as f32, width - self.s(130) as f32, self.s(18) as f32),
            TextFormat { end_ellipsis: true, ..TextFormat::LEFT_VCENTER },
        );
        g.draw_string(
            &theme::format_percent(Some(ram.percent())),
            self.font(FontSpec::ui_bold(9.0)),
            theme::TEXT,
            Rect::new(x + width - self.s(64) as f32, *y + self.s(8) as f32, self.s(52) as f32, self.s(18) as f32),
            TextFormat { right: true, vcenter: true, ..Default::default() },
        );

        let bar = Rect::new(x + self.s(12) as f32, *y + self.s(33) as f32, width - self.s(24) as f32, self.s(6) as f32);
        theme::draw_usage_bar(g, bar, Some(ram.percent()), theme::memory_color(Some(ram.percent())));

        *y += bounds.h + self.s(10) as f32;
    }

    fn draw_gpu_card(&mut self, g: &Graphics, x: f32, y: &mut f32, width: f32, gpu: &GpuTelemetry) {
        let height = self.s(158) as f32;
        let card = Rect::new(x, *y, width, height);
        theme::draw_card(g, card, self.s(10) as f32);

        let inner_x = x + self.s(12) as f32;
        let inner_y = *y + self.s(10) as f32;
        let inner_width = width - self.s(24) as f32;

        let mut extras: Vec<String> = Vec::new();
        if let Some(temperature) = gpu.temperature_c {
            extras.push(format!("{temperature:.0} °C"));
        }
        if let Some(power) = gpu.power_w {
            extras.push(format!("{power:.0} W"));
        }
        let extra_text = extras.join(" · ");
        let extra_width = if extra_text.is_empty() {
            0.0
        } else {
            g.measure_string(&extra_text, self.font(FontSpec::ui(8.0))).0 + self.s(4) as f32
        };

        let badge = Rect::new(inner_x, inner_y, self.s(44) as f32, self.s(18) as f32);
        g.fill_rounded(theme::ACCENT_SOFT, badge, self.s(6) as f32);
        g.draw_string(&format!("GPU {}", gpu.index), self.font(FontSpec::ui(8.0)), theme::ACCENT, badge, TextFormat::CENTER);

        if !extra_text.is_empty() {
            g.draw_string(
                &extra_text,
                self.font(FontSpec::ui(8.0)),
                theme::SECONDARY_TEXT,
                Rect::new(inner_x + inner_width - extra_width, inner_y, extra_width, self.s(18) as f32),
                TextFormat { right: true, vcenter: true, ..Default::default() },
            );
        }

        g.draw_string(
            &gpu.name,
            self.font(FontSpec::ui_bold(9.25)),
            theme::TEXT,
            Rect::new(
                inner_x + self.s(52) as f32,
                inner_y,
                inner_width - self.s(52) as f32 - extra_width - self.s(8) as f32,
                self.s(18) as f32,
            ),
            TextFormat { end_ellipsis: true, ..TextFormat::LEFT_VCENTER },
        );

        // 显存行
        g.draw_string(
            super::i18n::t().content_vram,
            self.font(FontSpec::ui(8.5)),
            theme::SECONDARY_TEXT,
            Rect::new(inner_x, inner_y + self.s(27) as f32, self.s(46) as f32, self.s(16) as f32),
            TextFormat::LEFT_VCENTER,
        );
        g.draw_string(
            &format!(
                "{} / {}",
                theme::format_memory(gpu.memory_used_mib),
                theme::format_memory(gpu.memory_total_mib)
            ),
            self.font(FontSpec::ui(8.0)),
            theme::SECONDARY_TEXT,
            Rect::new(
                inner_x + self.s(50) as f32,
                inner_y + self.s(27) as f32,
                inner_width - self.s(50) as f32 - self.s(54) as f32,
                self.s(16) as f32,
            ),
            TextFormat { end_ellipsis: true, ..TextFormat::LEFT_VCENTER },
        );
        g.draw_string(
            &theme::format_percent(gpu.memory_percent()),
            self.font(FontSpec::ui_bold(9.0)),
            theme::TEXT,
            Rect::new(inner_x + inner_width - self.s(46) as f32, inner_y + self.s(27) as f32, self.s(46) as f32, self.s(16) as f32),
            TextFormat { right: true, vcenter: true, ..Default::default() },
        );
        let mem_bar = Rect::new(
            inner_x + self.s(50) as f32,
            inner_y + self.s(47) as f32,
            inner_width - self.s(50) as f32 - self.s(54) as f32,
            self.s(6) as f32,
        );
        theme::draw_usage_bar(g, mem_bar, gpu.memory_percent(), theme::memory_color(gpu.memory_percent()));

        // 利用率行
        g.draw_string(
            super::i18n::t().content_util,
            self.font(FontSpec::ui(8.5)),
            theme::SECONDARY_TEXT,
            Rect::new(inner_x, inner_y + self.s(58) as f32, self.s(46) as f32, self.s(16) as f32),
            TextFormat::LEFT_VCENTER,
        );
        g.draw_string(
            &theme::format_percent(gpu.utilization_percent),
            self.font(FontSpec::ui_bold(9.0)),
            theme::TEXT,
            Rect::new(inner_x + inner_width - self.s(46) as f32, inner_y + self.s(58) as f32, self.s(46) as f32, self.s(16) as f32),
            TextFormat { right: true, vcenter: true, ..Default::default() },
        );
        let util_bar = Rect::new(
            inner_x + self.s(50) as f32,
            inner_y + self.s(62) as f32,
            inner_width - self.s(50) as f32 - self.s(54) as f32,
            self.s(6) as f32,
        );
        theme::draw_usage_bar(g, util_bar, gpu.utilization_percent, theme::ACCENT);

        // 走势图
        let chart = Rect::new(inner_x, inner_y + self.s(82) as f32, inner_width, self.s(44) as f32);
        let history: Vec<Arc<ServerSnapshot>> = self
            .state
            .as_ref()
            .map(|s| s.history.clone())
            .unwrap_or_default();
        self.draw_sparkline(g, chart, &history, gpu.index);
        self.draw_legend(g, inner_x, inner_y + self.s(132) as f32, inner_width);

        *y += height + self.s(10) as f32;
    }

    /// Sparkline.Draw 的移植：300 秒窗口双线 + 渐变面积。
    fn draw_sparkline(
        &mut self,
        g: &Graphics,
        bounds: Rect,
        history: &[Arc<ServerSnapshot>],
        gpu_index: i32,
    ) {
        let state = g.save_state();
        g.set_smoothing_anti_alias(true);
        g.set_pixel_offset_high_quality();

        g.fill_rounded(theme::CHART_BACKGROUND, bounds, 8.0);
        let plot = Rect::new(bounds.x + 8.0, bounds.y + 6.0, bounds.w - 16.0, bounds.h - 12.0);
        for i in 0..=2 {
            let y = plot.y + plot.h * i as f32 / 2.0;
            g.draw_line(theme::CHART_GRID, 1.0, plot.x, y, plot.right(), y);
        }

        let window_start = self.now - queue::retention();
        let mut utilization_points: Vec<(f32, f32)> = Vec::new();
        let mut memory_points: Vec<(f32, f32)> = Vec::new();

        for snapshot in history {
            if snapshot.captured_at < window_start
                || snapshot.captured_at > self.now + chrono::Duration::seconds(1)
            {
                continue;
            }
            let Some(gpu) = snapshot.gpus.iter().find(|gpu| gpu.index == gpu_index) else {
                continue;
            };
            let seconds = snapshot
                .captured_at
                .signed_duration_since(window_start)
                .num_milliseconds() as f64
                / 1000.0;
            let x = plot.x + (seconds / RETENTION_SECS as f64) as f32 * plot.w;
            let value_to_y =
                |percent: f64| plot.bottom() - (percent.clamp(0.0, 100.0) as f32 / 100.0) * plot.h;
            if let Some(utilization) = gpu.utilization_percent {
                utilization_points.push((x, value_to_y(utilization)));
            }
            if let Some(memory) = gpu.memory_percent() {
                memory_points.push((x, value_to_y(memory)));
            }
        }

        if utilization_points.len() < 2 && memory_points.len() < 2 {
            g.draw_string(super::i18n::t().content_chart_sampling, self.font(FontSpec::ui(8.0)), theme::MUTED_TEXT, plot, TextFormat::CENTER);
            g.restore_state(state);
            return;
        }

        let clip_state = g.save_state();
        g.set_clip_rounded(bounds, 8.0);

        if utilization_points.len() >= 2 {
            g.fill_gradient_area(
                Rect::new(plot.x, plot.y, plot.w, plot.h),
                &utilization_points,
                gfx::with_alpha(theme::ACCENT, 42),
                gfx::with_alpha(theme::ACCENT, 2),
            );
        }

        g.draw_polyline(&utilization_points, theme::ACCENT, 1.7);
        g.draw_polyline(&memory_points, theme::PURPLE, 1.4);
        if let Some(&(x, y)) = utilization_points.last() {
            g.fill_ellipse(theme::ACCENT, Rect::new(x - 2.0, y - 2.0, 4.0, 4.0));
        }
        if let Some(&(x, y)) = memory_points.last() {
            g.fill_ellipse(theme::PURPLE, Rect::new(x - 2.0, y - 2.0, 4.0, 4.0));
        }

        g.restore_state(clip_state);
        g.restore_state(state);
    }

    fn draw_legend(&mut self, g: &Graphics, x: f32, y: f32, width: f32) {
        let t = super::i18n::t();
        theme::draw_status_dot(g, x + self.s(3) as f32, y + self.s(6) as f32, self.s(6) as f32, theme::ACCENT);
        g.draw_string(
            t.content_util,
            self.font(FontSpec::ui(8.0)),
            theme::SECONDARY_TEXT,
            Rect::new(x + self.s(10) as f32, y, self.s(44) as f32, self.s(12) as f32),
            TextFormat::LEFT_VCENTER,
        );
        theme::draw_status_dot(g, x + self.s(57) as f32, y + self.s(6) as f32, self.s(6) as f32, theme::PURPLE);
        g.draw_string(
            t.content_vram,
            self.font(FontSpec::ui(8.0)),
            theme::SECONDARY_TEXT,
            Rect::new(x + self.s(64) as f32, y, self.s(44) as f32, self.s(12) as f32),
            TextFormat::LEFT_VCENTER,
        );
        g.draw_string(
            t.content_legend_5min,
            self.font(FontSpec::ui(8.0)),
            theme::MUTED_TEXT,
            Rect::new(x, y, width, self.s(12) as f32),
            TextFormat { right: true, vcenter: true, ..Default::default() },
        );
    }

    fn draw_processes(&mut self, g: &Graphics, x: f32, y: &mut f32, width: f32, snapshot: &ServerSnapshot) {
        let t = super::i18n::t();
        g.draw_string(
            t.content_processes_title,
            self.font(FontSpec::ui_bold(10.0)),
            theme::TEXT,
            Rect::new(x, *y, width, self.s(20) as f32),
            TextFormat::LEFT_VCENTER,
        );
        let count_text = if snapshot.processes.is_empty() {
            t.content_processes_none.to_string()
        } else {
            snapshot.processes.len().to_string()
        };
        g.draw_string(
            &super::i18n::fmt(t.content_processes_count, &[("n", &count_text)]),
            self.font(FontSpec::ui(8.0)),
            theme::MUTED_TEXT,
            Rect::new(x, *y, width, self.s(20) as f32),
            TextFormat { right: true, vcenter: true, ..Default::default() },
        );
        *y += self.s(24) as f32;

        let row_height = self.s(24) as f32;
        let rows = snapshot.processes.len().max(1) as f32;
        let card_height = self.s(10) as f32 + rows * row_height + self.s(10) as f32;
        let card = Rect::new(x, *y, width, card_height);
        theme::draw_card(g, card, self.s(10) as f32);

        if snapshot.processes.is_empty() {
            g.draw_string(
                super::i18n::t().content_processes_empty,
                self.font(FontSpec::ui(8.0)),
                theme::MUTED_TEXT,
                Rect::new(x + self.s(12) as f32, *y + self.s(10) as f32, width - self.s(24) as f32, row_height),
                TextFormat::LEFT_VCENTER,
            );
            *y += card_height + self.s(10) as f32;
            return;
        }

        let mut row_y = *y + self.s(10) as f32;
        for (index, process) in snapshot.processes.iter().enumerate() {
            if index > 0 {
                g.draw_line(theme::TRACK, 1.0, x + self.s(10) as f32, row_y, x + width - self.s(10) as f32, row_y);
            }

            let badge = Rect::new(x + self.s(12) as f32, row_y + self.s(4) as f32, self.s(24) as f32, self.s(16) as f32);
            g.fill_rounded(theme::ACCENT_SOFT, badge, self.s(5) as f32);
            g.draw_string(&format!("G{}", process.gpu_index), self.font(FontSpec::ui(8.0)), theme::ACCENT, badge, TextFormat::CENTER);
            g.draw_string(
                &process.pid.to_string(),
                self.font(FontSpec::mono(8.25)),
                theme::SECONDARY_TEXT,
                Rect::new(x + self.s(44) as f32, row_y, self.s(58) as f32, row_height),
                TextFormat::LEFT_VCENTER,
            );

            let memory_width = self.s(62) as f32;
            g.draw_string(
                &theme::format_memory(process.memory_mib),
                self.font(FontSpec::ui(8.0)),
                theme::SECONDARY_TEXT,
                Rect::new(x + width - memory_width - self.s(12) as f32, row_y, memory_width, row_height),
                TextFormat { right: true, vcenter: true, ..Default::default() },
            );
            g.draw_string(
                &process.command,
                self.font(FontSpec::ui(8.0)),
                theme::TEXT,
                Rect::new(x + self.s(106) as f32, row_y, width - self.s(106) as f32 - memory_width - self.s(20) as f32, row_height),
                TextFormat { end_ellipsis: true, ..TextFormat::LEFT_VCENTER },
            );

            row_y += row_height;
        }

        *y += card_height + self.s(10) as f32;
    }

    fn draw_no_data_card(&mut self, g: &Graphics, x: f32, y: f32, width: f32) {
        let bounds = Rect::new(x, y, width, self.s(148) as f32);
        theme::draw_card(g, bounds, self.s(10) as f32);

        let phase = self.state.as_ref().map(|s| s.phase).unwrap_or(MonitorPhase::Stopped);
        let error = self.state.as_ref().and_then(|s| s.error.clone());
        let is_error = phase == MonitorPhase::Error && error.is_some();
        let is_stale = self.state.as_ref().is_some_and(|s| s.latest().is_some());
        let color = if is_error {
            theme::RED
        } else if phase == MonitorPhase::Connecting {
            theme::AMBER
        } else {
            theme::ACCENT
        };
        let soft = if is_error {
            theme::RED_SOFT
        } else if phase == MonitorPhase::Connecting {
            theme::AMBER_SOFT
        } else {
            theme::ACCENT_SOFT
        };

        let icon_bounds = Rect::new(x + self.s(16) as f32, y + self.s(18) as f32, self.s(34) as f32, self.s(34) as f32);
        g.fill_rounded(soft, icon_bounds, self.s(10) as f32);
        g.draw_string(
            if is_error { theme::GLYPH_ERROR } else { theme::GLYPH_REFRESH },
            self.font(FontSpec::icon(9.0)),
            color,
            icon_bounds,
            TextFormat::CENTER,
        );

        let t = super::i18n::t();
        let title = if is_error {
            t.content_nodata_error
        } else if is_stale {
            t.content_nodata_stale
        } else if phase == MonitorPhase::Connecting {
            t.content_nodata_connecting
        } else {
            t.content_nodata_reading
        };
        let message = if is_error {
            error
                .as_ref()
                .map(|e| super::i18n::localize_backend(&e.message))
                .unwrap_or_default()
        } else if is_stale {
            t.content_stale_body.to_string()
        } else if phase == MonitorPhase::Connecting {
            t.content_connecting_body.to_string()
        } else {
            t.content_first_body.to_string()
        };

        g.draw_string(
            title,
            self.font(FontSpec::ui_bold(9.0)),
            theme::TEXT,
            Rect::new(x + self.s(62) as f32, y + self.s(16) as f32, width - self.s(80) as f32, self.s(22) as f32),
            TextFormat { end_ellipsis: true, ..TextFormat::LEFT_VCENTER },
        );
        g.draw_string(
            &message,
            self.font(FontSpec::ui(8.0)),
            theme::SECONDARY_TEXT,
            Rect::new(x + self.s(62) as f32, y + self.s(41) as f32, width - self.s(80) as f32, self.s(38) as f32),
            TextFormat::LEFT_TOP_WRAP,
        );

        let detail = if is_error {
            error.as_ref().and_then(|e| e.detail.clone())
        } else {
            None
        };
        if detail.as_deref().is_some_and(|d| !d.trim().is_empty()) {
            g.draw_string(
                detail.as_deref().unwrap_or(""),
                self.font(FontSpec::ui(8.0)),
                theme::MUTED_TEXT,
                Rect::new(x + self.s(62) as f32, y + self.s(80) as f32, width - self.s(80) as f32, self.s(18) as f32),
                TextFormat { end_ellipsis: true, ..TextFormat::LEFT_VCENTER },
            );
        }

        let button = Rect::new(x + self.s(62) as f32, y + self.s(104) as f32, self.s(86) as f32, self.s(28) as f32);
        self.draw_button(g, button, super::i18n::t().btn_retry, false);
        self.action_bounds = button;
        self.action_kind = ActionKind::Retry;
    }

    fn draw_empty(&mut self, g: &Graphics, hdc: windows::Win32::Graphics::Gdi::HDC) {
        if self.app_icon.is_none() {
            self.app_icon = super::tray::load_app_icon_sized(self.s(58));
        }
        let (client_w, client_h) = self.client_size();

        let group_height = self.s(204) as f32;
        let top = (self.s(18) as f32).max((client_h as f32 - group_height) / 2.0);

        if let Some(icon) = &self.app_icon {
            let icon_size = self.s(58);
            let x = (client_w - icon_size) / 2;
            unsafe {
                DrawIconEx(
                    hdc,
                    x,
                    top as i32,
                    icon.raw(),
                    icon_size,
                    icon_size,
                    0,
                    None,
                    // DI_NORMAL = DI_IMAGE|DI_MASK = 0x3；0x4 是 DI_COMPAT，单独用什么都画不出来。
                    windows::Win32::UI::WindowsAndMessaging::DI_FLAGS(0x3),
                );
            }
        }

        let t = super::i18n::t();
        g.draw_string(
            t.content_empty_title,
            self.font(FontSpec::ui_bold(12.0)),
            theme::TEXT,
            Rect::new(self.s(16) as f32, top + self.s(74) as f32, client_w as f32 - self.s(32) as f32, self.s(28) as f32),
            TextFormat::CENTER,
        );
        g.draw_string(
            t.content_empty_body,
            self.font(FontSpec::ui(8.5)),
            theme::SECONDARY_TEXT,
            Rect::new(self.s(40) as f32, top + self.s(106) as f32, client_w as f32 - self.s(80) as f32, self.s(36) as f32),
            TextFormat { hcenter: true, word_break: true, ..Default::default() },
        );

        let button = Rect::new(
            (client_w as f32 - self.s(124) as f32) / 2.0,
            top + self.s(154) as f32,
            self.s(124) as f32,
            self.s(32) as f32,
        );
        self.draw_button(g, button, super::i18n::t().btn_add_server, true);
        self.action_bounds = button;
        self.action_kind = ActionKind::AddServer;
    }

    fn draw_button(&mut self, g: &Graphics, bounds: Rect, text: &str, primary: bool) {
        let background = if primary {
            if self.action_hovered { theme::ACCENT_DARK } else { theme::ACCENT }
        } else if self.action_hovered {
            theme::HOVER
        } else {
            theme::CARD_BACKGROUND
        };
        g.fill_rounded(background, bounds, self.s(8) as f32);
        if !primary {
            g.draw_rounded(
                theme::BORDER_STRONG,
                1.0,
                Rect::new(bounds.x + 0.5, bounds.y + 0.5, bounds.w - 1.0, bounds.h - 1.0),
                self.s(8) as f32,
            );
        }
        g.draw_string(
            text,
            self.font(FontSpec::ui(8.5)),
            if primary { theme::WHITE } else { theme::TEXT },
            bounds,
            TextFormat::CENTER,
        );
    }
}
