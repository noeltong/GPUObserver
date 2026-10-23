using GPUObserver.Config;
using GPUObserver.Util;

namespace GPUObserver.Monitoring;

public sealed record HostKeyLearnedEventArgs(Guid ServerId, string Fingerprint);

public sealed class MonitorService : IDisposable
{
    private sealed class Runtime
    {
        public required ServerConfig Config { get; init; }
        public required ServerMonitor Monitor { get; init; }
        public NotificationStateMachine Notifications { get; } = new();
    }

    private readonly object _gate = new();
    private readonly Dictionary<Guid, Runtime> _runtimes = [];
    private AppConfig _config = new();
    private System.Threading.Timer? _pruneTimer;
    private bool _disposed;

    public event EventHandler<Guid>? Changed;

    public event EventHandler<IdleNotificationArgs>? IdleDetected;

    public event EventHandler<HostKeyLearnedEventArgs>? HostKeyLearned;

    public void ApplyConfig(AppConfig config)
    {
        ObjectDisposedException.ThrowIf(_disposed, this);

        var newConfig = config.Clone();
        var oldRuntimes = new List<Runtime>();
        var newRuntimes = new List<Runtime>();

        lock (_gate)
        {
            oldRuntimes.AddRange(_runtimes.Values);
            _runtimes.Clear();
            _config = newConfig;

            foreach (var server in newConfig.Servers)
            {
                var runtimeConfig = server.Clone();
                var runtime = new Runtime
                {
                    Config = runtimeConfig,
                    Monitor = new ServerMonitor(
                        runtimeConfig,
                        newConfig.RefreshSeconds,
                        HandleHostKeyLearned)
                };

                runtime.Monitor.SnapshotReceived += (_, snapshot) => HandleSnapshot(runtime, snapshot);
                runtime.Monitor.StatusChanged += (_, _) => HandleStatusChanged(runtime);
                _runtimes[runtime.Config.Id] = runtime;
                newRuntimes.Add(runtime);
            }

            _pruneTimer ??= new System.Threading.Timer(
                _ => PruneAll(),
                null,
                TimeSpan.FromSeconds(1),
                TimeSpan.FromSeconds(1));
        }

        foreach (var old in oldRuntimes)
        {
            old.Monitor.Dispose();
        }

        foreach (var runtime in newRuntimes)
        {
            runtime.Monitor.Start();
        }

        Changed?.Invoke(this, Guid.Empty);
    }

    public IReadOnlyList<ServerViewState> GetStates()
    {
        List<(ServerConfig Server, Runtime? Runtime)> entries;
        lock (_gate)
        {
            entries = _config.Servers
                .Select(server => (server, (Runtime?)(_runtimes.TryGetValue(server.Id, out var runtime) ? runtime : null)))
                .ToList();
        }

        var states = new List<ServerViewState>(entries.Count);
        foreach (var (server, runtime) in entries)
        {
            var status = runtime?.Monitor.Status ?? MonitorStatus.Stopped(server.Id);
            var history = runtime?.Monitor.GetHistory() ?? [];
            states.Add(new ServerViewState(server.Id, status.Phase, status.Error, history));
        }

        return states;
    }

    public ServerConfig? GetServer(Guid serverId)
    {
        lock (_gate)
        {
            return _config.Servers.FirstOrDefault(server => server.Id == serverId);
        }
    }

    public AppConfig GetConfigSnapshot()
    {
        lock (_gate)
        {
            return _config.Clone();
        }
    }

    public void RefreshAll()
    {
        List<ServerMonitor> monitors;
        lock (_gate)
        {
            monitors = _runtimes.Values.Select(runtime => runtime.Monitor).ToList();
        }

        foreach (var monitor in monitors)
        {
            monitor.RequestRefresh();
        }
    }

    public void Refresh(Guid serverId)
    {
        ServerMonitor? monitor = null;
        lock (_gate)
        {
            if (_runtimes.TryGetValue(serverId, out var runtime))
            {
                monitor = runtime.Monitor;
            }
        }

        monitor?.RequestRefresh();
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }

        _disposed = true;
        List<ServerMonitor> monitors;
        lock (_gate)
        {
            _pruneTimer?.Dispose();
            _pruneTimer = null;
            monitors = _runtimes.Values.Select(runtime => runtime.Monitor).ToList();
            _runtimes.Clear();
        }

        foreach (var monitor in monitors)
        {
            monitor.Dispose();
        }
    }

    private void HandleSnapshot(Runtime runtime, ServerSnapshot snapshot)
    {
        bool notifyEnabled;
        string label;

        lock (_gate)
        {
            if (!_runtimes.TryGetValue(runtime.Config.Id, out var current) || !ReferenceEquals(current, runtime))
            {
                return;
            }

            var server = _config.Servers.FirstOrDefault(item => item.Id == runtime.Config.Id);
            if (server is null)
            {
                return;
            }

            notifyEnabled = _config.NotificationsEnabled && server.NotificationsEnabled;
            label = server.DisplayHost;
        }

        var idleGpus = runtime.Notifications.Evaluate(snapshot, notifyEnabled);
        Changed?.Invoke(this, runtime.Config.Id);

        if (idleGpus.Count > 0)
        {
            IdleDetected?.Invoke(this, new IdleNotificationArgs(runtime.Config.Id, label, idleGpus, snapshot));
        }
    }

    private void HandleStatusChanged(Runtime runtime)
    {
        lock (_gate)
        {
            if (!_runtimes.TryGetValue(runtime.Config.Id, out var current) || !ReferenceEquals(current, runtime))
            {
                return;
            }
        }

        Changed?.Invoke(this, runtime.Config.Id);
    }

    private void HandleHostKeyLearned(Guid serverId, string fingerprint)
    {
        lock (_gate)
        {
            var server = _config.Servers.FirstOrDefault(item => item.Id == serverId);
            if (server is not null)
            {
                server.HostKeyFingerprintSha256 = fingerprint;
            }
        }

        HostKeyLearned?.Invoke(this, new HostKeyLearnedEventArgs(serverId, fingerprint));
    }

    private void PruneAll()
    {
        if (_disposed)
        {
            return;
        }

        List<ServerMonitor> monitors;
        lock (_gate)
        {
            monitors = _runtimes.Values.Select(runtime => runtime.Monitor).ToList();
        }

        foreach (var monitor in monitors)
        {
            monitor.PruneHistory();
        }
    }
}
