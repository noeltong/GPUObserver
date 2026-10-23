using System.Text;

namespace GPUObserver.Util;

internal static class Log
{
    private static readonly object Gate = new();
    private static readonly string LogDirectory = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
        "GPUObserver",
        "logs");

    private const long MaxBytes = 1024 * 1024;

    public static void Info(string message) => Write("INFO", message, null);

    public static void Error(string message, Exception? exception = null) => Write("ERROR", message, exception);

    private static void Write(string level, string message, Exception? exception)
    {
        try
        {
            lock (Gate)
            {
                Directory.CreateDirectory(LogDirectory);
                var path = Path.Combine(LogDirectory, "app.log");
                RotateIfNeeded(path);

                var builder = new StringBuilder();
                builder.Append(DateTimeOffset.Now.ToString("yyyy-MM-dd HH:mm:ss.fff zzz")).Append(" [").Append(level).Append("] ").Append(message);
                if (exception is not null)
                {
                    builder.Append(" | ").Append(exception.GetType().Name).Append(": ").Append(exception.Message);
                }
                builder.AppendLine();
                File.AppendAllText(path, builder.ToString(), Encoding.UTF8);
            }
        }
        catch
        {
            // Diagnostics must never take down the tray application.
        }
    }

    private static void RotateIfNeeded(string path)
    {
        if (!File.Exists(path))
        {
            return;
        }

        var info = new FileInfo(path);
        if (info.Length < MaxBytes)
        {
            return;
        }

        var old = Path.Combine(LogDirectory, "app.old.log");
        if (File.Exists(old))
        {
            File.Delete(old);
        }
        File.Move(path, old);
    }
}
