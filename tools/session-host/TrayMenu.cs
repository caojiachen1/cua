using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Windows.Forms;

namespace CuaSessionHost
{
    /// <summary>
    /// Dark renderer for the tray context menu: flat near-black panel, rounded
    /// corners, soft grey hover with an accent edge, muted separators. The stock
    /// professional renderer draws light gradients and hard corners that look
    /// wrong next to the dark viewer.
    /// </summary>
    internal sealed class TrayMenuRenderer : ToolStripProfessionalRenderer
    {
        public TrayMenuRenderer() : base(new Table())
        {
            RoundedEdges = false;
        }

        protected override void OnRenderToolStripBackground(ToolStripRenderEventArgs e)
        {
            using (var brush = new SolidBrush(Theme.Bar))
                e.Graphics.FillRectangle(brush, e.AffectedBounds);
        }

        protected override void OnRenderToolStripBorder(ToolStripRenderEventArgs e)
        {
            var bounds = new Rectangle(0, 0, e.ToolStrip.Width - 1, e.ToolStrip.Height - 1);
            using (var path = Rounded(bounds, 8))
            using (var pen = new Pen(Theme.Border))
                e.Graphics.DrawPath(pen, path);
        }

        protected override void OnRenderMenuItemBackground(ToolStripItemRenderEventArgs e)
        {
            if (!e.Item.Selected || !e.Item.Enabled) return;
            var rect = new Rectangle(5, 2, e.Item.Width - 10, e.Item.Height - 4);
            using (var path = Rounded(rect, 5))
            using (var brush = new SolidBrush(Theme.SubtleHover))
                e.Graphics.FillPath(brush, path);
        }

        protected override void OnRenderItemText(ToolStripItemTextRenderEventArgs e)
        {
            var color = e.Item.Enabled ? Theme.Text : Color.FromArgb(118, 118, 124);
            if (e.TextDirection != ToolStripTextDirection.Horizontal)
            {
                e.TextColor = color;
                base.OnRenderItemText(e);
                return;
            }
            var font = e.TextFont ?? e.Item.Font;
            var size = TextRenderer.MeasureText(e.Graphics, e.Text, font,
                new Size(int.MaxValue, int.MaxValue), TextFormatFlags.NoPadding);
            // Centre on the drop-down's visible width; Item.Width is wider than
            // the panel (it includes the reserved image-margin column), which is
            // what pushed earlier attempts to the right.
            int x = Math.Max(0, (e.ToolStrip.Width - size.Width) / 2);
            // centre on the item row itself: TextRectangle is the text box, not
            // the row, and using it left the labels ~7px high.
            // Item-local: the renderer's origin is this item's top-left, so
            // Bounds.Y would push every row below the first one out of view.
            // -2: TextRenderer's glyph box sits slightly low (measured 8/5).
            int y = Math.Max(0, (e.Item.Height - size.Height) / 2) - 2;
            TextRenderer.DrawText(e.Graphics, e.Text, font, new Point(x, y), color,
                TextFormatFlags.NoPrefix | TextFormatFlags.NoPadding);
        }

        protected override void OnRenderSeparator(ToolStripSeparatorRenderEventArgs e)
        {
            int y = e.Item.Height / 2 + 1;
            using (var pen = new Pen(Color.FromArgb(50, 50, 56)))
                e.Graphics.DrawLine(pen, 12, y, e.Item.Width - 12, y);
        }

        /// <summary>Clips a drop-down to rounded corners; call after it opened.</summary>
        public static void RoundCorners(Control menu, int radius)
        {
            if (menu.Width <= 0 || menu.Height <= 0) return;
            using (var path = Rounded(new Rectangle(0, 0, menu.Width, menu.Height), radius))
                menu.Region = new Region(path);
        }

        private static GraphicsPath Rounded(Rectangle rect, int radius)
        {
            var path = new GraphicsPath();
            int d = radius * 2;
            path.AddArc(rect.X, rect.Y, d, d, 180, 90);
            path.AddArc(rect.Right - d, rect.Y, d, d, 270, 90);
            path.AddArc(rect.Right - d, rect.Bottom - d, d, d, 0, 90);
            path.AddArc(rect.X, rect.Bottom - d, d, d, 90, 90);
            path.CloseFigure();
            return path;
        }

        private sealed class Table : ProfessionalColorTable
        {
            public override Color ToolStripDropDownBackground => Theme.Bar;
            public override Color MenuBorder => Theme.Border;
            public override Color MenuItemBorder => Color.Transparent;
            public override Color MenuItemSelected => Theme.SubtleHover;
            public override Color MenuItemSelectedGradientBegin => Theme.SubtleHover;
            public override Color MenuItemSelectedGradientEnd => Theme.SubtleHover;
            public override Color MenuItemPressedGradientBegin => Theme.Subtle;
            public override Color MenuItemPressedGradientEnd => Theme.Subtle;
            public override Color ImageMarginGradientBegin => Theme.Bar;
            public override Color ImageMarginGradientMiddle => Theme.Bar;
            public override Color ImageMarginGradientEnd => Theme.Bar;
            public override Color SeparatorDark => Color.FromArgb(50, 50, 56);
            public override Color SeparatorLight => Color.FromArgb(50, 50, 56);
        }
    }
}
