using System.Drawing.Drawing2D;

namespace GpuObserve.UI;

internal static class UsageBar
{
    public static void Draw(Graphics graphics, RectangleF bounds, double? percent, Color color)
    {
        var radius = bounds.Height / 2f;
        using (var track = Drawing.RoundedRect(bounds, radius))
        using (var trackBrush = new SolidBrush(Theme.Track))
        {
            graphics.FillPath(trackBrush, track);
        }

        if (percent is null)
        {
            return;
        }

        var value = Math.Clamp(percent.Value, 0, 100);
        if (value <= 0)
        {
            return;
        }

        var fillWidth = Math.Max(bounds.Height, (float)(bounds.Width * value / 100.0));
        var fill = new RectangleF(bounds.X, bounds.Y, Math.Min(fillWidth, bounds.Width), bounds.Height);
        var state = graphics.Save();
        graphics.SmoothingMode = SmoothingMode.AntiAlias;
        using var fillPath = Drawing.RoundedRect(fill, radius);
        using var fillBrush = new SolidBrush(color);
        graphics.FillPath(fillBrush, fillPath);
        graphics.Restore(state);
    }
}
