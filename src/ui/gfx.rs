//! GDI+/GDI 绘图封装：模拟 C# System.Drawing 的 Graphics/Font/Brush/Pen 用法。
//! 文本两条路径与 C# 一一对应：
//! - GDI+ DrawString/MeasureString（对应 Drawing.DrawText / Drawing.MeasureText）
//! - GDI DrawText（对应 TextRenderer.DrawText/MeasureText，用于 TabStrip 与图标字形）

use std::collections::HashMap;
use std::sync::OnceLock;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateFontIndirectW, DeleteDC, DrawTextW, GetDC,
    GetDeviceCaps, ReleaseDC, SelectObject, SetBkMode, SetTextColor, DT_CALCRECT, DT_CENTER,
    DT_EDITCONTROL, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE,
    DT_VCENTER, DT_WORDBREAK, HDC, HFONT, HGDIOBJ, LOGFONTW, LOGPIXELSY, TRANSPARENT,
};
use windows::Win32::Graphics::GdiPlus::*;
use windows::Win32::UI::WindowsAndMessaging::USER_DEFAULT_SCREEN_DPI;

pub type Argb = u32;

pub type GpFontHandle = *mut GpFont;
pub type GpPathHandle = *mut GpPath;
pub type GpFontFamilyHandle = *mut GpFontFamily;

#[allow(non_snake_case)]
pub type GpRectF = RectF;

#[allow(non_snake_case)]
pub type GpPointF = PointF;

fn status_ok(status: Status) -> bool {
    status.0 == 0
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    fn to_gp(&self) -> GpRectF {
        GpRectF { X: self.x, Y: self.y, Width: self.w, Height: self.h }
    }
}

pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> Argb {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

pub const fn rgb(r: u8, g: u8, b: u8) -> Argb {
    argb(255, r, g, b)
}

pub const fn with_alpha(color: Argb, alpha: u8) -> Argb {
    (color & 0x00FF_FFFF) | ((alpha as u32) << 24)
}

/// 进程级 GDI+ 初始化（对应 C# 程序启动时自动完成的 GdiplusStartup）。
pub fn startup_gdiplus() {
    static START: OnceLock<()> = OnceLock::new();
    START.get_or_init(|| unsafe {
        let mut token: usize = 0;
        let mut input = GdiplusStartupInput::default();
        input.GdiplusVersion = 1;
        GdiplusStartup(&mut token, &input, std::ptr::null_mut());
    });
}

/// 字体规格：族名 + 磅值（1/8 磅精度，对应 9.25/8.75pt）+ 粗体。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontSpec {
    pub family: &'static str,
    pub eighths: u32,
    pub bold: bool,
}

impl FontSpec {
    fn new(family: &'static str, points: f32, bold: bool) -> Self {
        FontSpec { family, eighths: (points * 8.0).round() as u32, bold }
    }

    pub fn ui(points: f32) -> Self {
        Self::new("Microsoft YaHei UI", points, false)
    }

    pub fn ui_bold(points: f32) -> Self {
        Self::new("Microsoft YaHei UI", points, true)
    }

    pub fn icon(points: f32) -> Self {
        Self::new("Segoe MDL2 Assets", points, false)
    }

    pub fn mono(points: f32) -> Self {
        Self::new("Consolas", points, false)
    }

    pub fn point_value(&self) -> f32 {
        self.eighths as f32 / 8.0
    }
}

/// 字体缓存：按 DPI 缓存 GDI+ 字体与等价的 GDI HFONT。
pub struct FontCache {
    dpi: u32,
    gdiplus: HashMap<FontSpec, GpFontHandle>,
    gdi: HashMap<FontSpec, HFONT>,
    families: HashMap<&'static str, GpFontFamilyHandle>,
}

impl FontCache {
    pub fn new(dpi: u32) -> Self {
        FontCache {
            dpi,
            gdiplus: HashMap::new(),
            gdi: HashMap::new(),
            families: HashMap::new(),
        }
    }

    pub fn dpi(&self) -> u32 {
        self.dpi
    }

    fn family(&mut self, spec: &FontSpec) -> Option<GpFontFamilyHandle> {
        if let Some(existing) = self.families.get(spec.family) {
            return Some(*existing);
        }
        unsafe {
            let wide: Vec<u16> = spec.family.encode_utf16().chain(std::iter::once(0)).collect();
            let mut family: GpFontFamilyHandle = std::ptr::null_mut();
            let status = GdipCreateFontFamilyFromName(PCWSTR(wide.as_ptr()), std::ptr::null_mut(), &mut family);
            if status_ok(status) && !family.is_null() {
                self.families.insert(spec.family, family);
                Some(family)
            } else {
                None
            }
        }
    }

    pub fn gdiplus_font(&mut self, spec: &FontSpec) -> Option<GpFontHandle> {
        if let Some(existing) = self.gdiplus.get(spec) {
            return Some(*existing);
        }
        let family = self.family(spec)?;
        unsafe {
            let mut font: GpFontHandle = std::ptr::null_mut();
            let style = if spec.bold { FontStyleBold } else { FontStyle(0) };
            let status = GdipCreateFont(family, spec.point_value(), style.0, UnitPoint, &mut font);
            if status_ok(status) && !font.is_null() {
                self.gdiplus.insert(spec.clone(), font);
                Some(font)
            } else {
                None
            }
        }
    }

    /// 与 WinForms Font.ToHfont 等价：磅值按 DPI 换算像素高。
    pub fn gdi_font(&mut self, spec: &FontSpec) -> Option<HFONT> {
        if let Some(existing) = self.gdi.get(spec) {
            return Some(*existing);
        }
        let height = -(spec.point_value() * self.dpi as f32 / 72.0).round() as i32;
        unsafe {
            let mut logfont: LOGFONTW = std::mem::zeroed();
            logfont.lfHeight = height;
            logfont.lfWeight = if spec.bold { 700 } else { 400 };
            logfont.lfCharSet = windows::Win32::Graphics::Gdi::FONT_CHARSET(1); // DEFAULT_CHARSET
            for (dest, ch) in logfont.lfFaceName.iter_mut().zip(spec.family.encode_utf16()) {
                *dest = ch;
            }
            let font = CreateFontIndirectW(&logfont);
            if font.is_invalid() {
                return None;
            }
            self.gdi.insert(spec.clone(), font);
            Some(font)
        }
    }
}

// GDI+ 字符串格式参数（对应 C# Drawing.DrawText 里的 StringFormat 组装）。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TextFormat {
    pub hcenter: bool,
    pub right: bool,
    pub vcenter: bool,
    pub bottom: bool,
    pub end_ellipsis: bool,
    pub word_break: bool,
}

impl TextFormat {
    /// C# 默认：Left | VerticalCenter | NoPrefix
    pub const LEFT_VCENTER: TextFormat = TextFormat { hcenter: false, right: false, vcenter: true, bottom: false, end_ellipsis: false, word_break: false };
    /// C# TextRenderer: HorizontalCenter | VerticalCenter | NoPrefix
    pub const CENTER: TextFormat = TextFormat { hcenter: true, right: false, vcenter: true, bottom: false, end_ellipsis: false, word_break: false };
    pub const LEFT_TOP_WRAP: TextFormat = TextFormat { hcenter: false, right: false, vcenter: false, bottom: false, end_ellipsis: false, word_break: true };
}

fn make_string_format(format: TextFormat) -> Option<(*mut GpStringFormat, bool)> {
    unsafe {
        let mut sf: *mut GpStringFormat = std::ptr::null_mut();
        let mut flags: i32 = 0;
        if !format.word_break {
            flags |= StringFormatFlagsNoWrap.0;
        }
        if !status_ok(GdipCreateStringFormat(flags, 0, &mut sf)) {
            return None;
        }
        let alignment = if format.hcenter {
            StringAlignmentCenter
        } else if format.right {
            StringAlignmentFar
        } else {
            StringAlignmentNear
        };
        let line_alignment = if format.vcenter {
            StringAlignmentCenter
        } else if format.bottom {
            StringAlignmentFar
        } else {
            StringAlignmentNear
        };
        GdipSetStringFormatAlign(sf, alignment);
        GdipSetStringFormatLineAlign(sf, line_alignment);
        let trimming = if format.end_ellipsis {
            StringTrimmingEllipsisCharacter
        } else {
            StringTrimmingNone
        };
        GdipSetStringFormatTrimming(sf, trimming);
        Some((sf, true))
    }
}

/// Graphics 包装：不拥有 HDC（由调用方的兼容 DC 提供）。
pub struct Graphics {
    graphics: *mut GpGraphics,
}

impl Graphics {
    pub fn from_hdc(hdc: HDC) -> Option<Graphics> {
        startup_gdiplus();
        unsafe {
            let mut graphics: *mut GpGraphics = std::ptr::null_mut();
            if status_ok(GdipCreateFromHDC(hdc, &mut graphics)) {
                Some(Graphics { graphics })
            } else {
                None
            }
        }
    }

    pub fn raw(&self) -> *mut GpGraphics {
        self.graphics
    }

    pub fn set_smoothing_anti_alias(&self, on: bool) {
        unsafe {
            GdipSetSmoothingMode(self.graphics, if on { SmoothingModeAntiAlias } else { SmoothingModeDefault });
        }
    }

    pub fn set_pixel_offset_high_quality(&self) {
        unsafe {
            GdipSetPixelOffsetMode(self.graphics, PixelOffsetModeHighQuality);
        }
    }

    pub fn set_text_hint_anti_alias_grid_fit(&self) {
        unsafe {
            GdipSetTextRenderingHint(self.graphics, TextRenderingHintAntiAliasGridFit);
        }
    }

    pub fn clear(&self, color: Argb) {
        unsafe {
            GdipGraphicsClear(self.graphics, color);
        }
    }

    pub fn translate_transform(&self, dx: f32, dy: f32) {
        unsafe {
            GdipTranslateWorldTransform(self.graphics, dx, dy, MatrixOrderPrepend);
        }
    }

    pub fn save_state(&self) -> u32 {
        let mut state: u32 = 0;
        unsafe {
            GdipSaveGraphics(self.graphics, &mut state);
        }
        state
    }

    pub fn restore_state(&self, state: u32) {
        unsafe {
            GdipRestoreGraphics(self.graphics, state);
        }
    }

    pub fn set_clip_rounded(&self, bounds: Rect, radius: f32) {
        if let Some(path) = create_rounded_path(bounds, radius) {
            unsafe {
                GdipSetClipPath(self.graphics, path, CombineModeReplace);
                GdipDeletePath(path);
            }
        }
    }

    pub fn reset_clip(&self) {
        unsafe {
            GdipResetClip(self.graphics);
        }
    }

    pub fn fill_rounded(&self, color: Argb, bounds: Rect, radius: f32) {
        if let Some(path) = create_rounded_path(bounds, radius) {
            unsafe {
                let mut brush: *mut GpSolidFill = std::ptr::null_mut();
                if status_ok(GdipCreateSolidFill(color, &mut brush)) {
                    GdipFillPath(self.graphics, brush as *mut GpBrush, path);
                    GdipDeleteBrush(brush as *mut GpBrush);
                }
                GdipDeletePath(path);
            }
        }
    }

    pub fn draw_rounded(&self, color: Argb, width: f32, bounds: Rect, radius: f32) {
        if let Some(path) = create_rounded_path(bounds, radius) {
            unsafe {
                let mut pen: *mut GpPen = std::ptr::null_mut();
                if status_ok(GdipCreatePen1(color, width, UnitPixel, &mut pen)) {
                    GdipDrawPath(self.graphics, pen, path);
                    GdipDeletePen(pen);
                }
                GdipDeletePath(path);
            }
        }
    }

    pub fn draw_line(&self, color: Argb, width: f32, x1: f32, y1: f32, x2: f32, y2: f32) {
        unsafe {
            let mut pen: *mut GpPen = std::ptr::null_mut();
            if status_ok(GdipCreatePen1(color, width, UnitPixel, &mut pen)) {
                GdipDrawLine(self.graphics, pen, x1, y1, x2, y2);
                GdipDeletePen(pen);
            }
        }
    }

    pub fn fill_ellipse(&self, color: Argb, bounds: Rect) {
        unsafe {
            let mut brush: *mut GpSolidFill = std::ptr::null_mut();
            if status_ok(GdipCreateSolidFill(color, &mut brush)) {
                GdipFillEllipse(self.graphics, brush as *mut GpBrush, bounds.x, bounds.y, bounds.w, bounds.h);
                GdipDeleteBrush(brush as *mut GpBrush);
            }
        }
    }

    pub fn fill_ellipse_at(&self, color: Argb, cx: f32, cy: f32, d: f32) {
        self.fill_ellipse(color, Rect::new(cx - d / 2.0, cy - d / 2.0, d, d));
    }

    /// GDI+ 文本绘制（对应 Drawing.DrawText → Graphics.DrawString）。
    pub fn draw_string(
        &self,
        text: &str,
        font: GpFontHandle,
        color: Argb,
        bounds: Rect,
        format: TextFormat,
    ) {
        if text.is_empty() || bounds.w <= 0.0 || bounds.h <= 0.0 || font.is_null() {
            return;
        }

        let mut owned_text = text.to_string();
        if !format.word_break {
            owned_text = owned_text.replace("\r\n", " ").replace('\n', " ").replace('\r', " ");
        }

        let Some((sf, _)) = make_string_format(format) else {
            return;
        };
        let wide: Vec<u16> = owned_text.encode_utf16().collect();
        unsafe {
            let mut brush: *mut GpSolidFill = std::ptr::null_mut();
            if status_ok(GdipCreateSolidFill(color, &mut brush)) {
                let rect = bounds.to_gp();
                GdipDrawString(
                    self.graphics,
                    PCWSTR(wide.as_ptr()),
                    wide.len() as i32,
                    font,
                    &rect,
                    sf,
                    brush as *mut GpBrush,
                );
                GdipDeleteBrush(brush as *mut GpBrush);
            }
            GdipDeleteStringFormat(sf);
        }
    }

    /// GDI+ 文本测量（对应 Drawing.MeasureText → Graphics.MeasureString）。
    pub fn measure_string(&self, text: &str, font: GpFontHandle) -> (f32, f32) {
        if text.is_empty() || font.is_null() {
            return (0.0, 0.0);
        }
        let Some((sf, _)) = make_string_format(TextFormat { word_break: false, ..Default::default() }) else {
            return (0.0, 0.0);
        };
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let layout = GpRectF { X: 0.0, Y: 0.0, Width: 1_000_000.0, Height: 1_000_000.0 };
            let mut bbox = GpRectF::default();
            GdipMeasureString(
                self.graphics,
                PCWSTR(wide.as_ptr()),
                wide.len() as i32,
                font,
                &layout,
                sf,
                &mut bbox,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            GdipDeleteStringFormat(sf);
            (bbox.Width, bbox.Height)
        }
    }

    /// 面积图填充：折线 → 底边闭合的路径，竖直渐变（对应 Sparkline 的 LinearGradientBrush）。
    pub fn fill_gradient_area(
        &self,
        bounds: Rect,
        points: &[(f32, f32)],
        top_color: Argb,
        bottom_color: Argb,
    ) {
        if points.len() < 2 {
            return;
        }
        unsafe {
            let mut brush: *mut GpLineGradient = std::ptr::null_mut();
            let p1 = GpPointF { X: bounds.x, Y: bounds.y };
            let p2 = GpPointF { X: bounds.x, Y: bounds.bottom() };
            if !status_ok(GdipCreateLineBrush(&p1, &p2, top_color, bottom_color, WrapModeClamp, &mut brush)) {
                return;
            }

            let mut path: GpPathHandle = std::ptr::null_mut();
            if status_ok(GdipCreatePath(FillModeWinding, &mut path)) {
                GdipAddPathLine2(path, points.as_ptr() as *const GpPointF, points.len() as i32);
                let first = points[0];
                let last = points[points.len() - 1];
                GdipAddPathLine(path, last.0, last.1, last.0, bounds.bottom());
                GdipAddPathLine(path, last.0, bounds.bottom(), first.0, bounds.bottom());
                GdipClosePathFigure(path);
                GdipFillPath(self.graphics, brush as *mut GpBrush, path);
                GdipDeletePath(path);
            }
            GdipDeleteBrush(brush as *mut GpBrush);
        }
    }

    /// 折线（圆角连接、圆端点，对应 Sparkline DrawSeries 的 Pen 设置）。
    pub fn draw_polyline(&self, points: &[(f32, f32)], color: Argb, width: f32) {
        if points.is_empty() {
            return;
        }
        unsafe {
            let mut pen: *mut GpPen = std::ptr::null_mut();
            if !status_ok(GdipCreatePen1(color, width, UnitPixel, &mut pen)) {
                return;
            }
            GdipSetPenLineJoin(pen, LineJoinRound);
            GdipSetPenStartCap(pen, LineCapRound);
            GdipSetPenEndCap(pen, LineCapRound);
            if points.len() >= 2 {
                GdipDrawLines(
                    self.graphics,
                    pen,
                    points.as_ptr() as *const GpPointF,
                    points.len() as i32,
                );
            }
            GdipDeletePen(pen);
        }
    }
}

/// 与 C# Drawing.RoundedRect 相同的圆角路径（四段 90° 弧）。
pub fn create_rounded_path(bounds: Rect, radius: f32) -> Option<GpPathHandle> {
    unsafe {
        let mut path: GpPathHandle = std::ptr::null_mut();
        if !status_ok(GdipCreatePath(FillModeWinding, &mut path)) {
            return None;
        }
        let diameter = (radius * 2.0).max(0.0);
        if diameter <= 0.5 || bounds.w <= diameter || bounds.h <= diameter {
            let rect = bounds.to_gp();
            GdipAddPathRectangle(path, rect.X, rect.Y, rect.Width, rect.Height);
            return Some(path);
        }

        let d = diameter;
        let (x, y, w, h) = (bounds.x, bounds.y, bounds.w, bounds.h);
        GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
        GdipAddPathArc(path, x + w - d, y, d, d, 270.0, 90.0);
        GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0.0, 90.0);
        GdipAddPathArc(path, x, y + h - d, d, d, 90.0, 90.0);
        GdipClosePathFigure(path);
        Some(path)
    }
}

/// GDI DrawText（对应 C# TextRenderer.DrawText）。
pub fn gdi_draw_text(
    hdc: HDC,
    text: &str,
    font: HFONT,
    color: Argb,
    bounds: Rect,
    format: TextFormat,
) {
    if text.is_empty() || bounds.w <= 0.0 || bounds.h <= 0.0 {
        return;
    }

    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut rect = windows::Win32::Foundation::RECT {
        left: bounds.x.round() as i32,
        top: bounds.y.round() as i32,
        right: bounds.right().round() as i32,
        bottom: bounds.bottom().round() as i32,
    };

    unsafe {
        let old_font = SelectObject(hdc, HGDIOBJ(font.0 as *mut core::ffi::c_void));
        let colorref = COLORREF(((color & 0xFF) << 16) | (color & 0xFF00) | ((color >> 16) & 0xFF));
        let old_color = SetTextColor(hdc, colorref);
        SetBkMode(hdc, TRANSPARENT);

        let mut flags = DT_NOPREFIX | DT_SINGLELINE;
        if format.hcenter {
            flags |= DT_CENTER;
        } else if format.right {
            flags |= DT_RIGHT;
        } else {
            flags |= DT_LEFT;
        }
        if format.vcenter {
            flags |= DT_VCENTER;
        } else if format.bottom {
            // DT_BOTTOM 仅在 DT_SINGLELINE 下有效
            flags |= windows::Win32::Graphics::Gdi::DT_BOTTOM;
        }
        if format.end_ellipsis {
            flags |= DT_END_ELLIPSIS;
        }
        if format.word_break {
            flags |= DT_WORDBREAK | DT_EDITCONTROL;
        }

        DrawTextW(hdc, &mut wide, &mut rect, flags);

        SetTextColor(hdc, old_color);
        SelectObject(hdc, old_font);
    }
}

/// GDI 文本测量（对应 C# TextRenderer.MeasureText，CALCRECT）。
pub fn gdi_measure_text(hdc: HDC, text: &str, font: HFONT) -> (f32, f32) {
    if text.is_empty() {
        return (0.0, 0.0);
    }
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut rect = windows::Win32::Foundation::RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe {
        let old_font = SelectObject(hdc, HGDIOBJ(font.0 as *mut core::ffi::c_void));
        DrawTextW(hdc, &mut wide, &mut rect, DT_CALCRECT | DT_NOPREFIX);
        SelectObject(hdc, old_font);
    }
    ((rect.right - rect.left) as f32, (rect.bottom - rect.top) as f32)
}

/// 取屏幕 DC 并执行回调，用于 GDI 文本测量。
pub fn with_screen_dc<T>(f: impl FnOnce(HDC) -> T) -> Option<T> {
    unsafe {
        let dc = GetDC(None);
        if dc.is_invalid() {
            return None;
        }
        let result = f(dc);
        ReleaseDC(None, dc);
        Some(result)
    }
}

/// 在临时兼容 DC 里测量（不污染屏幕 DC 的选中对象）。
pub fn measure_text_offscreen(screen_dc: HDC, text: &str, font: HFONT) -> (f32, f32) {
    unsafe {
        let mem = CreateCompatibleDC(Some(screen_dc));
        let result = gdi_measure_text(mem, text, font);
        DeleteDC(mem);
        result
    }
}

pub fn screen_dpi() -> u32 {
    with_screen_dc(|dc| unsafe { GetDeviceCaps(Some(dc), LOGPIXELSY) as u32 }).unwrap_or(USER_DEFAULT_SCREEN_DPI)
}

/// 窗口 DPI 查询（PerMonitorV2 下用 GetDpiForWindow）。
pub fn dpi_for_window(hwnd: HWND) -> u32 {
    unsafe {
        let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd);
        if dpi == 0 {
            USER_DEFAULT_SCREEN_DPI
        } else {
            dpi
        }
    }
}

/// 窗口背景刷（RGB 247,248,250），双缓冲的兜底底色。
fn window_background_brush() -> windows::Win32::Graphics::Gdi::HBRUSH {
    static BRUSH: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let brush = BRUSH.get_or_init(|| unsafe {
        windows::Win32::Graphics::Gdi::CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00FAF8F7))
            .0 as usize
    });
    windows::Win32::Graphics::Gdi::HBRUSH(*brush as *mut _)
}

/// 双缓冲绘制入口：创建兼容位图，回调拿到内存 DC 绘制后 BitBlt 到目标。
/// 内存位图先填充背景色——即使回调内 GDI+ 初始化失败，窗口也显示正确底色
/// 而不是未定义内容。
pub fn double_buffer_paint(
    hwnd: HWND,
    ps_hdc: HDC,
    paint: impl FnOnce(HDC, i32, i32),
) {
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, DeleteObject, FillRect, ROP_CODE, SRCCOPY,
    };
    unsafe {
        let mut client = windows::Win32::Foundation::RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut client);
        let width = client.right - client.left;
        let height = client.bottom - client.top;
        if width <= 0 || height <= 0 {
            return;
        }

        let mem_dc = CreateCompatibleDC(Some(ps_hdc));
        let bitmap = CreateCompatibleBitmap(ps_hdc, width, height);
        let old_bitmap = SelectObject(mem_dc, HGDIOBJ(bitmap.0 as *mut core::ffi::c_void));
        let _ = FillRect(mem_dc, &client, window_background_brush());
        paint(mem_dc, width, height);
        BitBlt(
            ps_hdc,
            0,
            0,
            width,
            height,
            Some(mem_dc),
            0,
            0,
            ROP_CODE(SRCCOPY.0),
        );
        SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0 as *mut core::ffi::c_void));
        let _ = DeleteDC(mem_dc);
    }
}
