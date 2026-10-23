using System.Net.Sockets;
using GPUObserver.Config;
using GPUObserver.Monitoring;
using Renci.SshNet;
using Renci.SshNet.Common;

namespace GPUObserver.Ssh;

public sealed class SshSession : IDisposable
{
    private static readonly TimeSpan ConnectTimeout = TimeSpan.FromSeconds(10);
    private static readonly TimeSpan DefaultCommandTimeout = TimeSpan.FromSeconds(15);

    private readonly ServerConfig _config;
    private readonly Action<string> _hostKeyLearned;

    private SshClient? _client;
    private string? _observedFingerprint;
    private bool _hostKeyMismatch;
    private string? _mismatchObserved;

    public SshSession(ServerConfig config, Action<string> hostKeyLearned)
    {
        _config = config;
        _hostKeyLearned = hostKeyLearned;
    }

    public bool IsConnected => _client?.IsConnected == true;

    public async Task ConnectAsync(CancellationToken cancellationToken)
    {
        DisposeClient();

        var authMethods = BuildAuthenticationMethods();
        var connectionInfo = new ConnectionInfo(_config.Host, _config.Port, _config.Username, authMethods)
        {
            Timeout = ConnectTimeout,
            ChannelCloseTimeout = TimeSpan.FromSeconds(10)
        };

        var client = new SshClient(connectionInfo)
        {
            KeepAliveInterval = TimeSpan.FromSeconds(30)
        };
        client.HostKeyReceived += OnHostKeyReceived;

        _observedFingerprint = null;
        _hostKeyMismatch = false;
        _mismatchObserved = null;

        try
        {
            await client.ConnectAsync(cancellationToken).ConfigureAwait(false);
            if (!client.IsConnected)
            {
                throw new MonitorOperationException(MonitorErrorKind.Network, "SSH 连接未建立。");
            }

            _client = client;

            var pinned = NormalizeFingerprint(_config.HostKeyFingerprintSha256);
            if (string.IsNullOrEmpty(pinned) && !string.IsNullOrEmpty(_observedFingerprint))
            {
                _config.HostKeyFingerprintSha256 = _observedFingerprint;
                try
                {
                    _hostKeyLearned(_observedFingerprint);
                }
                catch (Exception ex)
                {
                    Util.Log.Error("保存主机密钥指纹失败。", ex);
                }
            }
        }
        catch (OperationCanceledException)
        {
            client.Dispose();
            throw;
        }
        catch (MonitorOperationException)
        {
            client.Dispose();
            throw;
        }
        catch (Exception ex)
        {
            client.Dispose();
            if (_hostKeyMismatch)
            {
                throw new MonitorOperationException(
                    MonitorErrorKind.HostKey,
                    "服务器主机密钥已更改。为安全起见已阻断连接，请在设置中确认后再重新信任。",
                    _mismatchObserved,
                    ex);
            }

            throw MapConnectException(ex);
        }
    }

    public async Task<CommandResult> RunAsync(string commandText, CancellationToken cancellationToken, TimeSpan? timeout = null)
    {
        var client = _client;
        if (client is null || !client.IsConnected)
        {
            throw new MonitorOperationException(MonitorErrorKind.Network, "SSH 连接已断开。");
        }

        using var command = client.CreateCommand(commandText);
        command.CommandTimeout = timeout ?? DefaultCommandTimeout;
        await command.ExecuteAsync(cancellationToken).ConfigureAwait(false);

        return new CommandResult(command.Result ?? "", command.Error ?? "", command.ExitStatus);
    }

    public void Dispose() => DisposeClient();

    private AuthenticationMethod[] BuildAuthenticationMethods()
    {
        if (_config.AuthType == ServerAuthType.PrivateKey)
        {
            var passphrase = DecryptSecret(_config.ProtectedKeyPassphrase, "私钥口令");
            var key = PrivateKeyLoader.Load(_config.PrivateKeyPath ?? "", passphrase);
            return [new PrivateKeyAuthenticationMethod(_config.Username, key)];
        }

        var password = DecryptSecret(_config.ProtectedPassword, "登录密码");
        if (string.IsNullOrEmpty(password))
        {
            throw new MonitorOperationException(MonitorErrorKind.Credential, "未填写登录密码。");
        }

        var keyboard = new KeyboardInteractiveAuthenticationMethod(_config.Username);
        keyboard.AuthenticationPrompt += (_, args) =>
        {
            foreach (var prompt in args.Prompts)
            {
                prompt.Response = password;
            }
        };

        return
        [
            new PasswordAuthenticationMethod(_config.Username, password),
            keyboard
        ];
    }

    private static string DecryptSecret(string? protectedValue, string label)
    {
        if (string.IsNullOrEmpty(protectedValue))
        {
            return "";
        }

        try
        {
            return SecretProtector.Unprotect(protectedValue);
        }
        catch (InvalidOperationException ex)
        {
            throw new MonitorOperationException(
                MonitorErrorKind.Credential,
                $"无法解密已保存的{label}，请在设置中重新输入。",
                ex.Message,
                ex);
        }
    }

    private void OnHostKeyReceived(object? sender, HostKeyEventArgs e)
    {
        _observedFingerprint = NormalizeFingerprint(e.FingerPrintSHA256);
        var pinned = NormalizeFingerprint(_config.HostKeyFingerprintSha256);
        if (string.IsNullOrEmpty(pinned))
        {
            e.CanTrust = true;
            return;
        }

        if (!string.Equals(pinned, _observedFingerprint, StringComparison.Ordinal))
        {
            _hostKeyMismatch = true;
            _mismatchObserved = _observedFingerprint;
            e.CanTrust = false;
        }
    }

    private static string? NormalizeFingerprint(string? fingerprint)
    {
        if (string.IsNullOrWhiteSpace(fingerprint))
        {
            return null;
        }

        var value = fingerprint.Trim();
        return value.StartsWith("SHA256:", StringComparison.OrdinalIgnoreCase)
            ? value[7..].Trim()
            : value;
    }

    private static MonitorOperationException MapConnectException(Exception exception)
    {
        var detail = exception.Message;

        if (exception is SshAuthenticationException)
        {
            return new MonitorOperationException(
                MonitorErrorKind.Authentication,
                "SSH 认证失败，请检查用户名、密码或私钥。",
                detail,
                exception);
        }

        if (exception is SshOperationTimeoutException || exception is TimeoutException)
        {
            return new MonitorOperationException(
                MonitorErrorKind.Timeout,
                "连接服务器超时。",
                detail,
                exception);
        }

        if (exception is SocketException || exception is SshConnectionException || exception is IOException)
        {
            return new MonitorOperationException(
                MonitorErrorKind.Network,
                "无法连接服务器，请检查地址、端口和网络。",
                detail,
                exception);
        }

        if (exception is SshException && detail.Contains("host key", StringComparison.OrdinalIgnoreCase))
        {
            return new MonitorOperationException(
                MonitorErrorKind.HostKey,
                "服务器主机密钥验证失败。",
                detail,
                exception);
        }

        return new MonitorOperationException(
            MonitorErrorKind.Unknown,
            "SSH 连接失败。",
            detail,
            exception);
    }

    private void DisposeClient()
    {
        var client = _client;
        _client = null;
        if (client is null)
        {
            return;
        }

        client.HostKeyReceived -= OnHostKeyReceived;
        try
        {
            client.Dispose();
        }
        catch
        {
            // ignored
        }
    }
}
