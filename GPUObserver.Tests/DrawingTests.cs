using System.Drawing;
using System.Threading;
using System.Windows.Forms;
using GPUObserver.UI;

namespace GPUObserver.Tests;

public class DrawingTests
{
    [Fact]
    public void DrawText_HonorsGraphicsTranslation()
    {
        Exception? failure = null;
        var thread = new Thread(() =>
        {
            try
            {
                using var bitmap = new Bitmap(260, 150);
                using (var graphics = Graphics.FromImage(bitmap))
                {
                    graphics.Clear(Color.White);
                    graphics.TranslateTransform(0, 80);
                    using var font = new Font("Microsoft YaHei UI", 12f);
                    Drawing.DrawText(
                        graphics,
                        "GPU 0 TEST",
                        font,
                        Color.Black,
                        new Rectangle(0, 0, 240, 26),
                        TextFormatFlags.Left | TextFormatFlags.Top | TextFormatFlags.NoPrefix);
                }

                var upperDark = CountDarkPixels(bitmap, 0, 80);
                var lowerDark = CountDarkPixels(bitmap, 80, 150);

                Assert.True(lowerDark > 25, $"Expected translated text pixels, found {lowerDark}.");
                Assert.True(upperDark < 5, $"Unexpected untranslated text pixels: {upperDark}.");
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
        thread.Join(TimeSpan.FromSeconds(15));

        Assert.False(thread.IsAlive);
        Assert.Null(failure);
    }

    private static int CountDarkPixels(Bitmap bitmap, int top, int bottom)
    {
        var count = 0;
        for (var y = top; y < bottom; y++)
        {
            for (var x = 0; x < bitmap.Width; x++)
            {
                var color = bitmap.GetPixel(x, y);
                if (color.R < 100 && color.G < 100 && color.B < 100)
                {
                    count++;
                }
            }
        }

        return count;
    }
}
