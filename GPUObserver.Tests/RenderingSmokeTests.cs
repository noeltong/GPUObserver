using System.Drawing.Imaging;
using GPUObserver.Config;
using GPUObserver.Monitoring;
using GPUObserver.UI;

namespace GPUObserver.Tests;

public class RenderingSmokeTests
{
    [Fact]
    public void RendersGpuCardsMemoryProcessesAndSparklines()
    {
        Exception? failure = null;
        var thread = new Thread(() =>
        {
            try
            {
                var now = DateTimeOffset.UtcNow;
                var gpus = new[]
                {
                    new GpuTelemetry(0, "GPU-0", "NVIDIA GeForce RTX 4090", 82, 19456, 24564, 66, 310, 450),
                    new GpuTelemetry(1, "GPU-1", "NVIDIA A100-SXM4-80GB", 34, 30720, 81920, 48, 190, 400)
                };
                var processes = new[]
                {
                    new GpuProcessTelemetry(0, 18432, "alice", "python", 18200),
                    new GpuProcessTelemetry(0, 18440, "alice", "jupyter-lab", 1150),
                    new GpuProcessTelemetry(1, 19001, "alice", "train.py", 30000)
                };

                var history = new List<ServerSnapshot>();
                for (var i = 0; i < 60; i++)
                {
                    var captured = now.AddSeconds(-295 + i * 5);
                    var phase = i / 8.0;
                    history.Add(new ServerSnapshot(
                        Guid.NewGuid(),
                        captured,
                        new[]
                        {
                            new GpuTelemetry(0, "GPU-0", gpus[0].Name,
                                Math.Clamp(55 + Math.Sin(phase) * 35, 0, 100),
                                16000 + (long)(Math.Sin(phase) * 4000),
                                24564, 62, 280, 450),
                            new GpuTelemetry(1, "GPU-1", gpus[1].Name,
                                Math.Clamp(20 + Math.Cos(phase / 2) * 20, 0, 100),
                                28000 + (long)(Math.Cos(phase) * 3000),
                                81920, 45, 170, 400)
                        },
                        new RamTelemetry(64000, 39000, 23000),
                        processes,
                        true,
                        new Dictionary<int, int> { [0] = 2, [1] = 1 },
                        null));
                }

                var serverId = Guid.NewGuid();
                var state = new ServerViewState(serverId, MonitorPhase.Online, null, history);
                var server = new ServerConfig
                {
                    Id = serverId,
                    Host = "192.168.10.24",
                    Port = 22,
                    Username = "alice"
                };

                using var panel = new ServerContentPanel { Size = new Size(428, 760) };
                panel.SetData(server, state, 5);

                using var bitmap = new Bitmap(panel.Width, panel.Height);
                panel.DrawToBitmap(bitmap, new Rectangle(0, 0, panel.Width, panel.Height));

                var path = Path.Combine(Path.GetTempPath(), "gpuobserver-panel-preview.png");
                bitmap.Save(path, ImageFormat.Png);
                Assert.True(File.Exists(path));
            }
            catch (Exception ex)
            {
                failure = ex;
            }
        })
        {
            IsBackground = true
        };
        thread.SetApartmentState(ApartmentState.STA);
        thread.Start();
        thread.Join(TimeSpan.FromSeconds(20));

        Assert.Null(failure);
    }
}
