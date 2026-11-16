using System;
using System.Drawing;
using System.IO;
using System.Reflection;
using System.Windows.Forms;

namespace CuaSessionHost
{
    internal static class Theme
    {
        public static readonly Color Window = Color.FromArgb(32, 32, 32);
        public static readonly Color Bar = Color.FromArgb(28, 28, 30);
        public static readonly Color BarTop = Color.FromArgb(34, 34, 37);
        public static readonly Color Border = Color.FromArgb(58, 58, 64);
        public static readonly Color Text = Color.FromArgb(243, 243, 243);
        public static readonly Color Muted = Color.FromArgb(166, 166, 172);
        public static readonly Color Subtle = Color.FromArgb(45, 45, 50);
        public static readonly Color SubtleHover = Color.FromArgb(58, 58, 64);
        public static readonly Color Accent = Color.FromArgb(96, 205, 255);
        public static readonly Color AccentHover = Color.FromArgb(128, 218, 255);
        public static readonly Color AccentText = Color.FromArgb(16, 26, 34);
        public static readonly Color Warn = Color.FromArgb(255, 185, 0);
        public static readonly Color Field = Color.FromArgb(43, 43, 47);

        public static Font Ui(float size, FontStyle style = FontStyle.Regular)
        {
            return new Font("Segoe UI Variable Text", size, style, GraphicsUnit.Point);
        }

        public static Font Title(float size)
        {
            return new Font("Segoe UI Variable Display", size, FontStyle.Bold, GraphicsUnit.Point);
        }

        public static Font Icon(float size)
        {
            return new Font("Segoe Fluent Icons", size, FontStyle.Regular, GraphicsUnit.Point);
        }

        /// <summary>
        /// Tray glyph: a bright accent square with a white prompt mark. The
        /// application icon is deliberately dark and disappeared into a dark
        /// taskbar (measured: barely any bright pixels in its 25x25 slot), and
        /// the tray icon is the "session is alive" indicator, so it has to be
        /// unmistakable in both themes.
        /// </summary>
        public static System.Drawing.Icon TrayIcon()
        {
            int s = Math.Max(16, SystemInformation.SmallIconSize.Width);
            using (var bmp = new Bitmap(s, s))
            using (var g = Graphics.FromImage(bmp))
            {
                g.SmoothingMode = System.Drawing.Drawing2D.SmoothingMode.AntiAlias;
                g.Clear(Color.Transparent);
                float r = s * 0.22f;
                using (var path = RoundedRect(new RectangleF(0.5f, 0.5f, s - 1f, s - 1f), r))
                using (var brush = new SolidBrush(Color.FromArgb(47, 155, 255)))
                {
                    g.FillPath(brush, path);
                }
                using (var pen = new Pen(Color.White, Math.Max(2f, s * 0.11f)))
                {
                    pen.StartCap = System.Drawing.Drawing2D.LineCap.Round;
                    pen.EndCap = System.Drawing.Drawing2D.LineCap.Round;
                    pen.LineJoin = System.Drawing.Drawing2D.LineJoin.Round;
                    g.DrawLines(pen, new[]
                    {
                        new PointF(s * 0.30f, s * 0.31f),
                        new PointF(s * 0.47f, s * 0.50f),
                        new PointF(s * 0.30f, s * 0.69f),
                    });
                    g.DrawLine(pen, s * 0.55f, s * 0.69f, s * 0.72f, s * 0.69f);
                }
                return System.Drawing.Icon.FromHandle(bmp.GetHicon());
            }
        }

        private static System.Drawing.Drawing2D.GraphicsPath RoundedRect(RectangleF rect, float radius)
        {
            var path = new System.Drawing.Drawing2D.GraphicsPath();
            float d = radius * 2;
            path.AddArc(rect.X, rect.Y, d, d, 180, 90);
            path.AddArc(rect.Right - d, rect.Y, d, d, 270, 90);
            path.AddArc(rect.Right - d, rect.Bottom - d, d, d, 0, 90);
            path.AddArc(rect.X, rect.Bottom - d, d, d, 90, 90);
            path.CloseFigure();
            return path;
        }

        public static System.Drawing.Icon AppIcon(bool small)
        {
            try
            {
                Assembly assembly = typeof(Theme).Assembly;
                using (Stream stream = assembly.GetManifestResourceStream("CuaSessionHost.cua-session.ico"))
                {
                    if (stream != null)
                        return new System.Drawing.Icon(stream, small ? SystemInformation.SmallIconSize : SystemInformation.IconSize);
                }
            }
            catch { }
            try
            {
                System.Drawing.Icon extracted = System.Drawing.Icon.ExtractAssociatedIcon(Application.ExecutablePath);
                if (extracted != null) return extracted;
            }
            catch { }
            return SystemIcons.Application;
        }
    }
}
