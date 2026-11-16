using System;
using System.Drawing;
using System.Runtime.InteropServices;
using System.Text;

namespace CuaSessionHost
{
    internal static class Native
    {
        [DllImport("wtsapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool WTSEnableChildSessions([MarshalAs(UnmanagedType.Bool)] bool enable);

        [DllImport("wtsapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool WTSIsChildSessionsEnabled([MarshalAs(UnmanagedType.Bool)] out bool enabled);

        [DllImport("wtsapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool WTSGetChildSessionId(out uint sessionId);

        [DllImport("wtsapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool WTSLogoffSession(
            IntPtr serverHandle, uint sessionId, [MarshalAs(UnmanagedType.Bool)] bool wait);

        [StructLayout(LayoutKind.Sequential)]
        public struct WTS_SESSION_INFO
        {
            public uint SessionId;
            public IntPtr WinStationName;
            public int State;
        }

        [DllImport("wtsapi32.dll", SetLastError = true, CharSet = CharSet.Unicode, EntryPoint = "WTSEnumerateSessionsW")]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool WTSEnumerateSessions(
            IntPtr serverHandle, int reserved, int version,
            out IntPtr sessionInfo, out int count);

        [DllImport("wtsapi32.dll")]
        public static extern void WTSFreeMemory(IntPtr memory);

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool EnumChildWindows(
            IntPtr parentWindow, EnumChildProc callback, IntPtr parameter);

        public delegate bool EnumChildProc(IntPtr window, IntPtr parameter);

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        public static extern int GetWindowText(IntPtr window, StringBuilder text, int maximumLength);

        [DllImport("user32.dll")]
        public static extern IntPtr SetFocus(IntPtr window);

        [DllImport("user32.dll")]
        public static extern IntPtr GetFocus();

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool IsWindow(IntPtr window);

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool IsChild(IntPtr parentWindow, IntPtr window);

        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool EnableWindow(IntPtr window, [MarshalAs(UnmanagedType.Bool)] bool enable);

        [DllImport("user32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool SetProcessDpiAwarenessContext(IntPtr context);

        [DllImport("user32.dll")]
        public static extern uint GetDpiForWindow(IntPtr window);

        [DllImport("dwmapi.dll")]
        private static extern int DwmSetWindowAttribute(IntPtr window, int attribute, ref int value, int size);

        public static void ApplyDarkWindow(IntPtr window)
        {
            if (window == IntPtr.Zero) return;
            int dark = 1;
            TryDwm(window, 20, ref dark); // DWMWA_USE_IMMERSIVE_DARK_MODE
            int rounded = 2;
            TryDwm(window, 33, ref rounded); // DWMWA_WINDOW_CORNER_PREFERENCE
            int acrylic = 3;
            TryDwm(window, 38, ref acrylic); // DWMWA_SYSTEMBACKDROP_TYPE: transient/acrylic
            int caption = ColorTranslator.ToWin32(Theme.Bar);
            TryDwm(window, 35, ref caption); // DWMWA_CAPTION_COLOR
            int captionText = ColorTranslator.ToWin32(Theme.Text);
            TryDwm(window, 36, ref captionText); // DWMWA_TEXT_COLOR
            int border = ColorTranslator.ToWin32(Theme.Border);
            TryDwm(window, 34, ref border); // DWMWA_BORDER_COLOR
        }

        private static void TryDwm(IntPtr window, int attribute, ref int value)
        {
            try { DwmSetWindowAttribute(window, attribute, ref value, sizeof(int)); }
            catch { }
        }
    }
}
