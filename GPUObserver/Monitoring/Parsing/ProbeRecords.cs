namespace GPUObserver.Monitoring.Parsing;

public sealed record ComputeAppRecord(string GpuUuid, int Pid, string ProcessName, long? MemoryMiB);

public sealed record PmonRecord(int GpuIndex, int Pid);

public sealed record PsRecord(int Pid, string User, string Command);

public sealed class ProbeMainResult
{
    public List<GpuTelemetry> Gpus { get; } = [];
    public List<ComputeAppRecord> ComputeApps { get; } = [];
    public List<PmonRecord> Pmon { get; } = [];
    public List<string> Warnings { get; } = [];
    public bool PmonAvailable { get; set; }
    public bool NvSmiMissing { get; set; }
    public RamTelemetry? Ram { get; set; }

    public IEnumerable<int> CollectGpuPids()
    {
        return Pmon.Select(item => item.Pid)
            .Concat(ComputeApps.Select(item => item.Pid))
            .Where(pid => pid > 0)
            .Distinct();
    }
}

public static class NvidiaGpuParser
{
    public static List<GpuTelemetry> Parse(IEnumerable<string> lines)
    {
        var result = new Dictionary<int, GpuTelemetry>();

        foreach (var rawLine in lines)
        {
            var line = rawLine.Trim();
            if (line.Length == 0 || line.Contains("NVSMI_MISSING", StringComparison.Ordinal))
            {
                continue;
            }

            var fields = CsvLine.Split(line);
            if (fields.Count < 5)
            {
                continue;
            }

            var index = ParseUtil.Int(fields[0]);
            if (index is null)
            {
                continue;
            }

            var uuid = fields.Count > 1 ? fields[1] : "";
            var name = fields.Count > 2 ? fields[2] : $"GPU {index}";
            var utilization = ParseUtil.Double(fields.Count > 3 ? fields[3] : null);
            var memoryUsed = ParseUtil.Long(fields.Count > 4 ? fields[4] : null);
            var memoryTotal = ParseUtil.Long(fields.Count > 5 ? fields[5] : null);
            var temperature = ParseUtil.Double(fields.Count > 6 ? fields[6] : null);
            var power = ParseUtil.Double(fields.Count > 7 ? fields[7] : null);
            var powerLimit = ParseUtil.Double(fields.Count > 8 ? fields[8] : null);

            result[index.Value] = new GpuTelemetry(
                index.Value,
                uuid,
                name,
                utilization,
                memoryUsed,
                memoryTotal,
                temperature,
                power,
                powerLimit);
        }

        return result.Values.OrderBy(gpu => gpu.Index).ToList();
    }
}

public static class ComputeAppsParser
{
    public static List<ComputeAppRecord> Parse(IEnumerable<string> lines)
    {
        var result = new Dictionary<(string Uuid, int Pid), ComputeAppRecord>();

        foreach (var rawLine in lines)
        {
            var line = rawLine.Trim();
            if (line.Length == 0)
            {
                continue;
            }

            var fields = CsvLine.Split(line);
            if (fields.Count < 2)
            {
                continue;
            }

            var pid = ParseUtil.Int(fields[1]);
            var uuid = fields[0];
            if (pid is null || pid <= 0 || string.IsNullOrWhiteSpace(uuid))
            {
                continue;
            }

            var hasMemory = fields.Count >= 4;
            var memory = hasMemory ? ParseUtil.Long(fields[^1]) : null;
            var nameFieldCount = fields.Count > 2 ? fields.Count - (hasMemory ? 3 : 2) : 0;
            var name = nameFieldCount > 0
                ? string.Join(",", fields.Skip(2).Take(nameFieldCount)).Trim()
                : "";

            result[(uuid, pid.Value)] = new ComputeAppRecord(uuid, pid.Value, name, memory);
        }

        return result.Values.ToList();
    }
}

public static class PmonParser
{
    public static List<PmonRecord> Parse(IEnumerable<string> lines)
    {
        var result = new HashSet<(int Gpu, int Pid)>();

        foreach (var rawLine in lines)
        {
            var line = rawLine.Trim();
            if (line.Length == 0 || line.StartsWith('#'))
            {
                continue;
            }

            var fields = line.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
            if (fields.Length < 2)
            {
                continue;
            }

            var gpu = ParseUtil.Int(fields[0]);
            var pid = ParseUtil.Int(fields[1]);
            if (gpu is null || gpu < 0 || pid is null || pid <= 0)
            {
                continue;
            }

            result.Add((gpu.Value, pid.Value));
        }

        return result.OrderBy(item => item.Gpu).ThenBy(item => item.Pid)
            .Select(item => new PmonRecord(item.Gpu, item.Pid))
            .ToList();
    }
}

public static class PsParser
{
    public static Dictionary<int, PsRecord> Parse(IEnumerable<string> lines)
    {
        var result = new Dictionary<int, PsRecord>();

        foreach (var rawLine in lines)
        {
            var line = rawLine.Trim();
            if (line.Length == 0)
            {
                continue;
            }

            var parts = line.Split((char[]?)null, 3, StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length < 2)
            {
                continue;
            }

            var pid = ParseUtil.Int(parts[0]);
            if (pid is null || pid <= 0)
            {
                continue;
            }

            var command = parts.Length > 2 ? parts[2].Trim() : "";
            result[pid.Value] = new PsRecord(pid.Value, parts[1].Trim(), command);
        }

        return result;
    }
}

public static class FreeParser
{
    public static RamTelemetry? Parse(IEnumerable<string> lines)
    {
        foreach (var rawLine in lines)
        {
            var line = rawLine.Trim();
            if (!line.StartsWith("Mem:", StringComparison.Ordinal))
            {
                continue;
            }

            var parts = line.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length < 3)
            {
                continue;
            }

            var total = ParseUtil.Long(parts.ElementAtOrDefault(1));
            var used = ParseUtil.Long(parts.ElementAtOrDefault(2));
            var available = ParseUtil.Long(parts.ElementAtOrDefault(6)) ?? (total.HasValue && used.HasValue ? total - used : null);

            if (total is > 0 && used is >= 0 && available is >= 0)
            {
                return new RamTelemetry(total.Value, Math.Min(used.Value, total.Value), available.Value);
            }
        }

        return null;
    }
}
