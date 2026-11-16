using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32;

namespace CuaSessionHost
{
    internal static class Setup
    {
        public static int Run(Options opt)
        {
            var result = new Dictionary<string, object>();
            try
            {
                bool wasEnabled = false;
                try { wasEnabled = ChildSessionApi.IsEnabled(); } catch { }
                if (!wasEnabled) ChildSessionApi.Enable();
                if (opt.EnableRdp) EnableRemoteDesktop();
                bool enabled = ChildSessionApi.IsEnabled();
                bool fromPolicy;
                bool rdpDisabled = ChildSessionApi.IsRemoteDesktopDisabled(out fromPolicy);
                bool rdpWrapper = ChildSessionApi.IsRdpWrapperInstalled();

                result["ok"] = enabled && !rdpDisabled && !rdpWrapper;
                result["schema"] = "cua-session-setup-v1";
                result["enabled"] = enabled;
                result["changed"] = !wasEnabled;
                result["needs_signout"] = !wasEnabled;
                result["remote_desktop_disabled"] = rdpDisabled;
                result["rdp_enable_requested"] = opt.EnableRdp;
                result["remote_desktop_policy"] = fromPolicy;
                result["rdp_wrapper"] = rdpWrapper;
                result["message"] = !enabled
                    ? "Child Sessions could not be enabled."
                    : rdpDisabled
                        ? "Child Sessions enabled, but Remote Desktop host is disabled; enable it in Settings."
                        : rdpWrapper
                            ? "Child Sessions enabled, but RDP Wrapper is installed and must be removed."
                            : !wasEnabled
                                ? "Child Sessions enabled. Sign out of Windows and sign back in before starting the session."
                                : "Child Sessions are already enabled.";
                Write(opt, result);
                return (bool)result["ok"] ? 0 : 1;
            }
            catch (Exception ex)
            {
                result["ok"] = false;
                result["schema"] = "cua-session-setup-v1";
                result["error"] = ex.GetBaseException().Message;
                if (ex is Win32Exception)
                    result["win32_error"] = ((Win32Exception)ex).NativeErrorCode;
                Write(opt, result);
                return 1;
            }
        }

        private static void EnableRemoteDesktop()
        {
            // Explicit opt-in only. This is a security-relevant change; the CLI
            // exposes it as `cua session setup --enable-rdp`.
            using (var key = Registry.LocalMachine.OpenSubKey(
                       @"SYSTEM\CurrentControlSet\Control\Terminal Server", true))
            {
                if (key == null) throw new InvalidOperationException("Terminal Server key not found");
                key.SetValue("fDenyTSConnections", 0, RegistryValueKind.DWord);
            }
            using (var key = Registry.LocalMachine.OpenSubKey(
                       @"SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services", true))
            {
                if (key != null) key.SetValue("fDenyTSConnections", 0, RegistryValueKind.DWord);
            }
        }

        private static void Write(Options opt, Dictionary<string, object> result)
        {
            Directory.CreateDirectory(opt.DataDir);
            File.WriteAllText(Path.Combine(opt.DataDir, "session-setup.json"), Json.Serialize(result));
        }
    }
}
