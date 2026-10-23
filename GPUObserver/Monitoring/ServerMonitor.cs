using System.Net.Sockets;
using GPUObserver.Config;
using GPUObserver.Monitoring.Parsing;
using GPUObserver.Ssh;
using GPUObserver.Util;
using Renci.SshNet.Common;

namespace GPUObserver.Monitoring;

public sealed class ServerMonitor : IDisposable
{
    private readonly ServerConfig _config;
    private readonly int _refreshSeconds;
    private readonly Action<Guid, string> _hostKeyLearned;
    private readonly SampleQueue _queue = new();
    private readonly CancellationTokenSource _cts = new();
    private readonly object _statusGate = new();
    private readonly object _wakeGate = new();

    private SshSession? _session;
    private Task? _loop;
    private TaskCompletionSource<bool>? _wakeTcs;
    private bool _refreshPending;
    private MonitorStatus _status;
    private string? _lastLoggedError;
    private bool _disposed;

    public ServerMonitor(ServerConfig config, int refreshSeconds, Action<Guid, string> hostKeyLearned)
    {
        _config = config.Clone();
        _refreshSeconds = Math.Clamp(refreshSeconds, 2, 300);
        _hostKeyLearned = hostKeyLearned;
        _status = MonitorStatus.Stopped(_config.Id);
    }

    public Guid ServerId => _config.Id;

    public ServerConfig Config => _config;

    public MonitorStatus Status
    {
        get
        {
            lock (_statusGate)
            {
                return _status;
            }
        }
    }

    public event EventHandler<ServerSnapshot>? SnapshotReceived;

    public event EventHandler<MonitorStatus>? StatusChanged;

    public void Start()
    {
        if (_loop is not null || _disposed)
        {
            return;
        }

        UpdateStatus(MonitorStatus.Connecting(_config.Id));
        _loop = RunAsync();
        _ = _loop.ContinueWith(
            _ => _cts.Dispose(),
            TaskScheduler.Default);
    }

    public void RequestRefresh()
    {
        if (_disposed)
        {
            return;
        }

        TaskCompletionSource<bool>? wake;
        lock (_wakeGate)
        {
            _refreshPending = true;
            wake = _wakeTcs;
            _wakeTcs = null;
        }

        wake?.TrySetResult(true);
    }

    public void PruneHistory() => _queue.Prune();

    public IReadOnlyList<ServerSnapshot> GetHistory() => _queue.GetSnapshots();

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }

        _disposed = true;
        _queue.Clear();
        try
        {
            _cts.Cancel();
        }
        catch (ObjectDisposedException)
        {
            // Already finished.
        }

        if (_loop is null)
        {
            _cts.Dispose();
        }
    }

    private async Task RunAsync()
    {
        var backoff = TimeSpan.FromSeconds(5);

        try
        {
            while (!_cts.IsCancellationRequested)
            {
                var success = false;
                try
                {
                    if (!IsSessionConnected())
                    {
                        UpdateStatus(MonitorStatus.Connecting(_config.Id));
                    }

                    await EnsureConnectedAsync(_cts.Token).ConfigureAwait(false);
                    var snapshot = await PollAsync(_cts.Token).ConfigureAwait(false);
                    _queue.Enqueue(snapshot);
                    backoff = TimeSpan.FromSeconds(5);
                    success = true;
                    _lastLoggedError = null;
                    UpdateStatus(MonitorStatus.Online(_config.Id));
                    SnapshotReceived?.Invoke(this, snapshot);
                }
                catch (OperationCanceledException) when (_cts.IsCancellationRequested)
                {
                    break;
                }
                catch (Exception ex)
                {
                    var error = MapError(ex);
                    DisposeSession();

                    if (!string.Equals(_lastLoggedError, error.Message, StringComparison.Ordinal))
                    {
                        _lastLoggedError = error.Message;
                        Log.Error($"服务器 {_config.DisplayHost} 监控失败：{error.Message}", ex);
                    }

                    UpdateStatus(MonitorStatus.Failed(_config.Id, error));

                    var isPersistentConfigurationError = error.Kind is MonitorErrorKind.Authentication
                        or MonitorErrorKind.HostKey
                        or MonitorErrorKind.Credential
                        or MonitorErrorKind.PrivateKey;

                    backoff = TimeSpan.FromSeconds(Math.Min(backoff.TotalSeconds * 2, 60));
                    if (isPersistentConfigurationError)
                    {
                        backoff = TimeSpan.FromSeconds(60);
                    }
                }

                var delay = success ? TimeSpan.FromSeconds(_refreshSeconds) : backoff;
                await DelayWithWakeAsync(delay, _cts.Token).ConfigureAwait(false);
            }
        }
        catch (OperationCanceledException) when (_cts.IsCancellationRequested)
        {
            // normal shutdown
        }
        catch (Exception ex)
        {
            Log.Error($"服务器 {_config.DisplayHost} 监控循环意外退出。", ex);
        }
        finally
        {
            DisposeSession();
            _queue.Prune();
            UpdateStatus(MonitorStatus.Stopped(_config.Id));
        }
    }

    private async Task EnsureConnectedAsync(CancellationToken cancellationToken)
    {
        if (_session?.IsConnected == true)
        {
            return;
        }

        DisposeSession();
        var session = new SshSession(
            _config,
            fingerprint => _hostKeyLearned(_config.Id, fingerprint));

        try
        {
            await session.ConnectAsync(cancellationToken).ConfigureAwait(false);
            _session = session;
        }
        catch
        {
            session.Dispose();
            throw;
        }
    }

    private async Task<ServerSnapshot> PollAsync(CancellationToken cancellationToken)
    {
        var session = _session ?? throw new MonitorOperationException(MonitorErrorKind.Network, "SSH 连接已断开。");

        var mainResult = await session
            .RunAsync(ProbeCommand.MainScript, cancellationToken, TimeSpan.FromSeconds(20))
            .ConfigureAwait(false);
        var probe = ProbeParser.Parse(mainResult.StandardOutput);

        if (probe.NvSmiMissing)
        {
            throw new MonitorOperationException(
                MonitorErrorKind.NvidiaSmiMissing,
                "未找到 nvidia-smi，请确认服务器已安装 NVIDIA 驱动。",
                nullIfEmpty(mainResult.StandardError));
        }

        if (probe.Gpus.Count == 0)
        {
            throw new MonitorOperationException(
                MonitorErrorKind.Command,
                "未读取到 GPU 信息，请检查 NVIDIA 驱动和 nvidia-smi。",
                nullIfEmpty(mainResult.StandardError));
        }

        var pids = probe.CollectGpuPids().ToArray();
        var psSucceeded = true;
        var psRecords = new Dictionary<int, PsRecord>();
        if (pids.Length > 0)
        {
            try
            {
                var psCommand = ProbeCommand.BuildPsCommand(pids);
                if (psCommand is not null)
                {
                    var psResult = await session
                        .RunAsync(psCommand, cancellationToken, TimeSpan.FromSeconds(10))
                        .ConfigureAwait(false);
                    psSucceeded = psResult.ExitStatus is 0 or null;
                    if (psSucceeded)
                    {
                        psRecords = PsParser.Parse(psResult.StandardOutput.Split('\n'));
                    }
                }
            }
            catch (OperationCanceledException)
            {
                throw;
            }
            catch (Exception ex)
            {
                psSucceeded = false;
                Log.Error($"服务器 {_config.DisplayHost} 进程信息读取失败。", ex);
            }
        }

        return SnapshotBuilder.Build(
            _config.Id,
            _config.Username,
            probe,
            psRecords,
            psSucceeded,
            DateTimeOffset.UtcNow);
    }

    private async Task DelayWithWakeAsync(TimeSpan delay, CancellationToken cancellationToken)
    {
        TaskCompletionSource<bool> tcs;
        lock (_wakeGate)
        {
            if (_refreshPending)
            {
                _refreshPending = false;
                return;
            }

            tcs = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
            _wakeTcs = tcs;
        }

        using var registration = cancellationToken.Register(() => tcs.TrySetCanceled(cancellationToken));
        var delayTask = Task.Delay(delay, cancellationToken);
        await Task.WhenAny(delayTask, tcs.Task).ConfigureAwait(false);

        var woke = tcs.Task.IsCompletedSuccessfully;
        lock (_wakeGate)
        {
            if (ReferenceEquals(_wakeTcs, tcs))
            {
                _wakeTcs = null;
            }

            if (woke)
            {
                _refreshPending = false;
            }
        }

        cancellationToken.ThrowIfCancellationRequested();
    }

    private bool IsSessionConnected() => _session?.IsConnected == true;

    private void DisposeSession()
    {
        var session = _session;
        _session = null;
        session?.Dispose();
    }

    private void UpdateStatus(MonitorStatus status)
    {
        lock (_statusGate)
        {
            if (_status.Phase == status.Phase
                && _status.Error?.Kind == status.Error?.Kind
                && string.Equals(_status.Error?.Message, status.Error?.Message, StringComparison.Ordinal))
            {
                return;
            }

            _status = status;
        }

        StatusChanged?.Invoke(this, status);
    }

    private MonitorError MapError(Exception exception)
    {
        if (exception is MonitorOperationException operation)
        {
            return new MonitorError(operation.Kind, operation.Message, operation.Detail);
        }

        if (exception is SshAuthenticationException)
        {
            return new MonitorError(MonitorErrorKind.Authentication, "SSH 认证失败，请检查用户名、密码或私钥。", exception.Message);
        }

        if (exception is SshOperationTimeoutException or TimeoutException or TaskCanceledException)
        {
            return new MonitorError(MonitorErrorKind.Timeout, "采集命令超时，正在重试。", exception.Message);
        }

        if (exception is SocketException or SshConnectionException or IOException)
        {
            return new MonitorError(MonitorErrorKind.Network, "SSH 连接中断，正在重连。", exception.Message);
        }

        if (exception is SshException sshException)
        {
            var message = sshException.Message;
            if (message.Contains("auth", StringComparison.OrdinalIgnoreCase))
            {
                return new MonitorError(MonitorErrorKind.Authentication, "SSH 认证失败，请检查用户名、密码或私钥。", message);
            }

            if (message.Contains("host key", StringComparison.OrdinalIgnoreCase))
            {
                return new MonitorError(MonitorErrorKind.HostKey, "服务器主机密钥验证失败。", message);
            }
        }

        return new MonitorError(MonitorErrorKind.Unknown, "监控失败，正在重试。", exception.Message);
    }

    private static string? nullIfEmpty(string? value) => string.IsNullOrWhiteSpace(value) ? null : value.Trim();
}
