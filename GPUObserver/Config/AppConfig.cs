using System.Text.Json.Serialization;

namespace GPUObserver.Config;

public enum ServerAuthType
{
    Password,
    PrivateKey
}

public sealed class AppConfig
{
    public int SchemaVersion { get; set; } = 1;
    public int RefreshSeconds { get; set; } = 5;
    public bool NotificationsEnabled { get; set; } = true;
    public bool StartWithWindows { get; set; }
    public List<ServerConfig> Servers { get; set; } = [];

    public AppConfig Clone() => new()
    {
        SchemaVersion = SchemaVersion,
        RefreshSeconds = RefreshSeconds,
        NotificationsEnabled = NotificationsEnabled,
        StartWithWindows = StartWithWindows,
        Servers = Servers.Select(server => server.Clone()).ToList()
    };
}

public sealed class ServerConfig
{
    public Guid Id { get; set; } = Guid.NewGuid();
    public string Host { get; set; } = "";
    public int Port { get; set; } = 22;
    public string Username { get; set; } = "";
    public ServerAuthType AuthType { get; set; } = ServerAuthType.Password;
    public string? ProtectedPassword { get; set; }
    public string? PrivateKeyPath { get; set; }
    public string? ProtectedKeyPassphrase { get; set; }
    public bool NotificationsEnabled { get; set; } = true;
    public string? HostKeyFingerprintSha256 { get; set; }

    [JsonIgnore]
    public string DisplayHost => Port == 22 ? Host : $"{Host}:{Port}";

    [JsonIgnore]
    public string DisplayUserHost => $"{Username}@{DisplayHost}";

    public ServerConfig Clone() => new()
    {
        Id = Id,
        Host = Host,
        Port = Port,
        Username = Username,
        AuthType = AuthType,
        ProtectedPassword = ProtectedPassword,
        PrivateKeyPath = PrivateKeyPath,
        ProtectedKeyPassphrase = ProtectedKeyPassphrase,
        NotificationsEnabled = NotificationsEnabled,
        HostKeyFingerprintSha256 = HostKeyFingerprintSha256
    };
}
