using GpuObserve.Monitoring;

namespace GpuObserve.Ssh;

public sealed class MonitorOperationException : Exception
{
    public MonitorOperationException(MonitorErrorKind kind, string message, string? detail = null, Exception? inner = null)
        : base(message, inner)
    {
        Kind = kind;
        Detail = detail;
    }

    public MonitorErrorKind Kind { get; }

    public string? Detail { get; }
}

public sealed record CommandResult(string StandardOutput, string StandardError, int? ExitStatus);
