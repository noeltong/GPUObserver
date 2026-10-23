using GPUObserver.Config;
using GPUObserver.Monitoring;
using GPUObserver.Monitoring.Parsing;

namespace GPUObserver.Ssh;

public sealed record ConnectionTestResult(
    bool Connected,
    int GpuCount,
    bool RamAvailable,
    string Message,
    string? Fingerprint,
    MonitorError? Error);

public static class ConnectionTester
{
    public static async Task<ConnectionTestResult> TestAsync(ServerConfig config, CancellationToken cancellationToken)
    {
        using var session = new SshSession(config, _ => { });

        try
        {
            await session.ConnectAsync(cancellationToken).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (MonitorOperationException ex)
        {
            return new ConnectionTestResult(
                false,
                0,
                false,
                ex.Message,
                config.HostKeyFingerprintSha256,
                new MonitorError(ex.Kind, ex.Message, ex.Detail));
        }
        catch (Exception ex)
        {
            var message = Describe(ex);
            return new ConnectionTestResult(
                false,
                0,
                false,
                message,
                config.HostKeyFingerprintSha256,
                new MonitorError(MonitorErrorKind.Unknown, message, ex.Message));
        }

        var fingerprint = config.HostKeyFingerprintSha256;

        try
        {
            var commandResult = await session
                .RunAsync(ProbeCommand.BuildMainCommand(), cancellationToken, TimeSpan.FromSeconds(20))
                .ConfigureAwait(false);
            var probe = ProbeParser.Parse(commandResult.StandardOutput);

            if (probe.NvSmiMissing)
            {
                return new ConnectionTestResult(
                    true,
                    0,
                    probe.Ram is not null,
                    "SSH 连接成功，但服务器上未找到 nvidia-smi。请确认 NVIDIA 驱动已安装。",
                    fingerprint,
                    new MonitorError(MonitorErrorKind.NvidiaSmiMissing, "未找到 nvidia-smi。"));
            }

            if (probe.Gpus.Count == 0)
            {
                return new ConnectionTestResult(
                    true,
                    0,
                    probe.Ram is not null,
                    "SSH 连接成功，但没有读取到 GPU 信息。",
                    fingerprint,
                    new MonitorError(MonitorErrorKind.Command, "未读取到 GPU 信息。", commandResult.StandardError));
            }

            var ramText = probe.Ram is not null ? "，内存信息可用" : "";
            return new ConnectionTestResult(
                true,
                probe.Gpus.Count,
                probe.Ram is not null,
                $"连接成功：检测到 {probe.Gpus.Count} 块 NVIDIA GPU{ramText}。",
                fingerprint,
                null);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex)
        {
            var message = $"SSH 连接成功，但读取 GPU 数据失败：{Describe(ex)}";
            return new ConnectionTestResult(
                true,
                0,
                false,
                message,
                fingerprint,
                new MonitorError(MonitorErrorKind.Command, message, ex.Message));
        }
    }

    private static string Describe(Exception exception)
    {
        return exception switch
        {
            MonitorOperationException operation => operation.Message,
            Renci.SshNet.Common.SshAuthenticationException => "SSH 认证失败，请检查用户名、密码或私钥。",
            TimeoutException => "操作超时，请检查服务器网络和负载。",
            _ => $"连接测试失败：{exception.Message}"
        };
    }
}
