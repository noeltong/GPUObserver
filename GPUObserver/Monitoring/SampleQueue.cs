namespace GPUObserver.Monitoring;

/// <summary>
/// In-memory FIFO of server samples. This is the only place samples are kept:
/// nothing is written to disk, and anything older than five minutes is removed
/// on enqueue, on every consumer read, and by the monitor service timer.
/// </summary>
public sealed class SampleQueue
{
    public static readonly TimeSpan Retention = TimeSpan.FromSeconds(300);
    public const int MaxSamples = 512;

    private readonly object _gate = new();
    private readonly Queue<ServerSnapshot> _samples = new();

    public int Count
    {
        get
        {
            lock (_gate)
            {
                return _samples.Count;
            }
        }
    }

    public void Enqueue(ServerSnapshot sample)
    {
        lock (_gate)
        {
            _samples.Enqueue(sample);
            PruneCore(DateTimeOffset.UtcNow);
        }
    }

    public void Prune() => Prune(DateTimeOffset.UtcNow);

    public void Prune(DateTimeOffset now)
    {
        lock (_gate)
        {
            PruneCore(now);
        }
    }

    public IReadOnlyList<ServerSnapshot> GetSnapshots(DateTimeOffset? now = null)
    {
        lock (_gate)
        {
            PruneCore(now ?? DateTimeOffset.UtcNow);
            return _samples.ToArray();
        }
    }

    public ServerSnapshot? GetLatest(DateTimeOffset? now = null)
    {
        lock (_gate)
        {
            PruneCore(now ?? DateTimeOffset.UtcNow);
            return _samples.Count > 0 ? _samples.Last() : null;
        }
    }

    public void Clear()
    {
        lock (_gate)
        {
            _samples.Clear();
        }
    }

    private void PruneCore(DateTimeOffset now)
    {
        var cutoff = now - Retention;
        while (_samples.Count > 0 && _samples.Peek().CapturedAt < cutoff)
        {
            _samples.Dequeue();
        }

        while (_samples.Count > MaxSamples)
        {
            _samples.Dequeue();
        }
    }
}
