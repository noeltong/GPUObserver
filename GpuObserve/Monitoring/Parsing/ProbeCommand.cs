using System.Globalization;

namespace GpuObserve.Monitoring.Parsing;

public static class ProbeCommand
{
    public const string MainScript = """
        export LC_ALL=C
        SMI=""
        if command -v nvidia-smi >/dev/null 2>&1; then
          SMI=$(command -v nvidia-smi)
        else
          for candidate in /usr/bin/nvidia-smi /usr/local/bin/nvidia-smi /usr/local/nvidia/bin/nvidia-smi /opt/nvidia/bin/nvidia-smi /usr/lib/wsl/lib/nvidia-smi; do
            if [ -x "$candidate" ]; then SMI="$candidate"; break; fi
          done
        fi
        printf '@@GPU@@\n'
        if [ -n "$SMI" ]; then
          "$SMI" --query-gpu=index,uuid,name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw,power.limit --format=csv,noheader,nounits
          printf '@@APPS@@\n'
          "$SMI" --query-compute-apps=gpu_uuid,pid,process_name,used_gpu_memory --format=csv,noheader,nounits
          printf '@@PMON@@\n'
          PMON_OUT=$("$SMI" pmon -c 1 2>/dev/null)
          PMON_RC=$?
          printf '%s\n' "$PMON_OUT"
          printf '@@PMONRC@@%s\n' "$PMON_RC"
        else
          printf 'NVSMI_MISSING\n'
          printf '@@APPS@@\n'
          printf '@@PMON@@\n'
          printf '@@PMONRC@@127\n'
        fi
        printf '@@MEM@@\n'
        if command -v free >/dev/null 2>&1; then free -m; else printf 'MEM_MISSING\n'; fi
        printf '@@END@@\n'
        """;

    public static string? BuildPsCommand(IEnumerable<int> pids)
    {
        var normalized = pids
            .Where(pid => pid > 0)
            .Distinct()
            .ToArray();
        if (normalized.Length == 0)
        {
            return null;
        }

        var list = string.Join(",", normalized.Select(pid => pid.ToString(CultureInfo.InvariantCulture)));
        return $"LC_ALL=C ps -o pid=,user=,comm= -p {list}";
    }
}
