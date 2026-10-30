//! 主题：颜色、字形与格式化助手，与 C# 版 UI/Theme.cs、Drawing.cs 一致。

use super::gfx::{self, Argb, Graphics, Rect};
use crate::monitor::snapshot::MonitorPhase;
use chrono::{DateTime, Local, TimeDelta, Utc};

pub const WINDOW_BACKGROUND: Argb = gfx::rgb(247, 248, 250);
pub const CARD_BACKGROUND: Argb = gfx::rgb(255, 255, 255);
pub const BORDER: Argb = gfx::rgb(226, 229, 234);
pub const BORDER_STRONG: Argb = gfx::rgb(210, 215, 222);
pub const TEXT: Argb = gfx::rgb(25, 28, 33);
pub const SECONDARY_TEXT: Argb = gfx::rgb(98, 106, 118);
pub const MUTED_TEXT: Argb = gfx::rgb(143, 149, 160);
pub const ACCENT: Argb = gfx::rgb(37, 99, 235);
pub const ACCENT_DARK: Argb = gfx::rgb(29, 78, 187);
pub const ACCENT_SOFT: Argb = gfx::rgb(232, 240, 254);
pub const PURPLE: Argb = gfx::rgb(124, 58, 237);
pub const GREEN: Argb = gfx::rgb(16, 124, 65);
pub const GREEN_SOFT: Argb = gfx::rgb(229, 247, 237);
pub const AMBER: Argb = gfx::rgb(180, 83, 9);
pub const AMBER_SOFT: Argb = gfx::rgb(254, 243, 199);
pub const RED: Argb = gfx::rgb(185, 28, 28);
pub const RED_SOFT: Argb = gfx::rgb(254, 226, 226);
pub const TRACK: Argb = gfx::rgb(235, 238, 243);
pub const HOVER: Argb = gfx::rgb(238, 241, 246);
pub const PRESSED: Argb = gfx::rgb(228, 233, 240);
pub const CHART_BACKGROUND: Argb = gfx::rgb(250, 251, 253);
pub const CHART_GRID: Argb = gfx::rgb(235, 238, 243);
pub const WHITE: Argb = gfx::rgb(255, 255, 255);

pub const GLYPH_ADD: &str = "\u{E710}";
pub const GLYPH_SETTINGS: &str = "\u{E713}";
pub const GLYPH_REFRESH: &str = "\u{E72C}";
pub const GLYPH_WARNING: &str = "\u{E7BA}";
pub const GLYPH_ERROR: &str = "\u{E783}";
pub const GLYPH_CHECK: &str = "\u{E73E}";
pub const GLYPH_LEFT: &str = "\u{E76B}";
pub const GLYPH_RIGHT: &str = "\u{E76C}";

pub fn usage_color(percent: Option<f64>) -> Argb {
    match percent {
        None => MUTED_TEXT,
        Some(p) if p >= 90.0 => RED,
        Some(p) if p >= 70.0 => AMBER,
        Some(_) => ACCENT,
    }
}

pub fn memory_color(percent: Option<f64>) -> Argb {
    match percent {
        None => MUTED_TEXT,
        Some(p) if p >= 95.0 => RED,
        Some(p) if p >= 80.0 => AMBER,
        Some(_) => PURPLE,
    }
}

pub fn phase_color(phase: MonitorPhase) -> Argb {
    match phase {
        MonitorPhase::Online => GREEN,
        MonitorPhase::Connecting => AMBER,
        MonitorPhase::Error => RED,
        _ => MUTED_TEXT,
    }
}

pub fn phase_soft(phase: MonitorPhase) -> Argb {
    match phase {
        MonitorPhase::Online => GREEN_SOFT,
        MonitorPhase::Connecting => AMBER_SOFT,
        MonitorPhase::Error => RED_SOFT,
        _ => HOVER,
    }
}

pub fn phase_label(phase: MonitorPhase) -> &'static str {
    let t = super::i18n::t();
    match phase {
        MonitorPhase::Online => t.phase_online,
        MonitorPhase::Connecting => t.phase_connecting,
        MonitorPhase::Error => t.phase_error,
        MonitorPhase::Stopped => t.phase_stopped,
    }
}

pub fn format_memory(mib: Option<i64>) -> String {
    let Some(mib) = mib else {
        return "—".to_string();
    };
    if mib < 0 {
        return "—".to_string();
    }

    let gib = mib as f64 / 1024.0;
    if gib >= 100.0 {
        format!("{gib:.0} GB")
    } else {
        format!("{gib:.1} GB")
    }
}

pub fn format_percent(percent: Option<f64>) -> String {
    match percent {
        None => "—".to_string(),
        Some(value) => format!("{value:.0}%"),
    }
}

pub fn format_relative(timestamp: DateTime<Utc>, now: Option<DateTime<Utc>>) -> String {
    use super::i18n;
    let elapsed = now.unwrap_or_else(Utc::now).signed_duration_since(timestamp);
    let seconds = elapsed.num_milliseconds() as f64 / 1000.0;
    if seconds < 2.0 {
        return i18n::t().rel_just_now.to_string();
    }
    if seconds < 60.0 {
        return i18n::fmt(i18n::t().rel_seconds, &[("n", &(seconds as i64).to_string())]);
    }
    let minutes = seconds / 60.0;
    if minutes < 60.0 {
        return i18n::fmt(i18n::t().rel_minutes, &[("n", &(minutes as i64).to_string())]);
    }
    i18n::fmt(i18n::t().rel_hours, &[("n", &((minutes / 60.0) as i64).to_string())])
}

pub fn format_clock(timestamp: DateTime<Utc>) -> String {
    let local: DateTime<Local> = timestamp.with_timezone(&Local);
    local.format("%H:%M:%S").to_string()
}

/// 二分截断加省略号（对应 Drawing.Ellipsize）。
pub fn ellipsize(g: &Graphics, text: &str, font: super::gfx::GpFontHandle, max_width: f32) -> String {
    if max_width <= 0.0 || text.is_empty() {
        return text.to_string();
    }

    let (width, _) = g.measure_string(text, font);
    if width <= max_width {
        return text.to_string();
    }

    let ellipsis = "…";
    let chars: Vec<char> = text.chars().collect();
    let mut low: i64 = 0;
    let mut high: i64 = chars.len() as i64;
    while low < high {
        let mid = (low + high + 1) / 2;
        let candidate: String = chars[..mid as usize].iter().collect::<String>() + ellipsis;
        let (candidate_width, _) = g.measure_string(&candidate, font);
        if candidate_width <= max_width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }

    if low <= 0 {
        ellipsis.to_string()
    } else {
        chars[..low as usize].iter().collect::<String>() + ellipsis
    }
}

/// 状态圆点（对应 Drawing.DrawStatusDot）。
pub fn draw_status_dot(g: &Graphics, center_x: f32, center_y: f32, diameter: f32, color: Argb) {
    g.fill_ellipse_at(color, center_x, center_y, diameter);
}

/// 卡片：白底 + 1px 内缩边框（对应 Drawing.DrawCard）。
pub fn draw_card(g: &Graphics, bounds: Rect, radius: f32) {
    g.fill_rounded(CARD_BACKGROUND, bounds, radius);
    g.draw_rounded(
        BORDER,
        1.0,
        Rect::new(bounds.x + 0.5, bounds.y + 0.5, bounds.w - 1.0, bounds.h - 1.0),
        radius,
    );
}

/// UsageBar：胶囊形轨道 + 填充（最小填充宽 = 轨道高）。
pub fn draw_usage_bar(g: &Graphics, bounds: Rect, percent: Option<f64>, color: Argb) {
    let radius = bounds.h / 2.0;
    g.fill_rounded(TRACK, bounds, radius);

    let Some(percent) = percent else {
        return;
    };
    let value = percent.clamp(0.0, 100.0);
    if value <= 0.0 {
        return;
    }

    let fill_width = (bounds.h as f64).max(bounds.w as f64 * value / 100.0) as f32;
    let fill = Rect::new(bounds.x, bounds.y, fill_width.min(bounds.w), bounds.h);
    g.fill_rounded(color, fill, radius);
}

/// 忽略阈值仅用于队列过期判断的引用（保持与 C# SampleQueue.Retention 一致）。
pub fn retention() -> TimeDelta {
    TimeDelta::seconds(300)
}

/// Argb(0xAARRGGBB) → COLORREF(0x00BBGGRR)。
pub fn argb_to_colorref(color: Argb) -> windows::Win32::Foundation::COLORREF {
    let r = (color >> 16) & 0xFF;
    let g = (color >> 8) & 0xFF;
    let b = color & 0xFF;
    windows::Win32::Foundation::COLORREF(r | (g << 8) | (b << 16))
}
