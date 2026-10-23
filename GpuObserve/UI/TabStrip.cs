using GpuObserve.Monitoring;

namespace GpuObserve.UI;

internal sealed record TabStripItem(Guid Id, string Label, MonitorPhase Phase);

internal sealed class TabStrip : Control
{
    private readonly Font _labelFont;
    private readonly Font _arrowFont;
    private readonly ToolTip _toolTip = new();
    private readonly List<TabStripItem> _items = [];
    private readonly List<(Guid Id, RectangleF Bounds)> _tabBounds = [];

    private Guid? _selectedId;
    private int _visibleStart;
    private int _visibleCount;
    private int _hoverIndex = -1;
    private int _hoverArrow;
    private bool _overflow;
    private RectangleF _leftArrow;
    private RectangleF _rightArrow;
    private int _lastTooltipIndex = -2;

    public TabStrip()
    {
        SetStyle(
            ControlStyles.UserPaint
            | ControlStyles.OptimizedDoubleBuffer
            | ControlStyles.AllPaintingInWmPaint
            | ControlStyles.SupportsTransparentBackColor
            | ControlStyles.ResizeRedraw,
            true);
        BackColor = Color.Transparent;
        Height = 32;
        TabStop = false;
        _labelFont = Theme.UiFont(9.25f);
        _arrowFont = Theme.IconFont(7.5f);
    }

    public event EventHandler<Guid>? Selected;

    public Guid? SelectedId => _selectedId;

    public void SetItems(IReadOnlyList<TabStripItem> items, Guid? selectedId)
    {
        var selectionChanged = selectedId != _selectedId;
        _items.Clear();
        _items.AddRange(items);
        _selectedId = items.Any(item => item.Id == selectedId) ? selectedId : items.FirstOrDefault()?.Id;

        if (selectionChanged)
        {
            EnsureSelectedVisible();
        }

        RecalculateLayout();
        Invalidate();
    }

    public void Select(Guid? id, bool raiseEvent = false)
    {
        if (id == _selectedId || (id is not null && _items.All(item => item.Id != id)))
        {
            return;
        }

        _selectedId = id;
        EnsureSelectedVisible();
        RecalculateLayout();
        Invalidate();
        if (raiseEvent && id is { } value)
        {
            Selected?.Invoke(this, value);
        }
    }

    protected override void OnResize(EventArgs e)
    {
        base.OnResize(e);
        RecalculateLayout();
        Invalidate();
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        e.Graphics.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.AntiAlias;
        e.Graphics.Clear(Parent?.BackColor ?? Theme.WindowBackground);

        if (_items.Count == 0)
        {
            return;
        }

        if (_overflow)
        {
            DrawArrow(e.Graphics, _leftArrow, Theme.GlyphLeft, _visibleStart > 0, _hoverArrow == -1);
            DrawArrow(e.Graphics, _rightArrow, Theme.GlyphRight, _visibleStart + _visibleCount < _items.Count, _hoverArrow == 1);
        }

        foreach (var (id, bounds) in _tabBounds)
        {
            var index = _items.FindIndex(item => item.Id == id);
            if (index < 0)
            {
                continue;
            }

            var item = _items[index];
            var selected = item.Id == _selectedId;
            var hovered = index == _hoverIndex;

            if (selected)
            {
                Drawing.FillRounded(e.Graphics, Theme.CardBackground, bounds, S(8));
                using var border = new Pen(Theme.BorderStrong, 1f);
                Drawing.DrawRounded(
                    e.Graphics,
                    border,
                    new RectangleF(bounds.X + 0.5f, bounds.Y + 0.5f, bounds.Width - 1f, bounds.Height - 1f),
                    S(8));
            }
            else if (hovered)
            {
                Drawing.FillRounded(e.Graphics, Theme.Hover, bounds, S(8));
            }

            Drawing.DrawStatusDot(
                e.Graphics,
                bounds.Left + S(11),
                bounds.Top + bounds.Height / 2f,
                S(6),
                Theme.PhaseColor(item.Phase));

            var labelBounds = Rectangle.Round(new RectangleF(
                bounds.Left + S(20),
                bounds.Top,
                Math.Max(0, bounds.Width - S(28)),
                bounds.Height));
            TextRenderer.DrawText(
                e.Graphics,
                item.Label,
                _labelFont,
                labelBounds,
                selected ? Theme.Text : Theme.SecondaryText,
                TextFormatFlags.Left
                | TextFormatFlags.VerticalCenter
                | TextFormatFlags.EndEllipsis
                | TextFormatFlags.SingleLine
                | TextFormatFlags.NoPrefix);
        }
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        base.OnMouseMove(e);
        var newHover = HitTest(e.Location, out var arrow);
        if (newHover != _hoverIndex || arrow != _hoverArrow)
        {
            _hoverIndex = newHover;
            _hoverArrow = arrow;
            Cursor = newHover >= 0 || arrow != 0 ? Cursors.Hand : Cursors.Default;
            Invalidate();
        }

        if (newHover >= 0 && newHover != _lastTooltipIndex)
        {
            _lastTooltipIndex = newHover;
            _toolTip.SetToolTip(this, _items[newHover].Label);
        }
        else if (newHover < 0 && _lastTooltipIndex != -2)
        {
            _lastTooltipIndex = -2;
            _toolTip.SetToolTip(this, "");
        }
    }

    protected override void OnMouseLeave(EventArgs e)
    {
        base.OnMouseLeave(e);
        _hoverIndex = -1;
        _hoverArrow = 0;
        _lastTooltipIndex = -2;
        _toolTip.SetToolTip(this, "");
        Invalidate();
    }

    protected override void OnMouseUp(MouseEventArgs e)
    {
        base.OnMouseUp(e);
        if (e.Button != MouseButtons.Left)
        {
            return;
        }

        if (_overflow && _leftArrow.Contains(e.Location) && _visibleStart > 0)
        {
            _visibleStart--;
            RecalculateLayout();
            Invalidate();
            return;
        }

        if (_overflow && _rightArrow.Contains(e.Location) && _visibleStart + _visibleCount < _items.Count)
        {
            _visibleStart++;
            RecalculateLayout();
            Invalidate();
            return;
        }

        var index = HitTest(e.Location, out _);
        if (index < 0)
        {
            return;
        }

        var item = _items[index];
        if (_selectedId == item.Id)
        {
            return;
        }

        _selectedId = item.Id;
        RecalculateLayout();
        Invalidate();
        Selected?.Invoke(this, item.Id);
    }

    protected override void OnMouseWheel(MouseEventArgs e)
    {
        base.OnMouseWheel(e);
        if (!_overflow)
        {
            return;
        }

        if (e.Delta > 0 && _visibleStart > 0)
        {
            _visibleStart--;
            RecalculateLayout();
            Invalidate();
        }
        else if (e.Delta < 0 && _visibleStart + _visibleCount < _items.Count)
        {
            _visibleStart++;
            RecalculateLayout();
            Invalidate();
        }
    }

    private void RecalculateLayout()
    {
        _tabBounds.Clear();
        _overflow = false;
        _visibleCount = 0;

        if (_items.Count == 0 || Width <= 0)
        {
            return;
        }

        var widths = _items.Select(MeasureTab).ToList();
        var gap = S(4);
        var total = widths.Sum() + Math.Max(0, _items.Count - 1) * gap;
        _overflow = total > Width;

        var arrowWidth = S(22);
        _leftArrow = new RectangleF(0, S(4), arrowWidth, Height - S(8));
        _rightArrow = new RectangleF(Width - arrowWidth, S(4), arrowWidth, Height - S(8));

        _visibleStart = _overflow
            ? Math.Clamp(_visibleStart, 0, Math.Max(0, _items.Count - 1))
            : 0;

        var contentLeft = _overflow ? (float)arrowWidth : 0f;
        var contentRight = _overflow ? Width - arrowWidth : Width;
        var x = contentLeft;

        for (var i = _visibleStart; i < _items.Count; i++)
        {
            var width = widths[i];
            if (_tabBounds.Count > 0 && x + width > contentRight)
            {
                break;
            }

            if (_tabBounds.Count == 0 && x + width > contentRight)
            {
                width = Math.Max(S(40), contentRight - x);
            }

            _tabBounds.Add((_items[i].Id, new RectangleF(x, 2, width, Height - 4)));
            x += width + gap;
        }

        _visibleCount = _tabBounds.Count;
    }

    private void EnsureSelectedVisible()
    {
        if (_items.Count == 0)
        {
            _visibleStart = 0;
            return;
        }

        var selectedIndex = _selectedId is { } id ? _items.FindIndex(item => item.Id == id) : 0;
        if (selectedIndex < 0)
        {
            selectedIndex = 0;
        }

        _visibleStart = Math.Clamp(selectedIndex, 0, Math.Max(0, _items.Count - 1));
    }

    private float MeasureTab(TabStripItem item)
    {
        var textWidth = TextRenderer.MeasureText(
            item.Label,
            _labelFont,
            new Size(int.MaxValue, int.MaxValue),
            TextFormatFlags.NoPrefix).Width;
        return Math.Clamp(textWidth + S(32), S(58), S(140));
    }

    private int HitTest(Point point, out int arrow)
    {
        arrow = 0;
        if (_overflow)
        {
            if (_leftArrow.Contains(point))
            {
                arrow = -1;
                return -1;
            }

            if (_rightArrow.Contains(point))
            {
                arrow = 1;
                return -1;
            }
        }

        foreach (var (id, bounds) in _tabBounds)
        {
            if (bounds.Contains(point))
            {
                return _items.FindIndex(item => item.Id == id);
            }
        }

        return -1;
    }

    private void DrawArrow(Graphics graphics, RectangleF bounds, string glyph, bool enabled, bool hovered)
    {
        if (hovered && enabled)
        {
            Drawing.FillRounded(graphics, Theme.Hover, bounds, S(6));
        }

        TextRenderer.DrawText(
            graphics,
            glyph,
            _arrowFont,
            Rectangle.Round(bounds),
            enabled ? Theme.SecondaryText : Theme.MutedText,
            TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter | TextFormatFlags.NoPrefix);
    }

    private int S(int value) => (int)Math.Round(value * DeviceDpi / 96.0);

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            _labelFont.Dispose();
            _arrowFont.Dispose();
            _toolTip.Dispose();
        }

        base.Dispose(disposing);
    }
}
