using System.Drawing.Drawing2D;
using System.Drawing.Text;
using System.Globalization;

namespace GPUObserver.UI;

internal static class Drawing
{
    public static GraphicsPath RoundedRect(RectangleF bounds, float radius)
    {
        var path = new GraphicsPath();
        var diameter = Math.Max(0, radius * 2);
        if (diameter <= 0.5f || bounds.Width <= diameter || bounds.Height <= diameter)
        {
            path.AddRectangle(bounds);
            return path;
        }

        path.AddArc(bounds.X, bounds.Y, diameter, diameter, 180, 90);
        path.AddArc(bounds.Right - diameter, bounds.Y, diameter, diameter, 270, 90);
        path.AddArc(bounds.Right - diameter, bounds.Bottom - diameter, diameter, diameter, 0, 90);
        path.AddArc(bounds.X, bounds.Bottom - diameter, diameter, diameter, 90, 90);
        path.CloseFigure();
        return path;
    }

    public static void FillRounded(Graphics graphics, Color color, RectangleF bounds, float radius)
    {
        using var path = RoundedRect(bounds, radius);
        using var brush = new SolidBrush(color);
        graphics.FillPath(brush, path);
    }

    public static void DrawRounded(Graphics graphics, Pen pen, RectangleF bounds, float radius)
    {
        using var path = RoundedRect(bounds, radius);
        graphics.DrawPath(pen, path);
    }

    public static void DrawCard(Graphics graphics, RectangleF bounds, float radius = 10f, Color? background = null, Color? border = null)
    {
        FillRounded(graphics, background ?? Theme.CardBackground, bounds, radius);
        using var pen = new Pen(border ?? Theme.Border, 1f);
        DrawRounded(graphics, pen, new RectangleF(bounds.X + 0.5f, bounds.Y + 0.5f, bounds.Width - 1f, bounds.Height - 1f), radius);
    }

    public static void DrawText(
        Graphics graphics,
        string text,
        Font font,
        Color color,
        Rectangle bounds,
        TextFormatFlags flags = TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.NoPrefix)
    {
        if (string.IsNullOrEmpty(text) || bounds.Width <= 0 || bounds.Height <= 0)
        {
            return;
        }

        var allowWrap = flags.HasFlag(TextFormatFlags.WordBreak);
        if (!allowWrap)
        {
            text = text.ReplaceLineEndings(" ");
        }

        // TextRenderer uses GDI and ignores Graphics transforms; DrawString keeps
        // text aligned with the scroll translation used by the flyout panel.
        using var format = new StringFormat
        {
            Alignment = flags.HasFlag(TextFormatFlags.HorizontalCenter)
                ? StringAlignment.Center
                : flags.HasFlag(TextFormatFlags.Right)
                    ? StringAlignment.Far
                    : StringAlignment.Near,
            LineAlignment = flags.HasFlag(TextFormatFlags.VerticalCenter)
                ? StringAlignment.Center
                : flags.HasFlag(TextFormatFlags.Bottom)
                    ? StringAlignment.Far
                    : StringAlignment.Near,
            Trimming = flags.HasFlag(TextFormatFlags.EndEllipsis)
                ? StringTrimming.EllipsisCharacter
                : StringTrimming.None,
            HotkeyPrefix = HotkeyPrefix.None
        };

        if (!allowWrap)
        {
            format.FormatFlags |= StringFormatFlags.NoWrap;
        }

        using var brush = new SolidBrush(color);
        graphics.DrawString(text, font, brush, new RectangleF(bounds.X, bounds.Y, bounds.Width, bounds.Height), format);
    }

    public static SizeF MeasureText(Graphics graphics, string text, Font font)
    {
        if (string.IsNullOrEmpty(text))
        {
            return SizeF.Empty;
        }

        using var format = new StringFormat
        {
            FormatFlags = StringFormatFlags.NoWrap,
            Trimming = StringTrimming.None,
            HotkeyPrefix = HotkeyPrefix.None
        };
        return graphics.MeasureString(text, font, int.MaxValue, format);
    }

    public static string Ellipsize(Graphics graphics, string text, Font font, int maxWidth)
    {
        if (maxWidth <= 0 || string.IsNullOrEmpty(text))
        {
            return text;
        }

        var measured = MeasureText(graphics, text, font);
        if (measured.Width <= maxWidth)
        {
            return text;
        }

        const string ellipsis = "…";
        var low = 0;
        var high = text.Length;
        while (low < high)
        {
            var mid = (low + high + 1) / 2;
            var candidate = text[..mid] + ellipsis;
            var width = MeasureText(graphics, candidate, font).Width;
            if (width <= maxWidth)
            {
                low = mid;
            }
            else
            {
                high = mid - 1;
            }
        }

        return low <= 0 ? ellipsis : text[..low] + ellipsis;
    }

    public static void DrawStatusDot(Graphics graphics, float centerX, float centerY, float diameter, Color color)
    {
        var rect = new RectangleF(centerX - diameter / 2f, centerY - diameter / 2f, diameter, diameter);
        using var brush = new SolidBrush(color);
        graphics.SmoothingMode = SmoothingMode.AntiAlias;
        graphics.FillEllipse(brush, rect);
    }

    public static string FormatMemory(long? mib)
    {
        if (mib is null or < 0)
        {
            return "—";
        }

        var gib = mib.Value / 1024.0;
        return gib >= 100 ? $"{gib:F0} GB" : $"{gib:F1} GB";
    }

    public static string FormatPercent(double? percent) => percent is null ? "—" : $"{percent.Value:F0}%";

    public static string FormatRelative(DateTimeOffset timestamp, DateTimeOffset? now = null)
    {
        var elapsed = (now ?? DateTimeOffset.UtcNow) - timestamp;
        if (elapsed.TotalSeconds < 0)
        {
            return "刚刚";
        }

        if (elapsed.TotalSeconds < 2)
        {
            return "刚刚";
        }

        if (elapsed.TotalSeconds < 60)
        {
            return $"{(int)elapsed.TotalSeconds} 秒前";
        }

        if (elapsed.TotalMinutes < 60)
        {
            return $"{(int)elapsed.TotalMinutes} 分钟前";
        }

        return $"{(int)elapsed.TotalHours} 小时前";
    }

    public static string FormatClock(DateTimeOffset timestamp) =>
        timestamp.ToLocalTime().ToString("HH:mm:ss", CultureInfo.CurrentCulture);

    public static Color WithAlpha(Color color, int alpha) => Color.FromArgb(alpha, color);
}
