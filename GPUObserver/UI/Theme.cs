using System.Drawing.Text;

namespace GPUObserver.UI;

internal static class Theme
{
    public static readonly Color WindowBackground = Color.FromArgb(247, 248, 250);
    public static readonly Color CardBackground = Color.White;
    public static readonly Color Border = Color.FromArgb(226, 229, 234);
    public static readonly Color BorderStrong = Color.FromArgb(210, 215, 222);
    public static readonly Color Text = Color.FromArgb(25, 28, 33);
    public static readonly Color SecondaryText = Color.FromArgb(98, 106, 118);
    public static readonly Color MutedText = Color.FromArgb(143, 149, 160);
    public static readonly Color Accent = Color.FromArgb(37, 99, 235);
    public static readonly Color AccentDark = Color.FromArgb(29, 78, 187);
    public static readonly Color AccentSoft = Color.FromArgb(232, 240, 254);
    public static readonly Color Purple = Color.FromArgb(124, 58, 237);
    public static readonly Color Green = Color.FromArgb(16, 124, 65);
    public static readonly Color GreenSoft = Color.FromArgb(229, 247, 237);
    public static readonly Color Amber = Color.FromArgb(180, 83, 9);
    public static readonly Color AmberSoft = Color.FromArgb(254, 243, 199);
    public static readonly Color Red = Color.FromArgb(185, 28, 28);
    public static readonly Color RedSoft = Color.FromArgb(254, 226, 226);
    public static readonly Color Track = Color.FromArgb(235, 238, 243);
    public static readonly Color Hover = Color.FromArgb(238, 241, 246);
    public static readonly Color Pressed = Color.FromArgb(228, 233, 240);
    public static readonly Color ChartBackground = Color.FromArgb(250, 251, 253);
    public static readonly Color ChartGrid = Color.FromArgb(235, 238, 243);

    public const string UiFontFamily = "Microsoft YaHei UI";
    public const string IconFontFamily = "Segoe MDL2 Assets";
    public const string MonoFontFamily = "Consolas";

    public static readonly string GlyphAdd = "\uE710";
    public static readonly string GlyphSettings = "\uE713";
    public static readonly string GlyphRefresh = "\uE72C";
    public static readonly string GlyphWarning = "\uE7BA";
    public static readonly string GlyphError = "\uE783";
    public static readonly string GlyphCheck = "\uE73E";
    public static readonly string GlyphLeft = "\uE76B";
    public static readonly string GlyphRight = "\uE76C";

    public static Font UiFont(float points, FontStyle style = FontStyle.Regular) =>
        new(UiFontFamily, points, style, GraphicsUnit.Point);

    public static Font IconFont(float points) =>
        new(IconFontFamily, points, FontStyle.Regular, GraphicsUnit.Point);

    public static Font MonoFont(float points, FontStyle style = FontStyle.Regular) =>
        new(MonoFontFamily, points, style, GraphicsUnit.Point);

    public static Color UsageColor(double? percent) => percent switch
    {
        null => MutedText,
        >= 90 => Red,
        >= 70 => Amber,
        _ => Accent
    };

    public static Color MemoryColor(double? percent) => percent switch
    {
        null => MutedText,
        >= 95 => Red,
        >= 80 => Amber,
        _ => Purple
    };

    public static Color PhaseColor(Monitoring.MonitorPhase phase) => phase switch
    {
        Monitoring.MonitorPhase.Online => Green,
        Monitoring.MonitorPhase.Connecting => Amber,
        Monitoring.MonitorPhase.Error => Red,
        _ => MutedText
    };
}
