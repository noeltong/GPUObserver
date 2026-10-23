using System.Text;
using GPUObserver.Monitoring;
using GPUObserver.Monitoring.Parsing;

namespace GPUObserver.Tests;

public class ParsingTests
{
    [Fact]
    public void CsvLine_HandlesQuotesAndCommas()
    {
        var fields = CsvLine.Split("0, GPU-abc, \"NVIDIA RTX, Special\", 45, 1024, 24576, 62, 210.5, 350");

        Assert.Equal(9, fields.Count);
        Assert.Equal("NVIDIA RTX, Special", fields[2]);
    }

    [Fact]
    public void NvidiaGpuParser_ParsesValuesAndNA()
    {
        var lines = new[]
        {
            "0, GPU-1, NVIDIA GeForce RTX 4090, 55, 8192, 24564, 64, 120.5, [N/A]",
            "1, GPU-2, NVIDIA A100, [N/A], [N/A], 81920, [N/A], [N/A], 400"
        };

        var gpus = NvidiaGpuParser.Parse(lines);

        Assert.Equal(2, gpus.Count);
        var first = gpus[0];
        Assert.Equal(55, first.UtilizationPercent);
        Assert.Equal(8192, first.MemoryUsedMiB);
        Assert.Equal(24564, first.MemoryTotalMiB);
        Assert.Equal(120.5, first.PowerW);
        Assert.Null(first.PowerLimitW);
        Assert.Equal(33.35, first.MemoryPercent!.Value, 2);

        var second = gpus[1];
        Assert.Null(second.UtilizationPercent);
        Assert.Null(second.MemoryUsedMiB);
        Assert.Equal(400, second.PowerLimitW);
    }

    [Fact]
    public void ComputeAppsParser_HandlesCommaInNameAndMemory()
    {
        var rows = ComputeAppsParser.Parse(new[]
        {
            "GPU-1, 1234, python, 1024",
            "GPU-2, 5678, \"trainer, distributed\", 2048",
            "GPU-1, N/A, other, N/A"
        });

        Assert.Equal(2, rows.Count);
        Assert.Contains(rows, row => row.Pid == 1234 && row.MemoryMiB == 1024);
        var second = rows.Single(row => row.Pid == 5678);
        Assert.Equal("trainer, distributed", second.ProcessName);
        Assert.Equal(2048, second.MemoryMiB);
    }

    [Fact]
    public void PmonParser_IgnoresHeadersAndDeduplicates()
    {
        var rows = PmonParser.Parse(new[]
        {
            "# gpu pid type sm mem enc dec command",
            "# Idx # C/G % % % % name",
            "0 111 C 50 12 0 0 python",
            "0 111 C 50 12 0 0 python",
            "1 222 G 10 3 0 0 Xorg",
            "1 - - - - - - -"
        });

        Assert.Equal(2, rows.Count);
        Assert.Contains(rows, row => row is { GpuIndex: 0, Pid: 111 });
        Assert.Contains(rows, row => row is { GpuIndex: 1, Pid: 222 });
    }

    [Fact]
    public void PsParser_ParsesUserAndCommand()
    {
        var records = PsParser.Parse(new[]
        {
            " 111 alice python3",
            " 222 bob   /usr/bin/trainer --epochs 10",
            "bad line",
            " 333 carol"
        });

        Assert.Equal(3, records.Count);
        Assert.Equal("alice", records[111].User);
        Assert.Equal("/usr/bin/trainer --epochs 10", records[222].Command);
        Assert.Equal("carol", records[333].User);
        Assert.Equal("", records[333].Command);
    }

    [Fact]
    public void FreeParser_ParsesMemoryRow()
    {
        var ram = FreeParser.Parse(new[]
        {
            "               total        used        free      shared  buff/cache   available",
            "Mem:           64000       12000        2000        100       50000       49000",
            "Swap:          16384           0       16384"
        });

        Assert.NotNull(ram);
        Assert.Equal(64000, ram!.TotalMiB);
        Assert.Equal(12000, ram.UsedMiB);
        Assert.Equal(49000, ram.AvailableMiB);
        Assert.Equal(18.75, ram.Percent, 2);
    }

    [Fact]
    public void ProbeParser_SplitsSections()
    {
        const string output = """
            @@GPU@@
            0, GPU-1, RTX 4090, 50, 8000, 24564, 60, 200, 350
            @@APPS@@
            GPU-1, 1234, python, 8000
            @@PMON@@
            # gpu pid type sm mem enc dec command
            0 1234 C 10 30 0 0 python
            @@PMONRC@@0
            @@MEM@@
                           total        used        free      shared  buff/cache   available
            Mem:           64000       12000        2000         100       50000       49000
            Swap:          16384           0       16384
            @@END@@
            """;

        var result = ProbeParser.Parse(output);

        Assert.False(result.NvSmiMissing);
        Assert.True(result.PmonAvailable);
        Assert.Single(result.Gpus);
        Assert.Single(result.ComputeApps);
        Assert.Single(result.Pmon);
        Assert.NotNull(result.Ram);
        Assert.Single(result.CollectGpuPids());
    }

    [Fact]
    public void ProbeParser_MarksNvSmiMissing()
    {
        const string output = """
            @@GPU@@
            NVSMI_MISSING
            @@APPS@@
            @@PMON@@
            @@PMONRC@@127
            @@MEM@@
            MEM_MISSING
            @@END@@
            """;

        var result = ProbeParser.Parse(output);

        Assert.True(result.NvSmiMissing);
        Assert.False(result.PmonAvailable);
        Assert.Empty(result.Gpus);
        Assert.Null(result.Ram);
        Assert.NotEmpty(result.Warnings);
    }

    [Fact]
    public void SnapshotBuilder_FiltersToCurrentUserAndComputesCounts()
    {
        var uuid1 = "GPU-uuid-1";
        var uuid2 = "GPU-uuid-2";
        var main = new ProbeMainResult
        {
            PmonAvailable = true,
            Ram = new RamTelemetry(64000, 12000, 49000)
        };
        main.Gpus.AddRange(new[]
        {
            new GpuTelemetry(0, uuid1, "RTX 4090", 90, 8000, 24564, 60, 200, 350),
            new GpuTelemetry(1, uuid2, "RTX 4090", 10, 1000, 24564, 40, 80, 350)
        });
        main.Pmon.AddRange(new[] { new PmonRecord(0, 100), new PmonRecord(1, 200), new PmonRecord(0, 300) });
        main.ComputeApps.AddRange(new[]
        {
            new ComputeAppRecord(uuid1, 100, "python", 6000),
            new ComputeAppRecord(uuid2, 200, "trainer", 1000),
            new ComputeAppRecord(uuid1, 300, "other", 2000)
        });

        var ps = new Dictionary<int, PsRecord>
        {
            [100] = new(100, "alice", "python"),
            [200] = new(200, "bob", "trainer"),
            [300] = new(300, "alice", "jupyter")
        };

        var snapshot = SnapshotBuilder.Build(Guid.NewGuid(), "alice", main, ps, true, DateTimeOffset.UtcNow);

        Assert.True(snapshot.ProcessDataComplete);
        Assert.Equal(2, snapshot.Processes.Count);
        Assert.Equal(2, snapshot.Processes.Count(process => process.GpuIndex == 0));
        Assert.Equal(2, snapshot.MyProcessCounts[0]);
        Assert.Equal(0, snapshot.MyProcessCounts[1]);
        Assert.Contains(snapshot.Processes, process => process.Pid == 100 && process.MemoryMiB == 6000);
        Assert.DoesNotContain(snapshot.Processes, process => process.Pid == 200);
    }

    [Fact]
    public void SnapshotBuilder_InvalidPmonSuppressesComplete()
    {
        var main = new ProbeMainResult { PmonAvailable = false };
        main.Gpus.Add(new GpuTelemetry(0, "u", "GPU", 0, 0, 1, null, null, null));

        var snapshot = SnapshotBuilder.Build(
            Guid.NewGuid(),
            "alice",
            main,
            new Dictionary<int, PsRecord>(),
            true,
            DateTimeOffset.UtcNow);

        Assert.False(snapshot.ProcessDataComplete);
        Assert.NotNull(snapshot.Warning);
    }

    [Fact]
    public void MainCommand_RunsPosixScriptThroughShForFishLoginShell()
    {
        var command = ProbeCommand.BuildMainCommand();
        const string prefix = "printf '%s' '";

        Assert.StartsWith(prefix, command);
        Assert.EndsWith("' | base64 -d | /bin/sh", command);

        var start = prefix.Length;
        var end = command.IndexOf('\'', start);
        var encoded = command[start..end];

        Assert.Matches("^[A-Za-z0-9+/=]+$", encoded);
        Assert.Equal(ProbeCommand.MainScript, Encoding.UTF8.GetString(Convert.FromBase64String(encoded)));
    }

    [Fact]
    public void PsCommand_OnlyContainsNumericPids()
    {
        var command = ProbeCommand.BuildPsCommand(new[] { 10, 20, 20, -5, 0 });

        Assert.NotNull(command);
        Assert.StartsWith("env LC_ALL=C ps", command);
        Assert.Contains("-p 10,20", command);
        Assert.Null(ProbeCommand.BuildPsCommand(Array.Empty<int>()));
    }
}
