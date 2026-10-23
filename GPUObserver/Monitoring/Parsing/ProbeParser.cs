namespace GPUObserver.Monitoring.Parsing;

public static class ProbeParser
{
    private static readonly string[] Markers = ["@@GPU@@", "@@APPS@@", "@@PMON@@", "@@MEM@@", "@@END@@"];

    public static ProbeMainResult Parse(string stdout)
    {
        var sections = new Dictionary<string, List<string>>(StringComparer.Ordinal);
        string? pmonRcText = null;

        string? current = null;
        foreach (var rawLine in (stdout ?? "").Replace("\r\n", "\n").Split('\n'))
        {
            var line = rawLine.TrimEnd('\r');
            if (line.StartsWith("@@PMONRC@@", StringComparison.Ordinal))
            {
                pmonRcText = line["@@PMONRC@@".Length..].Trim();
                current = null;
                continue;
            }

            if (Markers.Contains(line, StringComparer.Ordinal))
            {
                current = line;
                if (!sections.ContainsKey(current))
                {
                    sections[current] = [];
                }
                continue;
            }

            if (current is not null)
            {
                sections[current].Add(line);
            }
        }

        var result = new ProbeMainResult();
        var gpuLines = sections.GetValueOrDefault("@@GPU@@") ?? [];
        if (gpuLines.Any(line => line.Contains("NVSMI_MISSING", StringComparison.Ordinal)))
        {
            result.NvSmiMissing = true;
            result.Warnings.Add("未找到 nvidia-smi，无法读取 GPU 信息。");
        }

        result.Gpus.AddRange(NvidiaGpuParser.Parse(gpuLines));
        result.ComputeApps.AddRange(ComputeAppsParser.Parse(sections.GetValueOrDefault("@@APPS@@") ?? []));
        result.Pmon.AddRange(PmonParser.Parse(sections.GetValueOrDefault("@@PMON@@") ?? []));

        var pmonRc = ParseUtil.Int(pmonRcText);
        result.PmonAvailable = pmonRc == 0;
        if (!result.PmonAvailable && !result.NvSmiMissing)
        {
            result.Warnings.Add("无法获取完整的 GPU 进程列表（pmon 不可用），空闲通知已暂停。");
        }

        result.Ram = FreeParser.Parse(sections.GetValueOrDefault("@@MEM@@") ?? []);
        if (result.Gpus.Count == 0 && !result.NvSmiMissing)
        {
            result.Warnings.Add("未读取到 GPU 信息。");
        }

        return result;
    }
}

public static class SnapshotBuilder
{
    public static ServerSnapshot Build(
        Guid serverId,
        string username,
        ProbeMainResult main,
        IReadOnlyDictionary<int, PsRecord> psRecords,
        bool psSucceeded,
        DateTimeOffset capturedAt)
    {
        var warnings = new List<string>(main.Warnings);
        if (!main.PmonAvailable)
        {
            warnings.Add("无法获取完整的 GPU 进程列表（pmon 不可用），空闲通知已暂停。");
        }
        if (!psSucceeded)
        {
            warnings.Add("无法解析 GPU 进程所属用户，空闲通知已暂停。");
        }

        var processDataComplete = main.PmonAvailable && psSucceeded;
        var gpuIndexByUuid = new Dictionary<string, int>(StringComparer.OrdinalIgnoreCase);
        foreach (var gpu in main.Gpus)
        {
            if (!string.IsNullOrWhiteSpace(gpu.Uuid))
            {
                gpuIndexByUuid[gpu.Uuid] = gpu.Index;
            }
        }

        var keys = new HashSet<(int Gpu, int Pid)>();
        var computeMemory = new Dictionary<(int Gpu, int Pid), long?>();
        var computeNames = new Dictionary<(int Gpu, int Pid), string>();

        foreach (var record in main.Pmon)
        {
            keys.Add((record.GpuIndex, record.Pid));
        }

        foreach (var app in main.ComputeApps)
        {
            if (!gpuIndexByUuid.TryGetValue(app.GpuUuid, out var gpuIndex))
            {
                continue;
            }

            var key = (gpuIndex, app.Pid);
            keys.Add(key);
            computeMemory[key] = app.MemoryMiB;
            computeNames[key] = app.ProcessName;
        }

        var counts = main.Gpus.ToDictionary(gpu => gpu.Index, _ => 0);
        var processes = new List<GpuProcessTelemetry>();

        if (psSucceeded)
        {
            foreach (var key in keys.OrderBy(item => item.Gpu).ThenBy(item => item.Pid))
            {
                if (!psRecords.TryGetValue(key.Pid, out var ps))
                {
                    // The pid most likely exited between pmon and ps. The next
                    // sample will decide; do not turn this into a fake transition.
                    continue;
                }

                if (!string.Equals(ps.User, username, StringComparison.Ordinal))
                {
                    continue;
                }

                var command = string.IsNullOrWhiteSpace(ps.Command)
                    ? computeNames.GetValueOrDefault(key, $"pid {key.Pid}")
                    : ps.Command;
                var memory = computeMemory.GetValueOrDefault(key);

                processes.Add(new GpuProcessTelemetry(key.Gpu, key.Pid, ps.User, command, memory));
                if (counts.ContainsKey(key.Gpu))
                {
                    counts[key.Gpu]++;
                }
            }
        }

        return new ServerSnapshot(
            serverId,
            capturedAt,
            main.Gpus,
            main.Ram,
            processes,
            processDataComplete,
            counts,
            warnings.Count == 0 ? null : string.Join(" ", warnings.Distinct()));
    }
}
