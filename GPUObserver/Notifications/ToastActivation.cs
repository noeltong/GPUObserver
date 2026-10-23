using GPUObserver.Util;
using Microsoft.Toolkit.Uwp.Notifications;

namespace GPUObserver.Notifications;

public static class ToastActivation
{
    private static readonly object Gate = new();
    private static string? _pendingArgument;
    private static bool _initialized;

    public static event EventHandler<Guid?>? Activated;

    public static void Initialize()
    {
        if (_initialized)
        {
            return;
        }

        _initialized = true;
        try
        {
            ToastNotificationManagerCompat.OnActivated += args =>
            {
                lock (Gate)
                {
                    _pendingArgument = args.Argument;
                }

                try
                {
                    Activated?.Invoke(null, ParseServerId(args.Argument));
                }
                catch (Exception ex)
                {
                    Log.Error("处理通知点击失败。", ex);
                }
            };
        }
        catch (Exception ex)
        {
            Log.Error("初始化通知激活失败。", ex);
        }
    }

    public static Guid? ParseServerId(string? argument)
    {
        if (string.IsNullOrWhiteSpace(argument))
        {
            return null;
        }

        try
        {
            var arguments = ToastArguments.Parse(argument);
            if (arguments.TryGetValue("serverId", out var raw) && Guid.TryParse(raw, out var id))
            {
                return id;
            }
        }
        catch
        {
            // Fall back to query-string parsing below.
        }

        foreach (var part in argument.Split('&', StringSplitOptions.RemoveEmptyEntries))
        {
            var pair = part.Split('=', 2);
            if (pair.Length == 2 && pair[0].Equals("serverId", StringComparison.OrdinalIgnoreCase)
                && Guid.TryParse(pair[1], out var id))
            {
                return id;
            }
        }

        return null;
    }

    public static string? TakePending()
    {
        lock (Gate)
        {
            var pending = _pendingArgument;
            _pendingArgument = null;
            return pending;
        }
    }

    public static string? WaitForPending(TimeSpan timeout)
    {
        var deadline = DateTime.UtcNow + timeout;
        while (DateTime.UtcNow < deadline)
        {
            var pending = TakePending();
            if (!string.IsNullOrWhiteSpace(pending))
            {
                return pending;
            }

            Thread.Sleep(50);
        }

        return null;
    }
}
