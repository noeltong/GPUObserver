using System.Drawing.Drawing2D;
using GPUObserver.Monitoring;

namespace GPUObserver.UI;

internal static class Sparkline
{
    public static void Draw(
        Graphics graphics,
        RectangleF bounds,
        IReadOnlyList<ServerSnapshot> history,
        int gpuIndex,
        DateTimeOffset now,
        Font placeholderFont)
    {
        var state = graphics.Save();
        graphics.SmoothingMode = SmoothingMode.AntiAlias;
        graphics.PixelOffsetMode = PixelOffsetMode.HighQuality;

        using (var backgroundPath = Drawing.RoundedRect(bounds, 8f))
        using (var backgroundBrush = new SolidBrush(Theme.ChartBackground))
        {
            graphics.FillPath(backgroundBrush, backgroundPath);
        }

        var plot = RectangleF.Inflate(bounds, -8f, -6f);
        using (var gridPen = new Pen(Theme.ChartGrid, 1f))
        {
            for (var i = 0; i <= 2; i++)
            {
                var y = plot.Top + plot.Height * i / 2f;
                graphics.DrawLine(gridPen, plot.Left, y, plot.Right, y);
            }
        }

        var windowStart = now - SampleQueue.Retention;
        var utilizationPoints = new List<PointF>();
        var memoryPoints = new List<PointF>();

        foreach (var snapshot in history)
        {
            if (snapshot.CapturedAt < windowStart || snapshot.CapturedAt > now.AddSeconds(1))
            {
                continue;
            }

            var gpu = snapshot.Gpus.FirstOrDefault(item => item.Index == gpuIndex);
            if (gpu is null)
            {
                continue;
            }

            var x = plot.Left + (float)((snapshot.CapturedAt - windowStart).TotalSeconds / SampleQueue.Retention.TotalSeconds) * plot.Width;
            if (gpu.UtilizationPercent is { } utilization)
            {
                utilizationPoints.Add(new PointF(x, ValueToY(plot, utilization)));
            }

            if (gpu.MemoryPercent is { } memory)
            {
                memoryPoints.Add(new PointF(x, ValueToY(plot, memory)));
            }
        }

        if (utilizationPoints.Count < 2 && memoryPoints.Count < 2)
        {
            Drawing.DrawText(
                graphics,
                "采样中…",
                placeholderFont,
                Theme.MutedText,
                Rectangle.Round(plot),
                TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter | TextFormatFlags.NoPrefix);
            graphics.Restore(state);
            return;
        }

        var clipState = graphics.Save();
        using (var clipPath = Drawing.RoundedRect(bounds, 8f))
        {
            graphics.SetClip(clipPath);
        }

        if (utilizationPoints.Count >= 2)
        {
            using var area = new GraphicsPath();
            area.AddLines(utilizationPoints.ToArray());
            area.AddLine(utilizationPoints[^1].X, plot.Bottom, utilizationPoints[0].X, plot.Bottom);
            area.CloseFigure();
            using var areaBrush = new LinearGradientBrush(
                new RectangleF(plot.Left, plot.Top, plot.Width, plot.Height),
                Drawing.WithAlpha(Theme.Accent, 42),
                Drawing.WithAlpha(Theme.Accent, 2),
                LinearGradientMode.Vertical);
            graphics.FillPath(areaBrush, area);
        }

        DrawSeries(graphics, utilizationPoints, Theme.Accent, 1.7f);
        DrawSeries(graphics, memoryPoints, Theme.Purple, 1.4f);

        graphics.Restore(clipState);
        graphics.Restore(state);
    }

    private static void DrawSeries(Graphics graphics, List<PointF> points, Color color, float width)
    {
        if (points.Count == 0)
        {
            return;
        }

        using var pen = new Pen(color, width)
        {
            LineJoin = LineJoin.Round,
            StartCap = LineCap.Round,
            EndCap = LineCap.Round
        };

        if (points.Count >= 2)
        {
            graphics.DrawLines(pen, points.ToArray());
        }

        using var dot = new SolidBrush(color);
        graphics.FillEllipse(dot, points[^1].X - 2f, points[^1].Y - 2f, 4f, 4f);
    }

    private static float ValueToY(RectangleF plot, double percent) =>
        plot.Bottom - (float)(Math.Clamp(percent, 0, 100) / 100.0) * plot.Height;
}
