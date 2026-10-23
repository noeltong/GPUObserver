using System.Security.Cryptography;
using System.Text;
using GpuObserve.Monitoring;
using Renci.SshNet;

namespace GpuObserve.Ssh;

/// <summary>
/// Loads an OpenSSH/PEM private key entirely inside the app. The key is parsed
/// for each connection and never handed to an external agent or ssh.exe.
/// </summary>
public static class PrivateKeyLoader
{
    public static IPrivateKeySource Load(string path, string? passphrase)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            throw new MonitorOperationException(MonitorErrorKind.PrivateKey, "未选择私钥文件。");
        }

        string fullPath;
        try
        {
            fullPath = Path.GetFullPath(path);
        }
        catch (Exception ex)
        {
            throw new MonitorOperationException(MonitorErrorKind.PrivateKey, "私钥文件路径无效。", ex.Message, ex);
        }

        if (!File.Exists(fullPath))
        {
            throw new MonitorOperationException(MonitorErrorKind.PrivateKey, "私钥文件不存在。", fullPath);
        }

        if (LooksLikePuttyKey(fullPath))
        {
            throw new MonitorOperationException(
                MonitorErrorKind.PrivateKey,
                "不支持 PuTTY .ppk 私钥，请先转换为 OpenSSH 格式。");
        }

        try
        {
            return new PrivateKeyFile(fullPath, passphrase ?? "");
        }
        catch (UnauthorizedAccessException ex)
        {
            throw new MonitorOperationException(MonitorErrorKind.PrivateKey, "没有权限读取私钥文件。", ex.Message, ex);
        }
        catch (Exception ex)
        {
            throw new MonitorOperationException(
                MonitorErrorKind.PrivateKey,
                "无法加载私钥，请检查格式和口令。",
                ex.Message,
                ex);
        }
    }

    private static bool LooksLikePuttyKey(string path)
    {
        try
        {
            using var stream = File.OpenRead(path);
            var buffer = new byte[64];
            var read = stream.Read(buffer, 0, buffer.Length);
            var header = Encoding.ASCII.GetString(buffer, 0, read);
            return header.Contains("PuTTY-User-Key-File", StringComparison.Ordinal);
        }
        catch
        {
            return false;
        }
    }
}
