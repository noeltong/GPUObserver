using GpuObserve.Config;
using GpuObserve.Monitoring;
using GpuObserve.Notifications;
using GpuObserve.UI;
using GpuObserve.Util;

namespace GpuObserve.App;

internal sealed class TrayAppContext : ApplicationContext
{
    private readonly SingleInstance _instance;
    private readonly NotifyIcon _notifyIcon;
    private readonly ContextMenuStrip _menu;
    private readonly Icon _trayIcon;
    private readonly MonitorService _monitors = new();
    private readonly PopupForm _popup;
    private readonly ToastService _toast;
    private readonly System.Windows.Forms.Timer _statusTimer;
    private readonly System.Windows.Forms.Timer _startupTimer;

    private AppConfig _config;
    private bool _exiting;
    private bool _startupHandled;

    public TrayAppContext(SingleInstance instance, bool startHidden)
    {
        _instance = instance;

        var load = ConfigStore.Load();
        _config = load.Config;
        if (load.CreatedNew)
        {
            try
            {
                ConfigStore.Save(_config);
            }
            catch (Exception ex)
            {
                Log.Error("创建初始配置失败。", ex);
            }
        }

        _trayIcon = TrayIconFactory.LoadAppIcon();
        _menu = BuildMenu();
        _notifyIcon = new NotifyIcon
        {
            Icon = _trayIcon,
            Text = "GPU 观察",
            Visible = true,
            ContextMenuStrip = _menu
        };
        _toast = new ToastService(_notifyIcon);
        _notifyIcon.MouseClick += (_, e) =>
        {
            if (e.Button == MouseButtons.Left)
            {
                TogglePopup();
            }
        };
        _notifyIcon.BalloonTipClicked += (_, _) =>
        {
            var serverId = _toast.TakeBalloonServerId();
            ShowPopup(serverId);
        };

        _popup = new PopupForm(_monitors, () => _config);
        _ = _popup.Handle;

        _popup.AddServerRequested += (_, _) => ShowAddServer();
        _popup.SettingsRequested += (_, _) => ShowSettings();
        _popup.RetryRequested += (_, id) => _monitors.Refresh(id);
        _popup.RefreshRequested += (_, id) =>
        {
            if (id is { } serverId)
            {
                _monitors.Refresh(serverId);
            }
            else
            {
                _monitors.RefreshAll();
            }
        };

        _monitors.IdleDetected += OnIdleDetected;
        _monitors.HostKeyLearned += OnHostKeyLearned;
        _monitors.ApplyConfig(_config);

        _instance.StartServer(HandlePipeMessage);
        ToastActivation.Activated += OnToastActivated;

        if (_config.StartWithWindows)
        {
            Autostart.SetEnabled(true);
        }

        _statusTimer = new System.Windows.Forms.Timer { Interval = 2000 };
        _statusTimer.Tick += (_, _) => UpdateTrayText();
        _statusTimer.Start();
        UpdateTrayText();

        _startupTimer = new System.Windows.Forms.Timer { Interval = 180 };
        _startupTimer.Tick += (_, _) =>
        {
            _startupTimer.Stop();
            HandleStartup(load, startHidden);
        };
        _startupTimer.Start();
    }

    private ContextMenuStrip BuildMenu()
    {
        var menu = new ContextMenuStrip
        {
            Font = Theme.UiFont(9f),
            ShowImageMargin = false
        };

        var open = new ToolStripMenuItem("打开面板");
        open.Click += (_, _) => ShowPopup();

        var refresh = new ToolStripMenuItem("立即刷新");
        refresh.Click += (_, _) => _monitors.RefreshAll();

        var settings = new ToolStripMenuItem("设置…");
        settings.Click += (_, _) => ShowSettings();

        var exit = new ToolStripMenuItem("退出");
        exit.Click += (_, _) => ExitApp();

        menu.Items.Add(open);
        menu.Items.Add(refresh);
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add(settings);
        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add(exit);
        return menu;
    }

    private void HandleStartup(ConfigLoadResult load, bool startHidden)
    {
        if (_exiting || _startupHandled)
        {
            return;
        }

        _startupHandled = true;
        var pending = ToastActivation.TakePending();
        var pendingServerId = ToastActivation.ParseServerId(pending);

        if (pendingServerId is { } serverId && _config.Servers.Any(server => server.Id == serverId))
        {
            ShowPopup(serverId);
        }
        else if (!startHidden)
        {
            ShowPopup();
            if (load.CreatedNew)
            {
                ShowAddServer();
            }
        }

        if (!string.IsNullOrWhiteSpace(load.Warning))
        {
            try
            {
                _notifyIcon.ShowBalloonTip(8000, "GPU 观察", load.Warning, ToolTipIcon.Warning);
            }
            catch
            {
                // A balloon is best-effort.
            }
        }
    }

    private void TogglePopup()
    {
        if (_exiting)
        {
            return;
        }

        if (_popup.IsFlyoutVisible)
        {
            _popup.HideFlyout();
            return;
        }

        if (_popup.WasHiddenWithin(TimeSpan.FromMilliseconds(300)))
        {
            return;
        }

        ShowPopup();
    }

    private void ShowPopup(Guid? serverId = null)
    {
        if (_exiting)
        {
            return;
        }

        _popup.ShowFlyout(serverId);
    }

    private void ShowAddServer()
    {
        if (_exiting)
        {
            return;
        }

        var wasVisible = _popup.IsFlyoutVisible;
        _popup.HideFlyout();

        using var dialog = new ServerEditForm(null);
        var result = dialog.ShowDialog();
        if (result == DialogResult.OK && dialog.ResultConfig is { } server)
        {
            var newConfig = _config.Clone();
            newConfig.Servers.Add(server);
            if (SaveAndApply(newConfig))
            {
                ShowPopup(server.Id);
                return;
            }
        }

        if (wasVisible || _config.Servers.Count == 0)
        {
            ShowPopup();
        }
    }

    private void ShowSettings()
    {
        if (_exiting)
        {
            return;
        }

        var wasVisible = _popup.IsFlyoutVisible;
        _popup.HideFlyout();
        var firstServer = _config.Servers.FirstOrDefault();

        using var dialog = new SettingsForm(
            _config,
            () => _toast.ShowTest(firstServer?.Id, firstServer?.DisplayHost));
        if (dialog.ShowDialog() == DialogResult.OK && dialog.ResultConfig is { } updated)
        {
            if (SaveAndApply(updated))
            {
                if (!Autostart.SetEnabled(updated.StartWithWindows))
                {
                    MessageBox.Show(
                        "无法更新开机自启动设置，请检查当前用户权限。",
                        "GPU 观察",
                        MessageBoxButtons.OK,
                        MessageBoxIcon.Warning);
                }
            }
        }

        if (wasVisible)
        {
            ShowPopup();
        }
    }

    private bool SaveAndApply(AppConfig newConfig)
    {
        try
        {
            ConfigStore.Save(newConfig);
        }
        catch (Exception ex)
        {
            Log.Error("保存设置失败。", ex);
            MessageBox.Show(
                $"无法保存设置：{ex.Message}",
                "GPU 观察",
                MessageBoxButtons.OK,
                MessageBoxIcon.Error);
            return false;
        }

        _config = newConfig;
        _monitors.ApplyConfig(_config);
        _popup.NotifyServersChanged();
        UpdateTrayText();
        return true;
    }

    private void OnIdleDetected(object? sender, IdleNotificationArgs args)
    {
        PostUi(() =>
        {
            if (_exiting)
            {
                return;
            }

            var server = _config.Servers.FirstOrDefault(item => item.Id == args.ServerId);
            if (server is null || !_config.NotificationsEnabled || !server.NotificationsEnabled)
            {
                return;
            }

            _toast.ShowIdle(args);
        });
    }

    private void OnHostKeyLearned(object? sender, HostKeyLearnedEventArgs args)
    {
        PostUi(() =>
        {
            if (_exiting)
            {
                return;
            }

            var updated = _config.Clone();
            var server = updated.Servers.FirstOrDefault(item => item.Id == args.ServerId);
            if (server is null)
            {
                return;
            }

            server.HostKeyFingerprintSha256 = args.Fingerprint;
            try
            {
                ConfigStore.Save(updated);
                _config = updated;
            }
            catch (Exception ex)
            {
                Log.Error("保存主机密钥指纹失败。", ex);
            }
        });
    }

    private void OnToastActivated(object? sender, Guid? serverId)
    {
        PostUi(() =>
        {
            if (_exiting)
            {
                return;
            }

            if (serverId is { } id && _config.Servers.Any(server => server.Id == id))
            {
                ShowPopup(id);
            }
            else
            {
                ShowPopup();
            }
        });
    }

    private void HandlePipeMessage(string message)
    {
        PostUi(() =>
        {
            if (_exiting)
            {
                return;
            }

            const string prefix = "open-server:";
            if (message.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)
                && Guid.TryParse(message[prefix.Length..], out var serverId)
                && _config.Servers.Any(server => server.Id == serverId))
            {
                ShowPopup(serverId);
            }
            else if (message.Equals("show", StringComparison.OrdinalIgnoreCase))
            {
                ShowPopup();
            }
            else
            {
                var parsed = ToastActivation.ParseServerId(message);
                ShowPopup(parsed);
            }
        });
    }

    private void UpdateTrayText()
    {
        if (_exiting)
        {
            return;
        }

        try
        {
            var states = _monitors.GetStates();
            var errors = states.Count(state => state.Phase == MonitorPhase.Error);
            var text = states.Count == 0
                ? "GPU 观察 · 未配置服务器"
                : $"GPU 观察 · {states.Count} 台服务器" + (errors > 0 ? $" · {errors} 个错误" : "");
            _notifyIcon.Text = text.Length <= 63 ? text : text[..63];
        }
        catch (Exception ex)
        {
            Log.Error("更新托盘提示失败。", ex);
        }
    }

    private void PostUi(Action action)
    {
        if (_exiting)
        {
            return;
        }

        try
        {
            if (_popup.IsHandleCreated)
            {
                _popup.BeginInvoke(action);
            }
            else
            {
                action();
            }
        }
        catch (ObjectDisposedException)
        {
            // The window is being torn down.
        }
        catch (InvalidOperationException)
        {
            // The window is being torn down.
        }
    }

    private void ExitApp()
    {
        if (_exiting)
        {
            return;
        }

        _exiting = true;
        _statusTimer.Stop();
        _startupTimer.Stop();
        _statusTimer.Dispose();
        _startupTimer.Dispose();
        ToastActivation.Activated -= OnToastActivated;

        try
        {
            _monitors.Dispose();
        }
        catch (Exception ex)
        {
            Log.Error("停止监控失败。", ex);
        }

        try
        {
            _popup.CloseForExit();
            _popup.Dispose();
        }
        catch
        {
            // ignored
        }

        _notifyIcon.Visible = false;
        _notifyIcon.Dispose();
        _menu.Dispose();
        _trayIcon.Dispose();
        _instance.Dispose();
        ExitThread();
    }
}
