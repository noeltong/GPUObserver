using System.Security.Cryptography;
using GPUObserver.Monitoring;
using GPUObserver.Ssh;

namespace GPUObserver.Tests;

public class SshKeyTests
{
    [Fact]
    public void PrivateKeyLoader_LoadsPemPrivateKey()
    {
        var directory = Path.Combine(Path.GetTempPath(), "gpuobserver-keytest-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(directory);

        try
        {
            var path = Path.Combine(directory, "id_rsa.pem");
            using (var rsa = RSA.Create(2048))
            {
                File.WriteAllText(path, rsa.ExportPkcs8PrivateKeyPem());
            }

            var key = PrivateKeyLoader.Load(path, "");

            Assert.NotNull(key);
        }
        finally
        {
            Directory.Delete(directory, recursive: true);
        }
    }

    [Fact]
    public void PrivateKeyLoader_RejectsPuttyKeyWithActionableError()
    {
        var directory = Path.Combine(Path.GetTempPath(), "gpuobserver-keytest-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(directory);

        try
        {
            var path = Path.Combine(directory, "id.ppk");
            File.WriteAllText(path, "PuTTY-User-Key-File-3: ssh-ed25519\nEncryption: none\n");

            var error = Assert.Throws<MonitorOperationException>(() => PrivateKeyLoader.Load(path, ""));

            Assert.Equal(MonitorErrorKind.PrivateKey, error.Kind);
            Assert.Contains("PuTTY", error.Message);
        }
        finally
        {
            Directory.Delete(directory, recursive: true);
        }
    }
}
