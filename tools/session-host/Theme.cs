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
