using GPUObserver.Config;
using GPUObserver.Ssh;

namespace GPUObserver.UI;

internal sealed class ServerEditForm : Form
{
    private readonly ServerConfig? _existing;
    private readonly TextBox _name = new();
    private readonly TextBox _host = new();
    private readonly NumericUpDown _port = new();
    private readonly TextBox _username = new();
    private readonly ComboBox _authType = new();
    private readonly TextBox _password = new();
    private readonly CheckBox _showPassword = new();
    private readonly Label _passwordLabel;
    private readonly TextBox _keyPath = new();
    private readonly Button _browse;
    private readonly Label _keyLabel;
    private readonly TextBox _passphrase = new();
    private readonly CheckBox _showPassphrase = new();
    private readonly Label _passphraseLabel;
    private readonly CheckBox _notifications = new();
    private readonly TextBox _fingerprint = new();
    private readonly Button _clearFingerprint;
    private readonly Button _test;
    private readonly Label _testResult = new();
    private readonly Label _error = new();
    private readonly Button _save;
    private readonly Button _cancel;
    private readonly CancellationTokenSource _lifetime = new();

    private string? _fingerprintValue;
    private bool _loading = true;
    private bool _testing;

    public ServerEditForm(ServerConfig? existing)
    {
        _existing = existing;
        Text = existing is null ? "添加服务器" : "编辑服务器";
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = false;
        MinimizeBox = false;
        ShowInTaskbar = true;
        StartPosition = FormStartPosition.CenterScreen;
        AutoScaleMode = AutoScaleMode.Dpi;
        BackColor = Theme.WindowBackground;
        Font = Theme.UiFont(9f);
        ClientSize = new Size(600, 588);

        _passwordLabel = MakeLabel("密码", 24, 234);
        _keyLabel = MakeLabel("私钥文件", 24, 234);
        _passphraseLabel = MakeLabel("私钥口令", 24, 270);
        _browse = MakeButton("浏览…", 428, 230, 70, 26);
        _save = MakeButton("保存", 480, 536, 96, 34, primary: true);
        _cancel = MakeButton("取消", 384, 536, 86, 34);
        _clearFingerprint = MakeButton("重新信任", 428, 342, 148, 26);
        _test = MakeButton("测试连接", 120, 382, 110, 32);

        AcceptButton = _save;
        CancelButton = _cancel;

        var title = new Label
        {
            Text = existing is null ? "添加服务器" : "编辑服务器",
            AutoSize = true,
            Location = new Point(24, 16),
            Font = Theme.UiFont(15f, FontStyle.Bold),
            ForeColor = Theme.Text
        };
        Controls.Add(title);

        var subtitle = new Label
        {
            Text = "SSH 连接与密钥读取完全内置，不依赖系统 OpenSSH 或 ssh-agent。",
            AutoSize = true,
            Location = new Point(24, 48),
            Font = Theme.UiFont(8.25f),
            ForeColor = Theme.MutedText
        };
        Controls.Add(subtitle);

        MakeLabel("名称（可选）", 24, 86);
        ConfigureTextBox(_name, 120, 82, 456);
        _name.PlaceholderText = "留空则标签显示主机/IP";
        Controls.Add(_name);

        MakeLabel("主机 / IP", 24, 122);
        ConfigureTextBox(_host, 120, 118, 456);
        Controls.Add(_host);

        MakeLabel("端口", 24, 158);
        _port.Location = new Point(120, 154);
        _port.Size = new Size(80, 26);
        _port.Minimum = 1;
        _port.Maximum = 65535;
        _port.Value = 22;
        _port.BorderStyle = BorderStyle.FixedSingle;
        _port.Font = Theme.UiFont(9f);
        Controls.Add(_port);

        MakeLabel("用户名", 216, 158);
        ConfigureTextBox(_username, 288, 154, 288);
        Controls.Add(_username);

        MakeLabel("认证方式", 24, 194);
        _authType.Location = new Point(120, 190);
        _authType.Size = new Size(160, 26);
        _authType.DropDownStyle = ComboBoxStyle.DropDownList;
        _authType.Font = Theme.UiFont(9f);
        _authType.Items.AddRange(["密码", "私钥文件"]);
        Controls.Add(_authType);

        Controls.Add(_passwordLabel);
        ConfigureTextBox(_password, 120, 230, 340);
        _password.UseSystemPasswordChar = true;
        Controls.Add(_password);
        _showPassword.Text = "显示";
        _showPassword.AutoSize = true;
        _showPassword.Location = new Point(470, 233);
        _showPassword.Font = Theme.UiFont(8.25f);
        Controls.Add(_showPassword);

        Controls.Add(_keyLabel);
        ConfigureTextBox(_keyPath, 120, 230, 300);
        Controls.Add(_keyPath);
        Controls.Add(_browse);
        Controls.Add(_passphraseLabel);
        ConfigureTextBox(_passphrase, 120, 266, 340);
        _passphrase.UseSystemPasswordChar = true;
        Controls.Add(_passphrase);
        _showPassphrase.Text = "显示";
        _showPassphrase.AutoSize = true;
        _showPassphrase.Location = new Point(470, 269);
        _showPassphrase.Font = Theme.UiFont(8.25f);
        Controls.Add(_showPassphrase);

        _notifications.Text = "在 GPU 空闲时通知我（当前用户进程全部退出）";
        _notifications.AutoSize = true;
        _notifications.Location = new Point(24, 310);
        _notifications.Font = Theme.UiFont(8.75f);
        _notifications.Checked = true;
        Controls.Add(_notifications);

        MakeLabel("主机密钥", 24, 346);
        ConfigureTextBox(_fingerprint, 120, 342, 300);
        _fingerprint.ReadOnly = true;
        _fingerprint.BackColor = Color.White;
        _fingerprint.Font = Theme.MonoFont(8.25f);
        Controls.Add(_fingerprint);
        Controls.Add(_clearFingerprint);

        Controls.Add(_test);
        _testResult.Location = new Point(242, 378);
        _testResult.Size = new Size(334, 48);
        _testResult.Font = Theme.UiFont(8.25f);
        _testResult.ForeColor = Theme.SecondaryText;
        _testResult.AutoSize = false;
        Controls.Add(_testResult);

        _error.Location = new Point(24, 430);
        _error.Size = new Size(552, 20);
        _error.Font = Theme.UiFont(8.25f);
        _error.ForeColor = Theme.Red;
        _error.AutoSize = false;
        Controls.Add(_error);

        var note = new Label
        {
            Text = "密码与口令使用 Windows DPAPI 加密保存；私钥只保存路径，解锁后的私钥仅存在于当前进程内存中。",
            AutoSize = false,
            Location = new Point(24, 456),
            Size = new Size(552, 34),
            Font = Theme.UiFont(8.25f),
            ForeColor = Theme.MutedText
        };
        Controls.Add(note);

        Controls.Add(_save);
        Controls.Add(_cancel);

        _showPassword.CheckedChanged += (_, _) => _password.UseSystemPasswordChar = !_showPassword.Checked;
        _showPassphrase.CheckedChanged += (_, _) => _passphrase.UseSystemPasswordChar = !_showPassphrase.Checked;
        _authType.SelectedIndexChanged += (_, _) => UpdateAuthVisibility();
        _browse.Click += (_, _) => BrowseKey();
        _clearFingerprint.Click += (_, _) => ClearFingerprint();
        _test.Click += async (_, _) => await TestConnectionAsync();
        _save.Click += (_, _) => SaveAndClose();
        _cancel.Click += (_, _) => { DialogResult = DialogResult.Cancel; Close(); };
        _host.TextChanged += (_, _) => InvalidateFingerprintOnTargetChange();
        _port.ValueChanged += (_, _) => InvalidateFingerprintOnTargetChange();

        LoadValues();
    }

    public ServerConfig? ResultConfig { get; private set; }

    protected override void OnFormClosing(FormClosingEventArgs e)
    {
        if (!_lifetime.IsCancellationRequested)
        {
            _lifetime.Cancel();
        }

        base.OnFormClosing(e);
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _lifetime.Dispose();
        }

        base.Dispose(disposing);
    }

    private void LoadValues()
    {
        _loading = true;

        if (_existing is not null)
        {
            _name.Text = _existing.Name ?? "";
            _host.Text = _existing.Host;
            _port.Value = Math.Clamp(_existing.Port, 1, 65535);
            _username.Text = _existing.Username;
            _authType.SelectedIndex = _existing.AuthType == ServerAuthType.PrivateKey ? 1 : 0;
            _keyPath.Text = _existing.PrivateKeyPath ?? "";
            _notifications.Checked = _existing.NotificationsEnabled;
            _fingerprintValue = NormalizeFingerprint(_existing.HostKeyFingerprintSha256);

            if (!string.IsNullOrEmpty(_existing.ProtectedPassword))
            {
                if (SecretProtector.TryUnprotect(_existing.ProtectedPassword, out var password))
                {
                    _password.Text = password;
                }
                else
                {
                    _error.Text = "已保存的密码无法解密，请重新输入。";
                }
            }

            if (!string.IsNullOrEmpty(_existing.ProtectedKeyPassphrase))
            {
                if (SecretProtector.TryUnprotect(_existing.ProtectedKeyPassphrase, out var passphrase))
                {
                    _passphrase.Text = passphrase;
                }
                else if (string.IsNullOrEmpty(_error.Text))
                {
                    _error.Text = "已保存的私钥口令无法解密，请重新输入。";
                }
            }
        }
        else
        {
            _authType.SelectedIndex = 0;
        }

        UpdateFingerprintDisplay();
        UpdateAuthVisibility();
        _loading = false;
    }

    private void UpdateAuthVisibility()
    {
        var useKey = _authType.SelectedIndex == 1;
        _passwordLabel.Visible = !useKey;
        _password.Visible = !useKey;
        _showPassword.Visible = !useKey;
        _keyLabel.Visible = useKey;
        _keyPath.Visible = useKey;
        _browse.Visible = useKey;
        _passphraseLabel.Visible = useKey;
        _passphrase.Visible = useKey;
        _showPassphrase.Visible = useKey;
    }

    private void BrowseKey()
    {
        using var dialog = new OpenFileDialog
        {
            Title = "选择 SSH 私钥文件",
            Filter = "私钥文件|*.pem;*.key;id_rsa*;id_ed25519*;*.openssh;*.ppk|所有文件|*.*",
            CheckFileExists = true,
            Multiselect = false
        };

        if (dialog.ShowDialog(this) == DialogResult.OK)
        {
            _keyPath.Text = dialog.FileName;
        }
    }

    private void ClearFingerprint()
    {
        var answer = MessageBox.Show(
            this,
            "清除已保存的主机密钥指纹后，下次连接会重新信任服务器提供的主机密钥。仅在确认服务器重装或密钥变更时执行。\r\n\r\n继续？",
            "重新信任主机密钥",
            MessageBoxButtons.YesNo,
            MessageBoxIcon.Warning,
            MessageBoxDefaultButton.Button2);
        if (answer != DialogResult.Yes)
        {
            return;
        }

        _fingerprintValue = null;
        UpdateFingerprintDisplay();
    }

    private void InvalidateFingerprintOnTargetChange()
    {
        if (_loading || _fingerprintValue is null)
        {
            return;
        }

        _fingerprintValue = null;
        UpdateFingerprintDisplay();
    }

    private void UpdateFingerprintDisplay()
    {
        _fingerprint.Text = string.IsNullOrWhiteSpace(_fingerprintValue)
            ? "首次连接时自动记录"
            : "SHA256:" + _fingerprintValue;
        _clearFingerprint.Enabled = !string.IsNullOrWhiteSpace(_fingerprintValue);
    }

    private async Task TestConnectionAsync()
    {
        if (_testing)
        {
            return;
        }

        if (!TryBuildConfig(out var config))
        {
            return;
        }

        _testing = true;
        _test.Enabled = false;
        _save.Enabled = false;
        _testResult.ForeColor = Theme.SecondaryText;
        _testResult.Text = "正在连接并读取 nvidia-smi…";

        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(35));
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(_lifetime.Token, timeout.Token);

        try
        {
            var result = await ConnectionTester.TestAsync(config, linked.Token);
            if (IsDisposed)
            {
                return;
            }

            if (!string.IsNullOrWhiteSpace(result.Fingerprint))
            {
                _fingerprintValue = NormalizeFingerprint(result.Fingerprint);
                UpdateFingerprintDisplay();
            }

            _testResult.ForeColor = result.Connected
                ? (result.Error is null ? Theme.Green : Theme.Amber)
                : Theme.Red;
            _testResult.Text = result.Message;
            if (result.Error?.Kind == GPUObserver.Monitoring.MonitorErrorKind.HostKey
                && !string.IsNullOrWhiteSpace(result.Error.Detail))
            {
                _testResult.Text += $"\r\n新指纹：SHA256:{result.Error.Detail}";
            }
            else if (result.Error is not null && result.Connected)
            {
                _testResult.Text += " 可保存后稍后重试。";
            }
        }
        catch (OperationCanceledException)
        {
            if (!IsDisposed)
            {
                _testResult.ForeColor = Theme.Amber;
                _testResult.Text = "连接测试已取消或超时。";
            }
        }
        catch (Exception ex)
        {
            if (!IsDisposed)
            {
                _testResult.ForeColor = Theme.Red;
                _testResult.Text = $"连接测试失败：{ex.Message}";
            }
        }
        finally
        {
            _testing = false;
            if (!IsDisposed)
            {
                _test.Enabled = true;
                _save.Enabled = true;
            }
        }
    }

    private void SaveAndClose()
    {
        if (_testing || !TryBuildConfig(out var config))
        {
            return;
        }

        ResultConfig = config;
        DialogResult = DialogResult.OK;
        Close();
    }

    private bool TryBuildConfig(out ServerConfig config)
    {
        config = _existing?.Clone() ?? new ServerConfig();
        _error.Text = "";

        var host = _host.Text.Trim();
        if (host.Length == 0)
        {
            _error.Text = "请填写服务器主机或 IP。";
            return false;
        }

        var username = _username.Text.Trim();
        if (username.Length == 0)
        {
            _error.Text = "请填写 SSH 用户名。";
            return false;
        }

        var useKey = _authType.SelectedIndex == 1;
        var keyPath = _keyPath.Text.Trim();
        if (useKey)
        {
            if (keyPath.Length == 0)
            {
                _error.Text = "请选择私钥文件。";
                return false;
            }

            if (!File.Exists(keyPath))
            {
                _error.Text = "私钥文件不存在。";
                return false;
            }

            if (keyPath.EndsWith(".ppk", StringComparison.OrdinalIgnoreCase))
            {
                _error.Text = "不支持 PuTTY .ppk 私钥，请转换为 OpenSSH 格式。";
                return false;
            }
        }
        else if (_password.Text.Length == 0)
        {
            _error.Text = "请填写登录密码。";
            return false;
        }

        try
        {
            config.Name = string.IsNullOrWhiteSpace(_name.Text) ? null : _name.Text.Trim();
            config.Host = host;
            config.Port = (int)_port.Value;
            config.Username = username;
            config.AuthType = useKey ? ServerAuthType.PrivateKey : ServerAuthType.Password;
            config.ProtectedPassword = SecretProtector.Protect(_password.Text);
            config.PrivateKeyPath = keyPath.Length == 0 ? null : keyPath;
            config.ProtectedKeyPassphrase = SecretProtector.Protect(_passphrase.Text);
            config.NotificationsEnabled = _notifications.Checked;
            config.HostKeyFingerprintSha256 = _fingerprintValue;
            return true;
        }
        catch (Exception ex)
        {
            _error.Text = $"无法加密凭据：{ex.Message}";
            return false;
        }
    }

    private Label MakeLabel(string text, int x, int y)
    {
        var label = new Label
        {
            Text = text,
            AutoSize = true,
            Location = new Point(x, y),
            Font = Theme.UiFont(8.75f),
            ForeColor = Theme.SecondaryText
        };
        Controls.Add(label);
        return label;
    }

    private static void ConfigureTextBox(TextBox textBox, int x, int y, int width)
    {
        textBox.Location = new Point(x, y);
        textBox.Size = new Size(width, 26);
        textBox.BorderStyle = BorderStyle.FixedSingle;
        textBox.Font = Theme.UiFont(9f);
        textBox.BackColor = Color.White;
    }

    private Button MakeButton(string text, int x, int y, int width, int height, bool primary = false)
    {
        var button = new Button
        {
            Text = text,
            Location = new Point(x, y),
            Size = new Size(width, height),
            Font = Theme.UiFont(8.75f),
            FlatStyle = FlatStyle.Flat,
            BackColor = primary ? Theme.Accent : Color.White,
            ForeColor = primary ? Color.White : Theme.Text,
            UseVisualStyleBackColor = false,
            Cursor = Cursors.Hand
        };
        button.FlatAppearance.BorderColor = primary ? Theme.Accent : Theme.BorderStrong;
        return button;
    }

    private static string? NormalizeFingerprint(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return null;
        }

        var trimmed = value.Trim();
        return trimmed.StartsWith("SHA256:", StringComparison.OrdinalIgnoreCase)
            ? trimmed[7..].Trim()
            : trimmed;
    }
}
