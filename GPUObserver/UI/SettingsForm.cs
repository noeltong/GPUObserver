using GPUObserver.Config;

namespace GPUObserver.UI;

internal sealed class SettingsForm : Form
{
    private readonly AppConfig _working;
    private readonly List<ServerConfig> _servers;
    private readonly ListBox _serverList = new();
    private readonly NumericUpDown _refresh = new();
    private readonly CheckBox _notifications = new();
    private readonly CheckBox _autostart = new();
    private readonly Button _add;
    private readonly Button _edit;
    private readonly Button _delete;
    private readonly Button _testNotification;
    private readonly Button _save;
    private readonly Button _cancel;
    private readonly Label _testHint = new();
    private readonly Action _testNotificationAction;

    public SettingsForm(AppConfig config, Action testNotificationAction)
    {
        _working = config.Clone();
        _servers = _working.Servers.Select(server => server.Clone()).ToList();
        _testNotificationAction = testNotificationAction;

        Text = "设置 - GPU 观察";
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = false;
        MinimizeBox = false;
        ShowInTaskbar = true;
        StartPosition = FormStartPosition.CenterScreen;
        AutoScaleMode = AutoScaleMode.Dpi;
        BackColor = Theme.WindowBackground;
        Font = Theme.UiFont(9f);
        ClientSize = new Size(680, 580);

        _add = MakeButton("添加服务器", 450, 106, 150, 32);
        _edit = MakeButton("编辑", 450, 146, 150, 32);
        _delete = MakeButton("删除", 450, 186, 150, 32, danger: true);
        _save = MakeButton("保存", 570, 532, 86, 34, primary: true);
        _cancel = MakeButton("取消", 474, 532, 86, 34);
        _testNotification = MakeButton("发送测试通知", 450, 420, 150, 32);

        AcceptButton = _save;
        CancelButton = _cancel;

        var title = new Label
        {
            Text = "设置",
            AutoSize = true,
            Location = new Point(24, 16),
            Font = Theme.UiFont(15f, FontStyle.Bold),
            ForeColor = Theme.Text
        };
        Controls.Add(title);

        var subtitle = new Label
        {
            Text = "管理监控服务器与通知选项。每个服务器的通知开关在编辑服务器中设置。",
            AutoSize = true,
            Location = new Point(24, 48),
            Font = Theme.UiFont(8.25f),
            ForeColor = Theme.MutedText
        };
        Controls.Add(subtitle);

        Controls.Add(new Label
        {
            Text = "服务器",
            AutoSize = true,
            Location = new Point(24, 82),
            Font = Theme.UiFont(10f, FontStyle.Bold),
            ForeColor = Theme.Text
        });

        _serverList.Location = new Point(24, 106);
        _serverList.Size = new Size(410, 210);
        _serverList.Font = Theme.UiFont(9.25f);
        _serverList.BorderStyle = BorderStyle.FixedSingle;
        _serverList.IntegralHeight = false;
        _serverList.SelectedIndexChanged += (_, _) => UpdateButtonStates();
        Controls.Add(_serverList);

        Controls.Add(_add);
        Controls.Add(_edit);
        Controls.Add(_delete);

        var serverHint = new Label
        {
            Text = "提示：服务器标签显示服务器地址；通知按服务器单独开关。",
            AutoSize = true,
            Location = new Point(24, 324),
            Font = Theme.UiFont(8.25f),
            ForeColor = Theme.MutedText
        };
        Controls.Add(serverHint);

        Controls.Add(new Label
        {
            Text = "常规",
            AutoSize = true,
            Location = new Point(24, 356),
            Font = Theme.UiFont(10f, FontStyle.Bold),
            ForeColor = Theme.Text
        });

        Controls.Add(new Label
        {
            Text = "刷新间隔",
            AutoSize = true,
            Location = new Point(24, 390),
            Font = Theme.UiFont(8.75f),
            ForeColor = Theme.SecondaryText
        });

        _refresh.Location = new Point(120, 386);
        _refresh.Size = new Size(80, 26);
        _refresh.Minimum = 2;
        _refresh.Maximum = 300;
        _refresh.Value = Math.Clamp(_working.RefreshSeconds, 2, 300);
        _refresh.BorderStyle = BorderStyle.FixedSingle;
        _refresh.Font = Theme.UiFont(9f);
        Controls.Add(_refresh);
        Controls.Add(new Label
        {
            Text = "秒",
            AutoSize = true,
            Location = new Point(206, 390),
            Font = Theme.UiFont(8.75f),
            ForeColor = Theme.SecondaryText
        });
        Controls.Add(new Label
        {
            Text = "2–300 秒；所有服务器共用。",
            AutoSize = true,
            Location = new Point(238, 390),
            Font = Theme.UiFont(8.25f),
            ForeColor = Theme.MutedText
        });

        _notifications.Text = "启用 GPU 空闲通知";
        _notifications.AutoSize = true;
        _notifications.Location = new Point(24, 424);
        _notifications.Font = Theme.UiFont(8.75f);
        _notifications.Checked = _working.NotificationsEnabled;
        Controls.Add(_notifications);

        _autostart.Text = "开机时自动启动 GPU 观察";
        _autostart.AutoSize = true;
        _autostart.Location = new Point(24, 452);
        _autostart.Font = Theme.UiFont(8.75f);
        _autostart.Checked = _working.StartWithWindows;
        Controls.Add(_autostart);

        Controls.Add(_testNotification);
        _testHint.Location = new Point(450, 456);
        _testHint.Size = new Size(200, 34);
        _testHint.Font = Theme.UiFont(8f);
        _testHint.ForeColor = Theme.MutedText;
        Controls.Add(_testHint);

        var note = new Label
        {
            Text = "采样数据只在内存中保留最近 300 秒，并用于面板趋势图；不会写入磁盘。",
            AutoSize = false,
            Location = new Point(24, 492),
            Size = new Size(630, 20),
            Font = Theme.UiFont(8.25f),
            ForeColor = Theme.MutedText
        };
        Controls.Add(note);

        Controls.Add(_save);
        Controls.Add(_cancel);

        _add.Click += (_, _) => AddServer();
        _edit.Click += (_, _) => EditServer();
        _delete.Click += (_, _) => DeleteServer();
        _testNotification.Click += (_, _) =>
        {
            _testHint.Text = "已发送测试通知。";
            try
            {
                _testNotificationAction();
            }
            catch (Exception ex)
            {
                _testHint.Text = $"发送失败：{ex.Message}";
            }
        };
        _save.Click += (_, _) => SaveAndClose();
        _cancel.Click += (_, _) => { DialogResult = DialogResult.Cancel; Close(); };
        _serverList.DoubleClick += (_, _) => EditServer();

        RefreshServerList(0);
    }

    public AppConfig? ResultConfig { get; private set; }

    private void RefreshServerList(int selectedIndex)
    {
        _serverList.BeginUpdate();
        try
        {
            _serverList.Items.Clear();
            foreach (var server in _servers)
            {
                var label = string.IsNullOrWhiteSpace(server.Name)
                    ? server.DisplayUserHost
                    : $"{server.Name} · {server.DisplayUserHost}";
                var suffix = server.NotificationsEnabled ? "" : "  ·  通知关闭";
                _serverList.Items.Add($"{label}{suffix}");
            }
        }
        finally
        {
            _serverList.EndUpdate();
        }

        if (_servers.Count > 0)
        {
            _serverList.SelectedIndex = Math.Clamp(selectedIndex, 0, _servers.Count - 1);
        }

        UpdateButtonStates();
    }

    private void UpdateButtonStates()
    {
        var hasSelection = _serverList.SelectedIndex >= 0 && _serverList.SelectedIndex < _servers.Count;
        _edit.Enabled = hasSelection;
        _delete.Enabled = hasSelection;
    }

    private void AddServer()
    {
        using var dialog = new ServerEditForm(null);
        if (dialog.ShowDialog(this) != DialogResult.OK || dialog.ResultConfig is null)
        {
            return;
        }

        _servers.Add(dialog.ResultConfig);
        RefreshServerList(_servers.Count - 1);
    }

    private void EditServer()
    {
        var index = _serverList.SelectedIndex;
        if (index < 0 || index >= _servers.Count)
        {
            return;
        }

        using var dialog = new ServerEditForm(_servers[index].Clone());
        if (dialog.ShowDialog(this) != DialogResult.OK || dialog.ResultConfig is null)
        {
            return;
        }

        _servers[index] = dialog.ResultConfig;
        RefreshServerList(index);
    }

    private void DeleteServer()
    {
        var index = _serverList.SelectedIndex;
        if (index < 0 || index >= _servers.Count)
        {
            return;
        }

        var server = _servers[index];
        var answer = MessageBox.Show(
            this,
            $"确定删除服务器 {server.DisplayUserHost} 吗？",
            "删除服务器",
            MessageBoxButtons.YesNo,
            MessageBoxIcon.Question,
            MessageBoxDefaultButton.Button2);
        if (answer != DialogResult.Yes)
        {
            return;
        }

        _servers.RemoveAt(index);
        RefreshServerList(Math.Min(index, _servers.Count - 1));
    }

    private void SaveAndClose()
    {
        _working.Servers = _servers.Select(server => server.Clone()).ToList();
        _working.RefreshSeconds = (int)_refresh.Value;
        _working.NotificationsEnabled = _notifications.Checked;
        _working.StartWithWindows = _autostart.Checked;
        ResultConfig = _working;
        DialogResult = DialogResult.OK;
        Close();
    }

    private Button MakeButton(string text, int x, int y, int width, int height, bool primary = false, bool danger = false)
    {
        var button = new Button
        {
            Text = text,
            Location = new Point(x, y),
            Size = new Size(width, height),
            Font = Theme.UiFont(8.75f),
            FlatStyle = FlatStyle.Flat,
            BackColor = primary ? Theme.Accent : Color.White,
            ForeColor = primary ? Color.White : danger ? Theme.Red : Theme.Text,
            UseVisualStyleBackColor = false,
            Cursor = Cursors.Hand
        };
        button.FlatAppearance.BorderColor = primary ? Theme.Accent : Theme.BorderStrong;
        return button;
    }
}
