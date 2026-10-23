namespace GPUObserver.UI;

internal sealed class IconButton : Control
{
    private readonly Font _glyphFont;
    private bool _hovered;
    private bool _pressed;

    public IconButton(string glyph, string accessibleName)
    {
        Glyph = glyph;
        AccessibleName = accessibleName;
        AccessibleRole = AccessibleRole.PushButton;
        SetStyle(
            ControlStyles.UserPaint
            | ControlStyles.OptimizedDoubleBuffer
            | ControlStyles.AllPaintingInWmPaint
            | ControlStyles.SupportsTransparentBackColor,
            true);
        BackColor = Color.Transparent;
        Size = new Size(30, 30);
        Cursor = Cursors.Hand;
        TabStop = false;
        _glyphFont = Theme.IconFont(10f);
    }

    public string Glyph { get; set; }

    protected override void OnPaint(PaintEventArgs e)
    {
        e.Graphics.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.AntiAlias;
        if ((_hovered || _pressed) && Enabled)
        {
            var background = _pressed ? Theme.Pressed : Theme.Hover;
            Drawing.FillRounded(e.Graphics, background, new RectangleF(0, 0, Width, Height), 7f);
        }

        var color = !Enabled ? Theme.MutedText : (_pressed ? Theme.AccentDark : Theme.Text);
        TextRenderer.DrawText(
            e.Graphics,
            Glyph,
            _glyphFont,
            ClientRectangle,
            color,
            TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter | TextFormatFlags.NoPrefix);
    }

    protected override void OnMouseEnter(EventArgs e)
    {
        _hovered = true;
        Invalidate();
        base.OnMouseEnter(e);
    }

    protected override void OnMouseLeave(EventArgs e)
    {
        _hovered = false;
        _pressed = false;
        Invalidate();
        base.OnMouseLeave(e);
    }

    protected override void OnMouseDown(MouseEventArgs e)
    {
        if (e.Button == MouseButtons.Left)
        {
            _pressed = true;
            Invalidate();
        }

        base.OnMouseDown(e);
    }

    protected override void OnMouseUp(MouseEventArgs e)
    {
        _pressed = false;
        Invalidate();
        base.OnMouseUp(e);
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _glyphFont.Dispose();
        }

        base.Dispose(disposing);
    }
}
