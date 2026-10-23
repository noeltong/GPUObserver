using System.Reflection;

namespace GPUObserver.UI;

internal static class TrayIconFactory
{
    private const string ResourceName = "GPUObserver.Assets.app.ico";

    public static Icon LoadAppIcon()
    {
        try
        {
            using var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream(ResourceName);
            if (stream is not null)
            {
                return new Icon(stream);
            }
        }
        catch
        {
            // fall through to the system icon
        }

        return (Icon)SystemIcons.Application.Clone();
    }
}
