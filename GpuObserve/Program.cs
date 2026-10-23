using GpuObserve.App;
using GpuObserve.Notifications;
using GpuObserve.Util;

namespace GpuObserve;

internal static class Program
{
    private const string InstanceName = "GpuObserve.Instance.v1";

    [STAThread]
    private static void Main(string[] args)
    {
        ToastActivation.Initialize();

        var startHidden = args.Any(argument => argument.Equals("--autostart", StringComparison.OrdinalIgnoreCase));

        SingleInstance? instance = null;
        try
        {
            instance = new SingleInstance(InstanceName);
            if (!instance.IsPrimary)
            {
                if (!startHidden)
                {
                    instance.Send("show");

                    // A toast click starts the exe through COM; wait briefly so
                    // this secondary process can forward the target server.
                    var pending = ToastActivation.WaitForPending(TimeSpan.FromSeconds(1.2));
                    var serverId = ToastActivation.ParseServerId(pending);
                    if (serverId is { } id)
                    {
                        instance.Send($"open-server:{id:D}");
                    }
                }

                instance.Dispose();
                return;
            }

            ApplicationConfiguration.Initialize();
            Application.SetUnhandledExceptionMode(UnhandledExceptionMode.CatchException);
            Application.ThreadException += (_, eventArgs) =>
            {
                Log.Error("未处理的 UI 异常。", eventArgs.Exception);
                MessageBox.Show(
                    $"GPU 观察遇到错误：{eventArgs.Exception.Message}",
                    "GPU 观察",
                    MessageBoxButtons.OK,
                    MessageBoxIcon.Error);
            };
            AppDomain.CurrentDomain.UnhandledException += (_, eventArgs) =>
            {
                if (eventArgs.ExceptionObject is Exception exception)
                {
                    Log.Error("未处理的异常。", exception);
                }
            };
            TaskScheduler.UnobservedTaskException += (_, eventArgs) =>
            {
                Log.Error("未观察到的后台异常。", eventArgs.Exception);
                eventArgs.SetObserved();
            };

            using var context = new TrayAppContext(instance, startHidden);
            instance = null;
            Application.Run(context);
        }
        catch (Exception ex)
        {
            Log.Error("应用启动失败。", ex);
            try
            {
                MessageBox.Show(
                    $"GPU 观察启动失败：{ex.Message}",
                    "GPU 观察",
                    MessageBoxButtons.OK,
                    MessageBoxIcon.Error);
            }
            catch
            {
                // ignored
            }

            instance?.Dispose();
        }
    }
}
