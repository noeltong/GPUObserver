using System.Runtime.InteropServices;
using GPUObserver.Config;
using GPUObserver.Monitoring;

namespace GPUObserver.UI;

internal sealed class PopupForm : Form
{
    private const int DwmwaWindowCornerPreference = 33;
    private const int DwmwcpRound = 2;

    private readonly MonitorService _monitors;
    private readonly Func<AppConfig> _configProvider;
    private readonly TabStrip _tabs = new();
    private readonly IconButton _addButton = new(Theme.GlyphAdd, "添加服务器");
    private readonly IconButton _settingsButton = new(Theme.GlyphSettings, "设置");
    private readonly IconButton _refreshButton = new(Theme.GlyphRefresh, "立即刷新");
    private readonly ServerContentPanel _content = new();
    private readonly Label _footerLabel;
    private readonly ToolTip _toolTip = new();
    private readonly System.Windows.Forms.Timer _uiTimer;
    private readonly Font _footerFont;

    private Guid? _selectedServerId;
    private bool _suppressDeactivate;
    private long _hiddenAtTicks;
    private bool _allowClose;

    public PopupForm(MonitorService monitors, Func<AppConfig> configProvider)
    {
        _monitors = monitors;
        _configProvider = configProvider;

        FormBorderStyle = FormBorderStyle.None;
        ShowInTaskbar = false;
        TopMost = true;
        StartPosition = FormStartPosition.Manual;
        AutoScaleMode = AutoScaleMode.None;
        DoubleBuffered = true;
        KeyPreview = true;
        ShowIcon = false;
        MaximizeBox = false;
        MinimizeBox = false;
        BackColor = Theme.WindowBackground;
        Text = "GPU 观察";

        _footerFont = Theme.UiFont(8.25f);
        _footerLabel = new Label
        {
            AutoSize = false,
            BackColor = Theme.WindowBackground,
            ForeColor = Theme.MutedText,
            Font = _footerFont,
            TextAlign = ContentAlignment.MiddleLeft,
            Text = ""
        };

        _uiTimer = new System.Windows.Forms.Timer { Interval = 1000 };
        _uiTimer.Tick += (_, _) =>
        {
            if (Visible)
            {
                SyncServers();
            }
        };

        Controls.Add(_tabs);
        Controls.Add(_content);
        Controls.Add(_footerLabel);
        Controls.Add(_addButton);
        Controls.Add(_settingsButton);
        Controls.Add(_refreshButton);

        _toolTip.SetToolTip(_addButton, "添加服务器");
        _toolTip.SetToolTip(_settingsButton, "设置");
        _toolTip.SetToolTip(_refreshButton, "立即刷新");

        _tabs.Selected += (_, id) =>
        {
            _selectedServerId = id;
            RefreshContent();
        };

        _addButton.Click += (_, _) => AddServerRequested?.Invoke(this, EventArgs.Empty);
        _settingsButton.Click += (_, _) => SettingsRequested?.Invoke(this, EventArgs.Empty);
        _refreshButton.Click += (_, _) => RefreshRequested?.Invoke(this, _selectedServerId);
        _content.AddServerRequested += (_, _) => AddServerRequested?.Invoke(this, EventArgs.Empty);
        _content.RetryRequested += (_, id) => RetryRequested?.Invoke(this, id);

        Deactivate += (_, _) =>
        {
            if (Visible && !_suppressDeactivate)
            {
                HideFlyout();
            }
        };

        DpiChanged += (_, _) =>
        {
            if (Visible)
            {
                ApplySizeAndPosition();
            }

            LayoutChildren();
        };

        Resize += (_, _) =>
        {
            LayoutChildren();
            ApplyRoundedRegion();
        };
    }

    public event EventHandler? AddServerRequested;

    public event EventHandler? SettingsRequested;

    public event EventHandler<Guid>? RetryRequested;

    public event EventHandler<Guid?>? RefreshRequested;

    public bool IsFlyoutVisible => Visible;

    public Guid? SelectedServerId => _selectedServerId;

    public void ShowFlyout(Guid? serverId = null)
    {
        if (_allowClose || IsDisposed)
        {
            return;
        }

        if (serverId is { } id && _configProvider().Servers.Any(server => server.Id == id))
        {
            _selectedServerId = id;
        }

        SyncServers();

        if (!Visible)
        {
            ApplySizeAndPosition();
            LayoutChildren();
            Show();
            ApplyRoundedRegion();
            _suppressDeactivate = true;
            BeginInvoke(new Action(() => _suppressDeactivate = false));
        }

        Activate();
        SetForegroundWindow(Handle);
        BringToFront();
        _uiTimer.Start();
    }

    public void HideFlyout()
    {
        if (!Visible)
        {
            return;
        }

        _uiTimer.Stop();
        Hide();
        _hiddenAtTicks = Environment.TickCount64;
    }

    public bool WasHiddenWithin(TimeSpan window)
    {
        if (_hiddenAtTicks == 0)
        {
            return false;
        }

        return Environment.TickCount64 - _hiddenAtTicks <= (long)window.TotalMilliseconds;
    }

    public void NotifyServersChanged()
    {
        if (!IsDisposed)
        {
            SyncServers();
            if (Visible)
            {
                LayoutChildren();
            }
        }
    }

    public void SelectServer(Guid? serverId)
    {
        _selectedServerId = serverId;
        if (!IsDisposed)
        {
            SyncServers();
        }
    }

    public void CloseForExit()
    {
        _allowClose = true;
        _uiTimer.Stop();
        Close();
    }

    protected override CreateParams CreateParams
    {
        get
        {
            var parameters = base.CreateParams;
            parameters.ClassStyle |= 0x00020000; // CS_DROPSHADOW
            parameters.ExStyle |= 0x00000080; // WS_EX_TOOLWINDOW
            return parameters;
        }
    }

    protected override void OnHandleCreated(EventArgs e)
    {
        base.OnHandleCreated(e);
        try
        {
            var preference = DwmwcpRound;
            DwmSetWindowAttribute(Handle, DwmwaWindowCornerPreference, ref preference, sizeof(int));
        }
        catch
        {
            // Rounded corners are cosmetic.
        }
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        base.OnPaint(e);
        e.Graphics.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.AntiAlias;
        var scale = DeviceDpi / 96f;
        using var border = new Pen(Theme.BorderStrong, Math.Max(1f, scale));
        Drawing.DrawRounded(
            e.Graphics,
            border,
            new RectangleF(0.5f, 0.5f, Width - 1f, Height - 1f),
            (float)Math.Round(10 * scale));
        using var separator = new Pen(Theme.Border, 1f);
        e.Graphics.DrawLine(separator, S(12), S(45), Width - S(12), S(45));
        e.Graphics.DrawLine(separator, S(12), Height - S(35), Width - S(12), Height - S(35));
    }

    protected override void OnKeyDown(KeyEventArgs e)
    {
        if (e.KeyCode == Keys.Escape)
        {
            HideFlyout();
            e.Handled = true;
        }

        base.OnKeyDown(e);
    }

    protected override void OnFormClosing(FormClosingEventArgs e)
    {
        if (!_allowClose && e.CloseReason != CloseReason.WindowsShutDown)
        {
            e.Cancel = true;
            HideFlyout();
            return;
        }

        base.OnFormClosing(e);
    }

    private void SyncServers()
    {
        if (IsDisposed)
        {
            return;
        }

        var config = _configProvider();
        var states = _monitors.GetStates();
        var stateLookup = states.ToDictionary(state => state.ServerId);

        if (_selectedServerId is null || config.Servers.All(server => server.Id != _selectedServerId))
        {
            _selectedServerId = config.Servers.FirstOrDefault()?.Id;
        }

        var items = config.Servers
            .Select(server => new TabStripItem(
                server.Id,
                server.DisplayHost,
                stateLookup.TryGetValue(server.Id, out var state) ? state.Phase : MonitorPhase.Stopped))
            .ToList();
        _tabs.SetItems(items, _selectedServerId);
        RefreshContent(config, states);
    }

    private void RefreshContent()
    {
        if (IsDisposed)
        {
            return;
        }

        RefreshContent(_configProvider(), _monitors.GetStates());
    }

    private void RefreshContent(AppConfig config, IReadOnlyList<ServerViewState> states)
    {
        var server = _selectedServerId is { } id
            ? config.Servers.FirstOrDefault(item => item.Id == id)
            : null;
        var state = server is null
            ? null
            : states.FirstOrDefault(item => item.ServerId == server.Id);

        _content.SetData(server, state, config.RefreshSeconds);
        _refreshButton.Enabled = server is not null;

        if (server is null)
        {
            _footerLabel.Text = "尚未添加服务器";
        }
        else if (state?.Latest is { } latest)
        {
            _footerLabel.Text = $"上次更新 {Drawing.FormatClock(latest.CapturedAt)}";
        }
        else
        {
            _footerLabel.Text = "等待数据…";
        }

        AdjustHeightIfNeeded();
    }

    private void AdjustHeightIfNeeded()
    {
        if (!Visible || IsDisposed)
        {
            return;
        }

        var scale = Math.Max(0.5, DeviceDpi / 96.0);
        var target = CalculateLogicalHeight();
        var current = ClientSize.Height / scale;
        if (Math.Abs(target - current) < 40)
        {
            return;
        }

        ApplySizeAndPosition();
    }

    private int CalculateLogicalHeight()
    {
        if (_configProvider().Servers.Count == 0)
        {
            return 300;
        }

        var scale = Math.Max(0.5, DeviceDpi / 96.0);
        var contentHeight = _content.PreferredContentHeight / scale;
        return (int)Math.Clamp(contentHeight + 97, 340, 960);
    }

    private void ApplySizeAndPosition()
    {
        var cursor = Cursor.Position;
        var screen = Screen.FromPoint(cursor);
        var workingArea = screen.WorkingArea;
        var bounds = screen.Bounds;
        var dpi = GetDpiForScreen(screen);
        var scale = dpi / 96.0;

        var logicalHeight = CalculateLogicalHeight();
        var margin = (int)Math.Round(10 * scale);
        var width = (int)Math.Round(428 * scale);
        var height = (int)Math.Round(logicalHeight * scale);
        width = Math.Min(width, Math.Max((int)(260 * scale), workingArea.Width - margin * 2));
        height = Math.Min(height, Math.Max((int)(220 * scale), workingArea.Height - margin * 2));

        var taskbarBottom = workingArea.Bottom < bounds.Bottom;
        var taskbarTop = workingArea.Top > bounds.Top;
        var taskbarLeft = workingArea.Left > bounds.Left;
        var taskbarRight = workingArea.Right < bounds.Right;

        int x;
        int y;
        if (taskbarBottom)
        {
            x = workingArea.Right - width - margin;
            y = workingArea.Bottom - height - margin;
        }
        else if (taskbarTop)
        {
            x = workingArea.Right - width - margin;
            y = workingArea.Top + margin;
        }
        else if (taskbarLeft)
        {
            x = workingArea.Left + margin;
            y = workingArea.Bottom - height - margin;
        }
        else if (taskbarRight)
        {
            x = workingArea.Right - width - margin;
            y = workingArea.Bottom - height - margin;
        }
        else
        {
            // Auto-hide taskbar: sit next to the cursor.
            x = workingArea.Right - width - margin;
            y = cursor.Y > bounds.Top + bounds.Height / 2
                ? cursor.Y - height - margin * 2
                : cursor.Y + margin * 2;
        }

        var minX = workingArea.Left + margin;
        var maxX = Math.Max(minX, workingArea.Right - width - margin);
        var minY = workingArea.Top + margin;
        var maxY = Math.Max(minY, workingArea.Bottom - height - margin);
        x = Math.Clamp(x, minX, maxX);
        y = Math.Clamp(y, minY, maxY);

        Bounds = new Rectangle(x, y, width, height);
    }

    private void LayoutChildren()
    {
        if (IsDisposed)
        {
            return;
        }

        var headerY = S(7);
        var buttonSize = S(30);
        _settingsButton.Bounds = new Rectangle(ClientSize.Width - S(10) - buttonSize, headerY, buttonSize, buttonSize);
        _addButton.Bounds = new Rectangle(_settingsButton.Left - S(4) - buttonSize, headerY, buttonSize, buttonSize);
        _tabs.Bounds = new Rectangle(
            S(10),
            headerY,
            Math.Max(0, _addButton.Left - S(10) - S(4)),
            buttonSize);

        var footerTop = ClientSize.Height - S(38);
        _content.Bounds = new Rectangle(0, S(47), ClientSize.Width, Math.Max(0, footerTop - S(47)));
        _footerLabel.Bounds = new Rectangle(S(14), ClientSize.Height - S(30), Math.Max(0, ClientSize.Width - S(72)), S(22));
        _refreshButton.Bounds = new Rectangle(ClientSize.Width - S(10) - S(28), ClientSize.Height - S(32), S(28), S(28));
    }

    private int S(int value) => (int)Math.Round(value * DeviceDpi / 96.0);

    private void ApplyRoundedRegion()
    {
        if (IsDisposed || Width <= 0 || Height <= 0)
        {
            return;
        }

        using var path = Drawing.RoundedRect(new RectangleF(0, 0, Width, Height), S(10));
        var region = new Region(path);
        var previous = Region;
        Region = region;
        previous?.Dispose();
    }

    private static int GetDpiForScreen(Screen screen)
    {
        try
        {
            var center = new NativePoint
            {
                X = screen.Bounds.Left + screen.Bounds.Width / 2,
                Y = screen.Bounds.Top + screen.Bounds.Height / 2
            };
            var monitor = MonitorFromPoint(center, 2);
            if (monitor != IntPtr.Zero && GetDpiForMonitor(monitor, 0, out var dpiX, out _) == 0 && dpiX > 0)
            {
                return (int)dpiX;
            }
        }
        catch
        {
            // Fall back to 96 DPI.
        }

        return 96;
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _uiTimer.Dispose();
            _toolTip.Dispose();
            _footerFont.Dispose();
        }

        base.Dispose(disposing);
    }

    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref int value, int size);

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern IntPtr MonitorFromPoint(NativePoint point, uint flags);

    [DllImport("shcore.dll")]
    private static extern int GetDpiForMonitor(IntPtr monitor, int dpiType, out uint dpiX, out uint dpiY);

    [StructLayout(LayoutKind.Sequential)]
    private struct NativePoint
    {
        public int X;
        public int Y;
    }
}
