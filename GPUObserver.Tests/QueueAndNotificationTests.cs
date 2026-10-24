using GPUObserver.Config;
using GPUObserver.Monitoring;

namespace GPUObserver.Tests;

public class QueueAndNotificationTests
{
    [Fact]
    public void SampleQueue_DropsSamplesOlderThan300Seconds()
    {
        var queue = new SampleQueue();
        var start = DateTimeOffset.UtcNow;

        queue.Enqueue(Snapshot(start, [1]));
        queue.Enqueue(Snapshot(start.AddSeconds(100), [1]));
        queue.Enqueue(Snapshot(start.AddSeconds(250), [1]));

        var later = start.AddSeconds(301);
        var visible = queue.GetSnapshots(later);

        Assert.Equal(2, visible.Count);
        Assert.All(visible, sample => Assert.True(later - sample.CapturedAt <= SampleQueue.Retention));
    }

    [Fact]
    public void SampleQueue_DoesNotRetainAnythingBeyond300SecondsInMemory()
    {
        var queue = new SampleQueue();
        var now = DateTimeOffset.UtcNow;
        for (var i = 0; i < 30; i++)
        {
            queue.Enqueue(Snapshot(now.AddSeconds(-i * 5), [1]));
        }

        Assert.Equal(30, queue.Count);
        queue.Prune(now.AddSeconds(600));

        Assert.Equal(0, queue.Count);
        Assert.Empty(queue.GetSnapshots(now.AddSeconds(600)));
    }

    [Fact]
    public void SampleQueue_EnforcesHardCap()
    {
        var queue = new SampleQueue();
        var now = DateTimeOffset.UtcNow;
        for (var i = 0; i < SampleQueue.MaxSamples + 20; i++)
        {
            queue.Enqueue(Snapshot(now.AddMilliseconds(i), [1]));
        }

        Assert.Equal(SampleQueue.MaxSamples, queue.Count);
    }

    [Fact]
    public void NotificationStateMachine_SeedsThenNotifiesOnTransition()
    {
        var machine = new NotificationStateMachine();
        var now = DateTimeOffset.UtcNow;

        Assert.Empty(machine.Evaluate(Snapshot(now, counts: new Dictionary<int, int> { [0] = 1 }), true));
        Assert.Equal(new[] { 0 }, machine.Evaluate(Snapshot(now.AddSeconds(5), counts: new Dictionary<int, int> { [0] = 0 }), true));
        Assert.Empty(machine.Evaluate(Snapshot(now.AddSeconds(10), counts: new Dictionary<int, int> { [0] = 0 }), true));
    }

    [Fact]
    public void NotificationStateMachine_CoalescesMultipleGpus()
    {
        var machine = new NotificationStateMachine();
        var now = DateTimeOffset.UtcNow;

        machine.Evaluate(Snapshot(now, [0, 1], new Dictionary<int, int> { [0] = 1, [1] = 1 }), true);
        var idle = machine.Evaluate(
            Snapshot(now.AddSeconds(5), [0, 1], new Dictionary<int, int> { [0] = 0, [1] = 0 }),
            true);

        Assert.Equal(new[] { 0, 1 }, idle);
    }

    [Fact]
    public void NotificationStateMachine_InvalidDataSuppressesAndReseeds()
    {
        var machine = new NotificationStateMachine();
        var now = DateTimeOffset.UtcNow;

        machine.Evaluate(Snapshot(now, counts: new Dictionary<int, int> { [0] = 1 }), true);
        Assert.Empty(machine.Evaluate(Snapshot(now.AddSeconds(5), complete: false), true));
        Assert.Empty(machine.Evaluate(Snapshot(now.AddSeconds(10), counts: new Dictionary<int, int> { [0] = 0 }), true));

        machine.Evaluate(Snapshot(now.AddSeconds(15), counts: new Dictionary<int, int> { [0] = 1 }), true);
        Assert.Equal(new[] { 0 }, machine.Evaluate(Snapshot(now.AddSeconds(20), counts: new Dictionary<int, int> { [0] = 0 }), true));
    }

    [Fact]
    public void NotificationStateMachine_DisabledUpdatesStateWithoutNotifying()
    {
        var machine = new NotificationStateMachine();
        var now = DateTimeOffset.UtcNow;

        machine.Evaluate(Snapshot(now, counts: new Dictionary<int, int> { [0] = 1 }), true);
        Assert.Empty(machine.Evaluate(Snapshot(now.AddSeconds(5), counts: new Dictionary<int, int> { [0] = 0 }), false));
        Assert.Empty(machine.Evaluate(Snapshot(now.AddSeconds(10), counts: new Dictionary<int, int> { [0] = 0 }), true));
    }

    [Fact]
    public void NotificationStateMachine_GapOver300SecondsReseeds()
    {
        var machine = new NotificationStateMachine();
        var now = DateTimeOffset.UtcNow;

        machine.Evaluate(Snapshot(now, counts: new Dictionary<int, int> { [0] = 1 }), true);
        var idleAfterGap = machine.Evaluate(
            Snapshot(now.AddSeconds(301), counts: new Dictionary<int, int> { [0] = 0 }),
            true);

        Assert.Empty(idleAfterGap);
    }

    [Fact]
    public void NotificationStateMachine_NewGpuDoesNotNotify()
    {
        var machine = new NotificationStateMachine();
        var now = DateTimeOffset.UtcNow;

        machine.Evaluate(Snapshot(now, [0], new Dictionary<int, int> { [0] = 1 }), true);
        var transitions = machine.Evaluate(
            Snapshot(now.AddSeconds(5), [0, 1], new Dictionary<int, int> { [0] = 1, [1] = 0 }),
            true);

        Assert.Empty(transitions);
    }

    private static ServerSnapshot Snapshot(
        DateTimeOffset timestamp,
        int[]? gpuIndices = null,
        Dictionary<int, int>? counts = null,
        bool complete = true)
    {
        gpuIndices ??= [0];
        counts ??= gpuIndices.ToDictionary(index => index, _ => 0);

        var gpus = gpuIndices
            .Select(index => new GpuTelemetry(index, $"uuid-{index}", $"GPU {index}", 10, 100, 1000, 40, 50, 100))
            .ToList();
        var processes = counts
            .Where(pair => pair.Value > 0)
            .SelectMany(pair => Enumerable.Range(0, pair.Value)
                .Select(i => new GpuProcessTelemetry(pair.Key, 1000 + i, "alice", "python", 100)))
            .ToList();

        return new ServerSnapshot(
            Guid.NewGuid(),
            timestamp,
            gpus,
            new RamTelemetry(1000, 200, 800),
            processes,
            complete,
            counts,
            null);
    }
}

public class ConfigTests
{
    [Fact]
    public void SecretProtector_RoundTripsAndRejectsEmpty()
    {
        const string secret = "correct horse battery staple 密码";
        var protectedValue = SecretProtector.Protect(secret);

        Assert.NotNull(protectedValue);
        Assert.NotEqual(secret, protectedValue);
        Assert.Equal(secret, SecretProtector.Unprotect(protectedValue));
        Assert.Null(SecretProtector.Protect(""));
    }

    [Fact]
    public void SecretProtector_TamperedValueFailsGracefully()
    {
        var protectedValue = SecretProtector.Protect("secret")!;
        var tampered = protectedValue[..^2] + (protectedValue[^2] == 'A' ? "BB" : "AA");

        Assert.False(SecretProtector.TryUnprotect(tampered, out _));
    }

    [Fact]
    public void ServerConfig_DisplayLabelPrefersNameAndFallsBackToHost()
    {
        var server = new ServerConfig { Host = "10.0.0.5", Username = "alice" };

        Assert.Equal("10.0.0.5", server.DisplayLabel);
        server.Name = "训练机";
        Assert.Equal("训练机", server.DisplayLabel);
        Assert.Equal("训练机", server.Clone().Name);
        server.Name = "   ";
        Assert.Equal("10.0.0.5", server.DisplayLabel);
        server.Port = 2222;
        Assert.Equal("10.0.0.5:2222", server.DisplayLabel);
    }
}
