using GpuObserve.Monitoring;
using GpuObserve.UI;
using GpuObserve.Util;
using Microsoft.Toolkit.Uwp.Notifications;
using Windows.UI.Notifications;

namespace GpuObserve.Notifications;

internal sealed class ToastService
{
    private readonly NotifyIcon _notifyIcon;
    private Guid? _lastBalloonServerId;

    public ToastService(NotifyIcon notifyIcon)
    {
        _notifyIcon = notifyIcon;
    }

    public void ShowIdle(IdleNotificationArgs args)
    {
        var gpuText = string.Join("、", args.GpuIndices.Select(index => $"GPU {index}"));
        var gpu = args.Snapshot.Gpus.FirstOrDefault(item => args.GpuIndices.Contains(item.Index));
        var metrics = gpu is null
            ? ""
            : $"当前利用率 {Drawing.FormatPercent(gpu.UtilizationPercent)}，显存 {Drawing.FormatMemory(gpu.MemoryUsedMiB)} / {Drawing.FormatMemory(gpu.MemoryTotalMiB)}。";
        var title = $"GPU 空闲 · {args.ServerLabel}";
        var body = $"{gpuText} 上的当前用户进程已全部退出。{metrics} 点击查看服务器详情。";

        Show(
            title,
            body,
            args.ServerId,
            $"idle-{args.ServerId:N}",
            () => _notifyIcon.ShowBalloonTip(8000, title, body, ToolTipIcon.Info));
    }

    public void ShowTest(Guid? serverId, string? serverLabel)
    {
        var title = "GPU 观察测试通知";
        var body = string.IsNullOrWhiteSpace(serverLabel)
            ? "通知已可用。当当前用户在该 GPU 上的进程全部退出时，会在这里提醒你。"
            : $"通知已可用。{serverLabel} 上的 GPU 进程全部退出时，会在这里提醒你。";

        Show(
            title,
            body,
            serverId,
            "test",
            () => _notifyIcon.ShowBalloonTip(5000, title, body, ToolTipIcon.Info));
    }

    public Guid? TakeBalloonServerId()
    {
        var id = _lastBalloonServerId;
        _lastBalloonServerId = null;
        return id;
    }

    private void Show(string title, string body, Guid? serverId, string tag, Action balloonFallback)
    {
        try
        {
            ToastNotificationManagerCompat.CreateToastNotifier();
        }
        catch
        {
            // Registration is also attempted by Show(); ignore and try.
        }

        try
        {
            var builder = new ToastContentBuilder();
            if (serverId is { } id)
            {
                builder.AddArgument("action", "open-server");
                builder.AddArgument("serverId", id.ToString("D"));
            }
            else
            {
                builder.AddArgument("action", "show");
            }

            builder.AddText(title);
            builder.AddText(body);
            builder.AddAttributionText("GPU 观察");
            builder.Show(toast =>
            {
                toast.Tag = tag;
                toast.Group = "gpu-observe";
                toast.ExpirationTime = DateTimeOffset.Now.AddMinutes(30);
            });
        }
        catch (Exception ex)
        {
            Log.Error("显示系统通知失败，改用托盘气泡。", ex);
            _lastBalloonServerId = serverId;
            try
            {
                balloonFallback();
            }
            catch (Exception balloonException)
            {
                Log.Error("显示托盘气泡失败。", balloonException);
            }
        }
    }
}
