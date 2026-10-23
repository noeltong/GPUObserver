namespace GpuObserve.Monitoring;

public sealed record GpuTelemetry(
    int Index,
    string Uuid,
    string Name,
    double? UtilizationPercent,
    long? MemoryUsedMiB,
    long? MemoryTotalMiB,
    double? TemperatureC,
    double? PowerW,
    double? PowerLimitW)
{
    public double? MemoryPercent => MemoryUsedMiB.HasValue && MemoryTotalMiB is > 0
        ? Math.Clamp(MemoryUsedMiB.Value * 100.0 / MemoryTotalMiB.Value, 0, 100)
        : null;
}

public sealed record RamTelemetry(long TotalMiB, long UsedMiB, long AvailableMiB)
{
    public double Percent => TotalMiB > 0
        ? Math.Clamp(UsedMiB * 100.0 / TotalMiB, 0, 100)
        : 0;
}

public sealed record GpuProcessTelemetry(
    int GpuIndex,
    int Pid,
    string User,
    string Command,
    long? MemoryMiB);

public sealed record ServerSnapshot(
    Guid ServerId,
    DateTimeOffset CapturedAt,
    IReadOnlyList<GpuTelemetry> Gpus,
    RamTelemetry? Ram,
    IReadOnlyList<GpuProcessTelemetry> Processes,
    bool ProcessDataComplete,
    IReadOnlyDictionary<int, int> MyProcessCounts,
    string? Warning)
{
    public int MyProcessCount => Processes.Count;
}

public enum MonitorPhase
{
    Stopped,
    Connecting,
    Online,
    Error
}

public enum MonitorErrorKind
{
    None,
    Network,
    Authentication,
    HostKey,
    Credential,
    PrivateKey,
    Timeout,
    NvidiaSmiMissing,
    Command,
    Unknown
}

public sealed record MonitorError(MonitorErrorKind Kind, string Message, string? Detail = null);

public sealed record MonitorStatus(Guid ServerId, MonitorPhase Phase, MonitorError? Error, DateTimeOffset ChangedAt)
{
    public static MonitorStatus Stopped(Guid serverId) => new(serverId, MonitorPhase.Stopped, null, DateTimeOffset.UtcNow);

    public static MonitorStatus Connecting(Guid serverId) => new(serverId, MonitorPhase.Connecting, null, DateTimeOffset.UtcNow);

    public static MonitorStatus Online(Guid serverId) => new(serverId, MonitorPhase.Online, null, DateTimeOffset.UtcNow);

    public static MonitorStatus Failed(Guid serverId, MonitorError error) => new(serverId, MonitorPhase.Error, error, DateTimeOffset.UtcNow);
}

public sealed record ServerViewState(
    Guid ServerId,
    MonitorPhase Phase,
    MonitorError? Error,
    IReadOnlyList<ServerSnapshot> History)
{
    public ServerSnapshot? Latest => History.Count > 0 ? History[^1] : null;
}

public sealed record IdleNotificationArgs(
    Guid ServerId,
    string ServerLabel,
    IReadOnlyList<int> GpuIndices,
    ServerSnapshot Snapshot);
