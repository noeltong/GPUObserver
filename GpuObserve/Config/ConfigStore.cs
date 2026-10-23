using System.Text.Json;
using System.Text.Json.Serialization;

namespace GpuObserve.Config;

public sealed record ConfigLoadResult(AppConfig Config, bool CreatedNew, string? Warning);

public static class ConfigStore
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
        PropertyNameCaseInsensitive = true,
        Converters = { new JsonStringEnumConverter() }
    };

    public static string ConfigDirectory { get; } = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
        "GpuObserve");

    public static string ConfigPath { get; } = Path.Combine(ConfigDirectory, "config.json");

    public static ConfigLoadResult Load()
    {
        try
        {
            if (!File.Exists(ConfigPath))
            {
                return new ConfigLoadResult(Normalize(new AppConfig()), true, null);
            }

            var json = File.ReadAllText(ConfigPath);
            var config = JsonSerializer.Deserialize<AppConfig>(json, JsonOptions);
            if (config is null)
            {
                throw new JsonException("配置文件内容为空。");
            }

            return new ConfigLoadResult(Normalize(config), false, null);
        }
        catch (Exception ex)
        {
            var warning = "配置文件无法读取，已备份并使用默认设置。";
            TryBackupCorruptFile();
            Util.Log.Error(warning, ex);
            return new ConfigLoadResult(Normalize(new AppConfig()), false, warning);
        }
    }

    public static void Save(AppConfig config)
    {
        Normalize(config);
        Directory.CreateDirectory(ConfigDirectory);

        var tempPath = ConfigPath + ".tmp";
        try
        {
            var json = JsonSerializer.Serialize(config, JsonOptions);
            using (var stream = new FileStream(tempPath, FileMode.Create, FileAccess.Write, FileShare.None))
            using (var writer = new StreamWriter(stream))
            {
                writer.Write(json);
                writer.Flush();
                stream.Flush(true);
            }

            File.Move(tempPath, ConfigPath, overwrite: true);
        }
        catch
        {
            TryDelete(tempPath);
            throw;
        }
    }

    private static AppConfig Normalize(AppConfig config)
    {
        config.SchemaVersion = Math.Max(1, config.SchemaVersion);
        config.RefreshSeconds = Math.Clamp(config.RefreshSeconds <= 0 ? 5 : config.RefreshSeconds, 2, 300);
        config.Servers ??= [];

        var ids = new HashSet<Guid>();
        foreach (var server in config.Servers)
        {
            server.Host = (server.Host ?? "").Trim();
            server.Username = (server.Username ?? "").Trim();
            server.PrivateKeyPath = string.IsNullOrWhiteSpace(server.PrivateKeyPath) ? null : server.PrivateKeyPath.Trim();
            server.Port = server.Port is < 1 or > 65535 ? 22 : server.Port;
            if (server.Id == Guid.Empty || !ids.Add(server.Id))
            {
                server.Id = Guid.NewGuid();
                ids.Add(server.Id);
            }

            if (!Enum.IsDefined(server.AuthType))
            {
                server.AuthType = ServerAuthType.Password;
            }
        }

        return config;
    }

    private static void TryBackupCorruptFile()
    {
        try
        {
            if (!File.Exists(ConfigPath))
            {
                return;
            }

            var backup = Path.Combine(
                ConfigDirectory,
                $"config.corrupt-{DateTime.Now:yyyyMMdd-HHmmss}.json");
            File.Move(ConfigPath, backup, overwrite: false);
        }
        catch
        {
            // Keep starting with defaults even if the backup cannot be made.
        }
    }

    private static void TryDelete(string path)
    {
        try
        {
            if (File.Exists(path))
            {
                File.Delete(path);
            }
        }
        catch
        {
            // ignored
        }
    }
}
