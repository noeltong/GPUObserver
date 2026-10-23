using GpuObserve.Config;
using GpuObserve.Monitoring;

namespace GpuObserve.UI;

internal sealed class ServerContentPanel : ScrollableControl
{
    private enum ActionKind
    {
        None,
        AddServer,
        Retry
    }

    private readonly Font _titleFont;
    private readonly Font _sectionFont;
    private readonly Font _gpuNameFont;
    private readonly Font _valueFont;
    private readonly Font _labelFont;
    private readonly Font _smallFont;
    private readonly Font _monoFont;
    private readonly Font _iconFont;
    private readonly Font _emptyTitleFont;

    private ServerConfig? _server;
    private ServerViewState? _state;
    private int _refreshSeconds = 5;
    private DateTimeOffset _now = DateTimeOffset.UtcNow;
    private RectangleF _actionBounds;
    private ActionKind _actionKind;
    private bool _actionHovered;
    private bool _updatingScroll;
    private Icon? _appIcon;

    public ServerContentPanel()
    {
        SetStyle(
            ControlStyles.UserPaint
            | ControlStyles.OptimizedDoubleBuffer
            | ControlStyles.AllPaintingInWmPaint
            | ControlStyles.ResizeRedraw
            | ControlStyles.Selectable,
            true);
        BackColor = Theme.WindowBackground;
        AutoScroll = true;
        TabStop = true;

        _titleFont = Theme.UiFont(9.5f, FontStyle.Bold);
        _sectionFont = Theme.UiFont(10f, FontStyle.Bold);
        _gpuNameFont = Theme.UiFont(9.25f, FontStyle.Bold);
        _valueFont = Theme.UiFont(9f, FontStyle.Bold);
        _labelFont = Theme.UiFont(8.5f);
        _smallFont = Theme.UiFont(8f);
        _monoFont = Theme.MonoFont(8.25f);
        _iconFont = Theme.IconFont(9f);
        _emptyTitleFont = Theme.UiFont(12f, FontStyle.Bold);
    }

    public event EventHandler? AddServerRequested;

    public event EventHandler<Guid>? RetryRequested;

    public int PreferredContentHeight => MeasureContentHeight();

    public void SetData(ServerConfig? server, ServerViewState? state, int refreshSeconds)
    {
        _server = server;
        _now = DateTimeOffset.UtcNow;
        _state = state is null
            ? null
            : state with
            {
                History = state.History
                    .Where(sample => _now - sample.CapturedAt <= SampleQueue.Retention)
                    .OrderBy(sample => sample.CapturedAt)
                    .ToArray()
            };
        _refreshSeconds = refreshSeconds;
        UpdateScrollSize();
        Invalidate();
    }

    public void SetNow(DateTimeOffset now)
    {
        _now = now;
        if (Visible)
        {
            Invalidate();
        }
    }

    protected override void OnResize(EventArgs e)
    {
        base.OnResize(e);
        UpdateScrollSize();
    }

    protected override void OnMouseEnter(EventArgs e)
    {
        base.OnMouseEnter(e);
        if (CanFocus)
        {
            Focus();
        }
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        base.OnMouseMove(e);
        var point = ToContentPoint(e.Location);
        var hovered = _actionKind != ActionKind.None && _actionBounds.Contains(point);
        if (hovered != _actionHovered)
        {
            _actionHovered = hovered;
            Cursor = hovered ? Cursors.Hand : Cursors.Default;
            Invalidate();
        }
    }

    protected override void OnMouseLeave(EventArgs e)
    {
        base.OnMouseLeave(e);
        if (_actionHovered)
        {
            _actionHovered = false;
            Cursor = Cursors.Default;
            Invalidate();
        }
    }

    protected override void OnMouseUp(MouseEventArgs e)
    {
        base.OnMouseUp(e);
        if (e.Button != MouseButtons.Left || _actionKind == ActionKind.None)
        {
            return;
        }

        if (!_actionBounds.Contains(ToContentPoint(e.Location)))
        {
            return;
        }

        if (_actionKind == ActionKind.AddServer)
        {
            AddServerRequested?.Invoke(this, EventArgs.Empty);
        }
        else if (_actionKind == ActionKind.Retry && _server is not null)
        {
            RetryRequested?.Invoke(this, _server.Id);
        }
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        base.OnPaint(e);
        var graphics = e.Graphics;
        graphics.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.AntiAlias;
        graphics.TranslateTransform(AutoScrollPosition.X, AutoScrollPosition.Y);

        var scale = DeviceDpi / 96f;
        var x = S(14);
        var width = Math.Max(S(200), ClientSize.Width - S(14) * 2 - (VerticalScroll.Visible ? SystemInformation.VerticalScrollBarWidth : 0));
        float y = S(8);
        _actionBounds = RectangleF.Empty;
        _actionKind = ActionKind.None;

        if (_server is null)
        {
            DrawEmpty(graphics, scale, width);
            return;
        }

        var latest = GetFreshLatest();
        DrawHeader(graphics, x, ref y, width, latest);

        if (latest is null)
        {
            DrawNoDataCard(graphics, x, y, width);
            return;
        }

        if (_state?.Phase == MonitorPhase.Error && _state.Error is not null)
        {
            DrawErrorBanner(graphics, x, ref y, width, _state.Error.Message);
        }

        if (!string.IsNullOrWhiteSpace(latest.Warning))
        {
            DrawWarningBanner(graphics, x, ref y, width, latest.Warning);
        }

        if (latest.Ram is not null)
        {
            DrawRamCard(graphics, x, ref y, width, latest.Ram);
        }

        foreach (var gpu in latest.Gpus.OrderBy(item => item.Index))
        {
            DrawGpuCard(graphics, x, ref y, width, scale, gpu, latest);
        }

        DrawProcesses(graphics, x, ref y, width, scale, latest);
    }

    private ServerSnapshot? GetFreshLatest()
    {
        var latest = _state?.Latest;
        if (latest is null)
        {
            return null;
        }

        return _now - latest.CapturedAt > SampleQueue.Retention ? null : latest;
    }

    private void DrawHeader(Graphics graphics, float x, ref float y, float width, ServerSnapshot? latest)
    {
        var titleRect = new RectangleF(x, y, width - S(112), S(22));
        Drawing.DrawText(graphics, _server!.DisplayUserHost, _titleFont, Theme.Text, Rectangle.Round(titleRect), TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine);

        var phase = _state?.Phase ?? MonitorPhase.Stopped;
        DrawStatusPill(graphics, new RectangleF(x + width - S(104), y, S(104), S(22)), phase);

        var subtitle = latest is null
            ? $"每 {_refreshSeconds} 秒刷新 · 等待首次数据"
            : $"每 {_refreshSeconds} 秒刷新 · 更新于 {Drawing.FormatClock(latest.CapturedAt)}";
        var subtitleRect = new RectangleF(x, y + S(23), width, S(16));
        Drawing.DrawText(graphics, subtitle, _smallFont, Theme.MutedText, Rectangle.Round(subtitleRect), TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine);

        y += S(46);
        using var line = new Pen(Theme.Border, 1f);
        graphics.DrawLine(line, x, y, x + width, y);
        y += S(10);
    }

    private void DrawStatusPill(Graphics graphics, RectangleF bounds, MonitorPhase phase)
    {
        var color = Theme.PhaseColor(phase);
        var soft = phase switch
        {
            MonitorPhase.Online => Theme.GreenSoft,
            MonitorPhase.Connecting => Theme.AmberSoft,
            MonitorPhase.Error => Theme.RedSoft,
            _ => Theme.Hover
        };
        Drawing.FillRounded(graphics, soft, bounds, bounds.Height / 2f);
        Drawing.DrawStatusDot(graphics, bounds.Left + S(12), bounds.Top + bounds.Height / 2f, S(6), color);
        var label = phase switch
        {
            MonitorPhase.Online => "已连接",
            MonitorPhase.Connecting => "连接中",
            MonitorPhase.Error => "连接失败",
            _ => "已停止"
        };
        var textRect = new RectangleF(bounds.Left + S(21), bounds.Top, bounds.Width - S(27), bounds.Height);
        Drawing.DrawText(graphics, label, _smallFont, color, Rectangle.Round(textRect), TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.SingleLine);
    }

    private void DrawErrorBanner(Graphics graphics, float x, ref float y, float width, string message)
    {
        var bounds = new RectangleF(x, y, width, S(34));
        Drawing.FillRounded(graphics, Theme.RedSoft, bounds, S(8));
        var glyphRect = new RectangleF(x + S(9), y, S(24), bounds.Height);
        Drawing.DrawText(graphics, Theme.GlyphWarning, _iconFont, Theme.Red, Rectangle.Round(glyphRect), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
        var retryWidth = S(42);
        var textRect = new RectangleF(x + S(33), y, width - S(33) - retryWidth - S(12), bounds.Height);
        Drawing.DrawText(graphics, $"连接失败：{message}", _smallFont, Theme.Red, Rectangle.Round(textRect), TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine);

        var retryRect = new RectangleF(x + width - retryWidth - S(10), y, retryWidth, bounds.Height);
        Drawing.DrawText(graphics, "重试", _smallFont, _actionHovered ? Theme.AccentDark : Theme.Accent, Rectangle.Round(retryRect), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
        _actionBounds = retryRect;
        _actionKind = ActionKind.Retry;
        y += bounds.Height + S(8);
    }

    private void DrawWarningBanner(Graphics graphics, float x, ref float y, float width, string message)
    {
        var bounds = new RectangleF(x, y, width, S(32));
        Drawing.FillRounded(graphics, Theme.AmberSoft, bounds, S(8));
        var glyphRect = new RectangleF(x + S(9), y, S(24), bounds.Height);
        Drawing.DrawText(graphics, Theme.GlyphWarning, _iconFont, Theme.Amber, Rectangle.Round(glyphRect), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);
        var textRect = new RectangleF(x + S(33), y, width - S(45), bounds.Height);
        Drawing.DrawText(graphics, message, _smallFont, Theme.Amber, Rectangle.Round(textRect), TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine);
        y += bounds.Height + S(8);
    }

    private void DrawRamCard(Graphics graphics, float x, ref float y, float width, RamTelemetry ram)
    {
        var bounds = new RectangleF(x, y, width, S(52));
        Drawing.DrawCard(graphics, bounds, S(10));

        var titleRect = new RectangleF(x + S(12), y + S(8), S(60), S(18));
        Drawing.DrawText(graphics, "内存", _valueFont, Theme.Text, Rectangle.Round(titleRect), TextFormatFlags.Left | TextFormatFlags.VerticalCenter);
        var valueRect = new RectangleF(x + S(58), y + S(8), width - S(130), S(18));
        Drawing.DrawText(
            graphics,
            $"{Drawing.FormatMemory(ram.UsedMiB)} / {Drawing.FormatMemory(ram.TotalMiB)}",
            _smallFont,
            Theme.SecondaryText,
            Rectangle.Round(valueRect),
            TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine);
        var percentRect = new RectangleF(x + width - S(64), y + S(8), S(52), S(18));
        Drawing.DrawText(graphics, Drawing.FormatPercent(ram.Percent), _valueFont, Theme.Text, Rectangle.Round(percentRect), TextFormatFlags.Right | TextFormatFlags.VerticalCenter);

        var barRect = new RectangleF(x + S(12), y + S(33), width - S(24), S(6));
        UsageBar.Draw(graphics, barRect, ram.Percent, Theme.MemoryColor(ram.Percent));

        y += bounds.Height + S(10);
    }

    private void DrawGpuCard(Graphics graphics, float x, ref float y, float width, float scale, GpuTelemetry gpu, ServerSnapshot snapshot)
    {
        var height = S(158);
        var card = new RectangleF(x, y, width, height);
        Drawing.DrawCard(graphics, card, S(10));

        var innerX = x + S(12);
        var innerY = y + S(10);
        var innerWidth = width - S(24);

        var extras = new List<string>();
        if (gpu.TemperatureC is { } temperature)
        {
            extras.Add($"{temperature:F0} °C");
        }
        if (gpu.PowerW is { } power)
        {
            extras.Add($"{power:F0} W");
        }
        var extraText = string.Join(" · ", extras);
        var extraWidth = string.IsNullOrEmpty(extraText)
            ? 0f
            : TextRenderer.MeasureText(extraText, _smallFont, new Size(int.MaxValue, int.MaxValue), TextFormatFlags.NoPrefix).Width + S(4);
        var badgeRect = new RectangleF(innerX, innerY, S(44), S(18));
        Drawing.FillRounded(graphics, Theme.AccentSoft, badgeRect, S(6));
        Drawing.DrawText(graphics, $"GPU {gpu.Index}", _smallFont, Theme.Accent, Rectangle.Round(badgeRect), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);

        if (!string.IsNullOrEmpty(extraText))
        {
            var extraRect = new RectangleF(innerX + innerWidth - extraWidth, innerY, extraWidth, S(18));
            Drawing.DrawText(graphics, extraText, _smallFont, Theme.SecondaryText, Rectangle.Round(extraRect), TextFormatFlags.Right | TextFormatFlags.VerticalCenter | TextFormatFlags.SingleLine);
        }

        var nameRect = new RectangleF(innerX + S(52), innerY, innerWidth - S(52) - extraWidth - S(8), S(18));
        Drawing.DrawText(graphics, gpu.Name, _gpuNameFont, Theme.Text, Rectangle.Round(nameRect), TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine);

        var utilLabel = new RectangleF(innerX, innerY + S(27), S(46), S(16));
        Drawing.DrawText(graphics, "利用率", _labelFont, Theme.SecondaryText, Rectangle.Round(utilLabel), TextFormatFlags.Left | TextFormatFlags.VerticalCenter);
        var utilPercentRect = new RectangleF(innerX + innerWidth - S(46), innerY + S(27), S(46), S(16));
        Drawing.DrawText(graphics, Drawing.FormatPercent(gpu.UtilizationPercent), _valueFont, Theme.Text, Rectangle.Round(utilPercentRect), TextFormatFlags.Right | TextFormatFlags.VerticalCenter);
        var utilBar = new RectangleF(innerX + S(50), innerY + S(32), innerWidth - S(50) - S(54), S(6));
        UsageBar.Draw(graphics, utilBar, gpu.UtilizationPercent, Theme.Accent);

        var memLabel = new RectangleF(innerX, innerY + S(48), S(46), S(16));
        Drawing.DrawText(graphics, "显存", _labelFont, Theme.SecondaryText, Rectangle.Round(memLabel), TextFormatFlags.Left | TextFormatFlags.VerticalCenter);
        var memText = $"{Drawing.FormatMemory(gpu.MemoryUsedMiB)} / {Drawing.FormatMemory(gpu.MemoryTotalMiB)}";
        var memValueRect = new RectangleF(innerX + S(50), innerY + S(48), innerWidth - S(50) - S(54), S(16));
        Drawing.DrawText(graphics, memText, _smallFont, Theme.SecondaryText, Rectangle.Round(memValueRect), TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine);
        var memPercentRect = new RectangleF(innerX + innerWidth - S(46), innerY + S(48), S(46), S(16));
        Drawing.DrawText(graphics, Drawing.FormatPercent(gpu.MemoryPercent), _valueFont, Theme.Text, Rectangle.Round(memPercentRect), TextFormatFlags.Right | TextFormatFlags.VerticalCenter);
        var memBar = new RectangleF(innerX + S(50), innerY + S(68), innerWidth - S(50) - S(54), S(6));
        UsageBar.Draw(graphics, memBar, gpu.MemoryPercent, Theme.MemoryColor(gpu.MemoryPercent));

        var chartRect = new RectangleF(innerX, innerY + S(82), innerWidth, S(44));
        Sparkline.Draw(graphics, chartRect, _state?.History ?? Array.Empty<ServerSnapshot>(), gpu.Index, _now, _smallFont);

        DrawLegend(graphics, innerX, innerY + S(132), innerWidth);

        y += height + S(10);
    }

    private void DrawLegend(Graphics graphics, float x, float y, float width)
    {
        Drawing.DrawStatusDot(graphics, x + S(3), y + S(6), S(6), Theme.Accent);
        Drawing.DrawText(graphics, "利用率", _smallFont, Theme.SecondaryText, Rectangle.Round(new RectangleF(x + S(10), y, S(44), S(12))), TextFormatFlags.Left | TextFormatFlags.VerticalCenter);
        Drawing.DrawStatusDot(graphics, x + S(57), y + S(6), S(6), Theme.Purple);
        Drawing.DrawText(graphics, "显存", _smallFont, Theme.SecondaryText, Rectangle.Round(new RectangleF(x + S(64), y, S(44), S(12))), TextFormatFlags.Left | TextFormatFlags.VerticalCenter);
        Drawing.DrawText(graphics, "近 5 分钟", _smallFont, Theme.MutedText, Rectangle.Round(new RectangleF(x, y, width, S(12))), TextFormatFlags.Right | TextFormatFlags.VerticalCenter);
    }

    private void DrawProcesses(Graphics graphics, float x, ref float y, float width, float scale, ServerSnapshot snapshot)
    {
        var titleRect = new RectangleF(x, y, width, S(20));
        Drawing.DrawText(graphics, "我的 GPU 进程", _sectionFont, Theme.Text, Rectangle.Round(titleRect), TextFormatFlags.Left | TextFormatFlags.VerticalCenter);
        var countText = snapshot.Processes.Count == 0 ? "无" : snapshot.Processes.Count.ToString();
        var countRect = new RectangleF(x, y, width, S(20));
        Drawing.DrawText(graphics, $"仅 SSH 用户 · {countText}", _smallFont, Theme.MutedText, Rectangle.Round(countRect), TextFormatFlags.Right | TextFormatFlags.VerticalCenter);
        y += S(24);

        var rowHeight = S(24);
        var rows = Math.Max(1, snapshot.Processes.Count);
        var cardHeight = S(10) + rows * rowHeight + S(10);
        var card = new RectangleF(x, y, width, cardHeight);
        Drawing.DrawCard(graphics, card, S(10));

        if (snapshot.Processes.Count == 0)
        {
            var emptyRect = new RectangleF(x + S(12), y + S(10), width - S(24), rowHeight);
            Drawing.DrawText(graphics, "当前用户暂无 GPU 进程", _smallFont, Theme.MutedText, Rectangle.Round(emptyRect), TextFormatFlags.Left | TextFormatFlags.VerticalCenter);
            y += cardHeight + S(10);
            return;
        }

        var rowY = y + S(10);
        for (var index = 0; index < snapshot.Processes.Count; index++)
        {
            var process = snapshot.Processes[index];
            if (index > 0)
            {
                using var line = new Pen(Theme.Track, 1f);
                graphics.DrawLine(line, x + S(10), rowY, x + width - S(10), rowY);
            }

            var badge = new RectangleF(x + S(12), rowY + S(4), S(24), S(16));
            Drawing.FillRounded(graphics, Theme.AccentSoft, badge, S(5));
            Drawing.DrawText(graphics, $"G{process.GpuIndex}", _smallFont, Theme.Accent, Rectangle.Round(badge), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);

            var pidRect = new RectangleF(x + S(44), rowY, S(58), rowHeight);
            Drawing.DrawText(graphics, process.Pid.ToString(), _monoFont, Theme.SecondaryText, Rectangle.Round(pidRect), TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.SingleLine);

            var memoryWidth = S(62);
            var memoryRect = new RectangleF(x + width - memoryWidth - S(12), rowY, memoryWidth, rowHeight);
            Drawing.DrawText(graphics, Drawing.FormatMemory(process.MemoryMiB), _smallFont, Theme.SecondaryText, Rectangle.Round(memoryRect), TextFormatFlags.Right | TextFormatFlags.VerticalCenter);

            var commandRect = new RectangleF(x + S(106), rowY, width - S(106) - memoryWidth - S(20), rowHeight);
            Drawing.DrawText(graphics, process.Command, _smallFont, Theme.Text, Rectangle.Round(commandRect), TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine);

            rowY += rowHeight;
        }

        y += cardHeight + S(10);
    }

    private void DrawNoDataCard(Graphics graphics, float x, float y, float width)
    {
        var bounds = new RectangleF(x, y, width, S(148));
        Drawing.DrawCard(graphics, bounds, S(10));

        var phase = _state?.Phase ?? MonitorPhase.Stopped;
        var error = _state?.Error;
        var isError = phase == MonitorPhase.Error && error is not null;
        var isStale = _state?.Latest is not null;
        var color = isError ? Theme.Red : phase == MonitorPhase.Connecting ? Theme.Amber : Theme.Accent;
        var soft = isError ? Theme.RedSoft : phase == MonitorPhase.Connecting ? Theme.AmberSoft : Theme.AccentSoft;

        var iconBounds = new RectangleF(x + S(16), y + S(18), S(34), S(34));
        Drawing.FillRounded(graphics, soft, iconBounds, S(10));
        Drawing.DrawText(
            graphics,
            isError ? Theme.GlyphError : Theme.GlyphRefresh,
            _iconFont,
            color,
            Rectangle.Round(iconBounds),
            TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter);

        var title = isError
            ? "无法获取 GPU 数据"
            : isStale
                ? "采样数据已超过 5 分钟"
                : phase == MonitorPhase.Connecting
                    ? "正在连接服务器…"
                    : "正在读取 GPU 数据…";
        var message = isError
            ? error!.Message
            : isStale
                ? "已清除过期数据，正在等待新的采样。"
                : _state?.Phase == MonitorPhase.Connecting
                    ? "正在建立 SSH 连接并读取 nvidia-smi。"
                    : "首次采样通常只需要几秒钟。";

        var titleRect = new RectangleF(x + S(62), y + S(16), width - S(80), S(22));
        Drawing.DrawText(graphics, title, _valueFont, Theme.Text, Rectangle.Round(titleRect), TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine);

        var messageRect = new RectangleF(x + S(62), y + S(41), width - S(80), S(38));
        Drawing.DrawText(graphics, message, _smallFont, Theme.SecondaryText, Rectangle.Round(messageRect), TextFormatFlags.Left | TextFormatFlags.Top | TextFormatFlags.WordBreak);

        var detail = isError ? error!.Detail : null;
        if (!string.IsNullOrWhiteSpace(detail))
        {
            var detailRect = new RectangleF(x + S(62), y + S(80), width - S(80), S(18));
            Drawing.DrawText(graphics, detail, _smallFont, Theme.MutedText, Rectangle.Round(detailRect), TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.EndEllipsis | TextFormatFlags.SingleLine);
        }

        var button = new RectangleF(x + S(62), y + S(104), S(86), S(28));
        DrawButton(graphics, button, "重试", primary: false);
        _actionBounds = button;
        _actionKind = ActionKind.Retry;
    }

    private void DrawEmpty(Graphics graphics, float scale, float width)
    {
        if (_appIcon is null)
        {
            _appIcon = TrayIconFactory.LoadAppIcon();
        }

        var groupHeight = S(204);
        var top = Math.Max(S(18), (ClientSize.Height - groupHeight) / 2f);
        if (_appIcon is not null)
        {
            var iconSize = S(58);
            var iconRect = new RectangleF((ClientSize.Width - iconSize) / 2f, top, iconSize, iconSize);
            graphics.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.HighQuality;
            graphics.DrawIcon(_appIcon, Rectangle.Round(iconRect));
        }

        var titleRect = new RectangleF(S(16), top + S(74), ClientSize.Width - S(32), S(28));
        Drawing.DrawText(graphics, "还没有 GPU 服务器", _emptyTitleFont, Theme.Text, Rectangle.Round(titleRect), TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter | TextFormatFlags.SingleLine);

        var subtitleRect = new RectangleF(S(40), top + S(106), ClientSize.Width - S(80), S(36));
        Drawing.DrawText(
            graphics,
            "添加一台 Linux + NVIDIA 服务器，开始监控 GPU 与内存。",
            _labelFont,
            Theme.SecondaryText,
            Rectangle.Round(subtitleRect),
            TextFormatFlags.HorizontalCenter | TextFormatFlags.Top | TextFormatFlags.WordBreak);

        var button = new RectangleF((ClientSize.Width - S(124)) / 2f, top + S(154), S(124), S(32));
        DrawButton(graphics, button, "添加服务器", primary: true);
        _actionBounds = button;
        _actionKind = ActionKind.AddServer;
    }

    private void DrawButton(Graphics graphics, RectangleF bounds, string text, bool primary)
    {
        var background = primary
            ? (_actionHovered ? Theme.AccentDark : Theme.Accent)
            : (_actionHovered ? Theme.Hover : Theme.CardBackground);
        if (primary)
        {
            Drawing.FillRounded(graphics, background, bounds, S(8));
        }
        else
        {
            Drawing.FillRounded(graphics, background, bounds, S(8));
            using var pen = new Pen(Theme.BorderStrong, 1f);
            Drawing.DrawRounded(graphics, pen, new RectangleF(bounds.X + 0.5f, bounds.Y + 0.5f, bounds.Width - 1f, bounds.Height - 1f), S(8));
        }

        Drawing.DrawText(
            graphics,
            text,
            _labelFont,
            primary ? Color.White : Theme.Text,
            Rectangle.Round(bounds),
            TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter | TextFormatFlags.SingleLine);
    }

    private void UpdateScrollSize()
    {
        if (_updatingScroll)
        {
            return;
        }

        var height = MeasureContentHeight();
        var target = height > ClientSize.Height ? new Size(0, height) : Size.Empty;
        if (AutoScrollMinSize == target)
        {
            return;
        }

        _updatingScroll = true;
        try
        {
            AutoScrollMinSize = target;
        }
        finally
        {
            _updatingScroll = false;
        }
    }

    private int MeasureContentHeight()
    {
        if (_server is null)
        {
            return 0;
        }

        var latest = GetFreshLatest();
        var height = S(56);
        if (latest is null)
        {
            return height + S(148) + S(16);
        }

        if (_state?.Phase == MonitorPhase.Error && _state.Error is not null)
        {
            height += S(42);
        }
        if (!string.IsNullOrWhiteSpace(latest.Warning))
        {
            height += S(40);
        }
        if (latest.Ram is not null)
        {
            height += S(62);
        }
        height += latest.Gpus.Count * (S(158) + S(10));
        height += S(24);
        var rows = Math.Max(1, latest.Processes.Count);
        height += S(10) + rows * S(24) + S(10) + S(10);
        return height + S(8);
    }

    private PointF ToContentPoint(Point point) => new(point.X - AutoScrollPosition.X, point.Y - AutoScrollPosition.Y);

    private int S(int value) => (int)Math.Round(value * DeviceDpi / 96.0);

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _titleFont.Dispose();
            _sectionFont.Dispose();
            _gpuNameFont.Dispose();
            _valueFont.Dispose();
            _labelFont.Dispose();
            _smallFont.Dispose();
            _monoFont.Dispose();
            _iconFont.Dispose();
            _emptyTitleFont.Dispose();
            _appIcon?.Dispose();
        }

        base.Dispose(disposing);
    }
}
