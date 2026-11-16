using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32;

namespace CuaSessionHost
{
    internal static class ChildSessionApi
    {
        private const uint NoChildSessionId = uint.MaxValue;
        private const string RdpTcpKey =
            @"SYSTEM\CurrentControlSet\Control\Terminal Server\WinStations\RDP-Tcp";
        private const string TerminalServerKey =
            @"SYSTEM\CurrentControlSet\Control\Terminal Server";
        private const string TerminalServerPolicyKey =
            @"SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services";
        private const string TermServiceKey =
            @"SYSTEM\CurrentControlSet\Services\TermService\Parameters";

        private static IntPtr _cachedHost;
        private static IntPtr _cachedInput;

        public static bool IsEnabled()
        {
            bool enabled;
            if (!Native.WTSIsChildSessionsEnabled(out enabled))
                throw new Win32Exception(Marshal.GetLastWin32Error(), "WTSIsChildSessionsEnabled failed");
            return enabled;
        }

        public static void Enable()
        {
            if (!Native.WTSEnableChildSessions(true))
                throw new Win32Exception(Marshal.GetLastWin32Error(), "WTSEnableChildSessions failed");
        }

        public static uint? TryGetId()
        {
            uint id;
            if (!Native.WTSGetChildSessionId(out id) || id == NoChildSessionId) return null;
            return id;
        }

        public static void Logoff(uint id)
        {
            if (!Native.WTSLogoffSession(IntPtr.Zero, id, true))
                throw new Win32Exception(Marshal.GetLastWin32Error(), "WTSLogoffSession failed");
        }

        public static bool IsRdpWrapperInstalled()
        {
            try
            {
                using (var baseKey = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64))
                using (var key = baseKey.OpenSubKey(TermServiceKey))
                {
                    string dll = key == null ? null : key.GetValue("ServiceDll") as string;
                    return dll != null && dll.IndexOf("rdpwrap.dll", StringComparison.OrdinalIgnoreCase) >= 0;
                }
            }
            catch (Exception ex) when (ex is IOException || ex is UnauthorizedAccessException)
            {
                return false;
            }
        }

        public static bool IsRemoteDesktopDisabled(out bool fromPolicy)
        {
            int? policy = ReadDword(TerminalServerPolicyKey, "fDenyTSConnections");
            if (policy.HasValue)
            {
                fromPolicy = true;
                return policy.Value == 1;
            }
            fromPolicy = false;
            return ReadDword(TerminalServerKey, "fDenyTSConnections") == 1;
        }

        public static int RdpPort()
        {
            int? port = ReadDword(RdpTcpKey, "PortNumber");
            return port.HasValue && port.Value > 0 && port.Value <= 65535 ? port.Value : 3389;
        }

        private static int? ReadDword(string path, string name)
        {
            try
            {
                using (var baseKey = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64))
                using (var key = baseKey.OpenSubKey(path))
                {
                    object v = key == null ? null : key.GetValue(name);
                    if (v == null) return null;
                    return Convert.ToInt32(v);
                }
            }
            catch (Exception ex) when (ex is IOException || ex is UnauthorizedAccessException)
            {
                return null;
            }
        }

        public static void SetInputBlocked(IntPtr host, bool blocked)
        {
            IntPtr input = FindInput(host);
            if (input != IntPtr.Zero) Native.EnableWindow(input, !blocked);
        }

        public static bool FocusInput(IntPtr host)
        {
            IntPtr input = FindInput(host);
            if (input == IntPtr.Zero) return false;
            Native.SetFocus(input);
            return Native.GetFocus() == input;
        }

        public static bool IsInputFocused(IntPtr host)
        {
            IntPtr input = FindInput(host);
            return input != IntPtr.Zero && Native.GetFocus() == input;
        }

        public static void ClearCache(IntPtr host)
        {
            if (_cachedHost == host)
            {
                _cachedHost = IntPtr.Zero;
                _cachedInput = IntPtr.Zero;
            }
        }

        private static IntPtr FindInput(IntPtr host)
        {
            if (host == IntPtr.Zero) return IntPtr.Zero;
            if (_cachedHost == host && Native.IsWindow(_cachedInput) && Native.IsChild(host, _cachedInput))
                return _cachedInput;

            IntPtr found = IntPtr.Zero;
            Native.EnumChildWindows(host, delegate(IntPtr window, IntPtr ignored)
            {
                var text = new StringBuilder(256);
                Native.GetWindowText(window, text, text.Capacity);
                if (string.Equals(text.ToString(), "Input Capture Window", StringComparison.Ordinal))
                {
                    found = window;
                    return false;
                }
                return true;
            }, IntPtr.Zero);

            _cachedHost = host;
            _cachedInput = found;
            return found;
        }
    }
}
