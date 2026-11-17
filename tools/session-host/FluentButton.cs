using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Windows.Forms;

namespace CuaSessionHost
{
    internal sealed class FluentButton : Control
    {
        private bool _hover;
        private bool _pressed;

        public FluentButton()
        {
            SetStyle(ControlStyles.UserPaint | ControlStyles.AllPaintingInWmPaint |
                     ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw |
                     ControlStyles.SupportsTransparentBackColor, true);
            SetStyle(ControlStyles.StandardClick | ControlStyles.StandardDoubleClick, false);
            BackColor = Color.Transparent;
            ForeColor = Theme.Text;
            Font = Theme.Ui(9.5f, FontStyle.Bold);
            Cursor = Cursors.Hand;
            TabStop = false;
            Size = new Size(104, 32);
            Accent = false;
        }

        public bool Accent { get; set; }

        /// 无边框无背景，仅绘制内容；悬停时淡高亮（标题栏图标钮用）
        public bool Ghost { get; set; }

        public bool Danger { get; set; }

        public int CornerRadius { get; set; } = 8;

        protected override void OnPaint(PaintEventArgs e)
        {
            e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
            e.Graphics.Clear(Parent == null ? Theme.Bar : Parent.BackColor);

            Color fill;
            Color border;
            Color text;
            if (!Enabled)
            {
                fill = Theme.Subtle;
                border = Theme.Border;
                text = Theme.Muted;
            }
            else if (Ghost)
            {
                // Fluent 标题栏图标：常态纯图标，悬停淡底，无外框
                fill = _pressed ? Color.FromArgb(52, 52, 58) : _hover ? Theme.SubtleHover : Color.Transparent;
                border = Color.Transparent;
                text = ForeColor;
            }
            else if (Danger && _hover)
            {
                fill = _pressed ? Color.FromArgb(150, 30, 24) : Color.FromArgb(196, 43, 28);
                border = fill;
                text = Color.White;
            }
            else if (Accent)
            {
                fill = _pressed ? Color.FromArgb(70, 180, 230) : _hover ? Theme.AccentHover : Theme.Accent;
                border = fill;
                text = Theme.AccentText;
            }
            else
            {
                fill = _pressed ? Color.FromArgb(36, 36, 40) : _hover ? Theme.SubtleHover : Theme.Subtle;
                border = _hover ? Color.FromArgb(78, 78, 86) : Theme.Border;
                text = ForeColor;
            }

            Rectangle r = new Rectangle(0, 0, Width - 1, Height - 1);
            using (GraphicsPath path = RoundedRect(r, CornerRadius))
            using (var brush = new SolidBrush(fill))
            using (var pen = new Pen(border))
            {
                if (fill.A > 0)
                {
                    e.Graphics.FillPath(brush, path);
                }
                if (border.A > 0)
                {
                    e.Graphics.DrawPath(pen, path);
                }
            }

            TextRenderer.DrawText(
                e.Graphics,
                Text,
                Font,
                r,
                text,
                TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter |
                TextFormatFlags.SingleLine | TextFormatFlags.EndEllipsis | TextFormatFlags.NoPadding);
        }

        protected override void OnMouseEnter(EventArgs e)
        {
            _hover = true;
            Invalidate();
            base.OnMouseEnter(e);
        }

        protected override void OnMouseLeave(EventArgs e)
        {
            _hover = false;
            _pressed = false;
            Invalidate();
            base.OnMouseLeave(e);
        }

        protected override void OnMouseDown(MouseEventArgs e)
        {
            if (e.Button == MouseButtons.Left)
            {
                _pressed = true;
                Capture = true;
                Invalidate();
            }
            base.OnMouseDown(e);
        }

        protected override void OnMouseUp(MouseEventArgs e)
        {
            if (e.Button == MouseButtons.Left)
            {
                bool clicked = _pressed && ClientRectangle.Contains(e.Location);
                _pressed = false;
                Capture = false;
                Invalidate();
                if (clicked) OnClick(EventArgs.Empty);
            }
            base.OnMouseUp(e);
        }

        protected override void OnEnabledChanged(EventArgs e)
        {
            Invalidate();
            base.OnEnabledChanged(e);
        }

        internal static GraphicsPath RoundedRect(Rectangle bounds, int radius)
        {
            var path = new GraphicsPath();
            int d = Math.Max(1, radius * 2);
            d = Math.Min(d, Math.Min(bounds.Width, bounds.Height));
            var arc = new Rectangle(bounds.X, bounds.Y, d, d);
            path.AddArc(arc, 180, 90);
            arc.X = bounds.Right - d;
            path.AddArc(arc, 270, 90);
            arc.Y = bounds.Bottom - d;
            path.AddArc(arc, 0, 90);
            arc.X = bounds.Left;
            path.AddArc(arc, 90, 90);
            path.CloseFigure();
            return path;
        }
    }

    internal sealed class StatusPill : Control
    {
        private Color _dot = Theme.Warn;

        public StatusPill()
        {
            SetStyle(ControlStyles.UserPaint | ControlStyles.AllPaintingInWmPaint |
                     ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw |
                     ControlStyles.SupportsTransparentBackColor, true);
            SetStyle(ControlStyles.StandardClick | ControlStyles.StandardDoubleClick, false);
            BackColor = Color.Transparent;
            ForeColor = Theme.Text;
            Font = Theme.Ui(9f, FontStyle.Bold);
            Height = 28;
            Width = 150;
        }

        public void SetState(string text, Color dot)
        {
            Text = text;
            _dot = dot;
            RefreshSize();
            Invalidate();
        }

        public void RefreshSize()
        {
            Width = Math.Max(120, TextRenderer.MeasureText(Text ?? "", Font).Width + 42);
        }

        protected override void OnPaint(PaintEventArgs e)
        {
            e.Graphics.SmoothingMode = SmoothingMode.AntiAlias;
            e.Graphics.Clear(Parent == null ? Theme.Bar : Parent.BackColor);
            Rectangle r = new Rectangle(0, 0, Width - 1, Height - 1);
            using (GraphicsPath path = FluentButton.RoundedRect(r, Height / 2))
            using (var brush = new SolidBrush(Color.FromArgb(26, 255, 255, 255)))
            using (var pen = new Pen(Color.FromArgb(52, 255, 255, 255)))
            {
                e.Graphics.FillPath(brush, path);
                e.Graphics.DrawPath(pen, path);
            }

            int cy = Height / 2;
            using (var brush = new SolidBrush(_dot))
                e.Graphics.FillEllipse(brush, 12, cy - 4, 8, 8);

            TextRenderer.DrawText(
                e.Graphics,
                Text,
                Font,
                new Rectangle(28, 0, Width - 34, Height),
                ForeColor,
                TextFormatFlags.Left | TextFormatFlags.VerticalCenter |
                TextFormatFlags.SingleLine | TextFormatFlags.EndEllipsis | TextFormatFlags.NoPadding);
        }
    }
}
