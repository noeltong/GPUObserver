namespace GpuObserve.Monitoring;

public sealed class NotificationStateMachine
{
    private readonly Dictionary<int, bool> _hadMine = new();
    private DateTimeOffset? _lastValidSampleAt;
    private bool _seeded;
    private bool _invalidSinceLastValid;

    public IReadOnlyList<int> Evaluate(ServerSnapshot snapshot, bool notifyEnabled)
    {
        if (!snapshot.ProcessDataComplete)
        {
            _invalidSinceLastValid = true;
            return [];
        }

        var mustSeed = !_seeded
            || _invalidSinceLastValid
            || _lastValidSampleAt is null
            || snapshot.CapturedAt - _lastValidSampleAt > SampleQueue.Retention;

        var becameIdle = new List<int>();
        var seenGpus = new HashSet<int>();

        foreach (var gpu in snapshot.Gpus)
        {
            seenGpus.Add(gpu.Index);
            var hasMine = snapshot.MyProcessCounts.TryGetValue(gpu.Index, out var count) && count > 0;

            if (mustSeed || !_hadMine.TryGetValue(gpu.Index, out var previous))
            {
                _hadMine[gpu.Index] = hasMine;
                continue;
            }

            if (previous && !hasMine)
            {
                becameIdle.Add(gpu.Index);
            }

            _hadMine[gpu.Index] = hasMine;
        }

        foreach (var removed in _hadMine.Keys.Where(index => !seenGpus.Contains(index)).ToList())
        {
            _hadMine.Remove(removed);
        }

        _lastValidSampleAt = snapshot.CapturedAt;
        _invalidSinceLastValid = false;
        _seeded = true;

        if (!notifyEnabled || becameIdle.Count == 0)
        {
            return [];
        }

        becameIdle.Sort();
        return becameIdle;
    }

    public void Reset()
    {
        _hadMine.Clear();
        _lastValidSampleAt = null;
        _seeded = false;
        _invalidSinceLastValid = false;
    }
}
