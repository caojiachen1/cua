using System;
using System.Drawing;
using System.Globalization;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace CuaSessionHost
{
    internal sealed class SessionRdp : AxHost
    {
        // MsRdpClient10 (non-scriptable). Ray-Desktop and DesktopClone both use
        // Windows' in-box RDP ActiveX; no COM interop assembly is required.
        private const string Clsid = "A0C63C30-F08D-4AB4-907C-34905D770C7D";

        public SessionRdp() : base(Clsid)
        {
            Dock = DockStyle.Fill;
        }

        public object Ocx
        {
            get
            {
                if (!IsHandleCreated)
                {
                    IntPtr force = Handle;
                    GC.KeepAlive(force);
                }
                return GetOcx();
            }
        }

        public int Connected
        {
            get
            {
                try
                {
                    object client = Ocx;
                    return Convert.ToInt32(ComGet(client, "Connected"), CultureInfo.InvariantCulture);
                }
                catch
                {
                    return 0;
                }
            }
        }

        public void Connect(Size size, int scalePercent, ChildCredential credential)
        {
            object client = Ocx;
            if (client == null) throw new InvalidOperationException("RDP ActiveX did not initialize");

            ComSet(client, "Server", "localhost");
            ComSet(client, "DesktopWidth", Even(Math.Max(640, Math.Min(8192, size.Width))));
            ComSet(client, "DesktopHeight", Even(Math.Max(480, Math.Min(8192, size.Height))));
            ComSet(client, "ColorDepth", 32);
            ComSet(client, "ConnectingText", "creating child session...");
            ComSet(client, "DisconnectedText", "child session disconnected");

            object adv = ComGet(client, "AdvancedSettings7");
            if (adv != null)
            {
                SetIfSupported(adv, "RDPPort", ChildSessionApi.RdpPort());
                SetIfSupported(adv, "EnableCredSspSupport", true);
                SetIfSupported(adv, "AuthenticationLevel", 0);
                SetIfSupported(adv, "SmartSizing", true);
                SetIfSupported(adv, "RedirectClipboard", true);
                SetIfSupported(adv, "RedirectDrives", false);
                SetIfSupported(adv, "RedirectPrinters", false);
                SetIfSupported(adv, "RedirectPorts", false);
                SetIfSupported(adv, "RedirectSmartCards", false);
                SetIfSupported(adv, "EnableWindowsKey", 1);
                SetIfSupported(adv, "DisplayConnectionBar", false);
                SetIfSupported(adv, "PinConnectionBar", false);
                SetIfSupported(adv, "GrabFocusOnConnect", false);
                SetIfSupported(adv, "EnableAutoReconnect", true);
                SetIfSupported(adv, "MaxReconnectAttempts", 8);
            }

            object secured = ComGet(client, "SecuredSettings2");
            if (secured != null)
            {
                SetIfSupported(secured, "KeyboardHookMode", 1);
                SetIfSupported(secured, "AudioRedirectionMode", 2);
            }

            var extended = client as IMsRdpExtendedSettings;
            if (extended == null)
                throw new COMException("RDP ActiveX does not expose IMsRdpExtendedSettings");

            TrySetExtended(extended, "DesktopScaleFactor", (uint)Math.Max(100, Math.Min(500, scalePercent)));
            TrySetExtended(extended, "DeviceScaleFactor", 100u);
            object enabled = true;
            extended.set_Property("ConnectToChildSession", ref enabled);

            if (credential != null)
            {
                string domain;
                string user;
                credential.RdpIdentity(out domain, out user);
                ComSet(client, "Domain", domain);
                ComSet(client, "UserName", user);
                ((IMsRdpClientNonScriptable)client).put_ClearTextPassword(credential.Password);
            }

            InvokeCom(client, "Connect");
        }

        public void Disconnect()
        {
            try
            {
                if (Connected != 0) InvokeCom(Ocx, "Disconnect");
            }
            catch { }
        }

        public void SetInputBlocked(bool blocked)
        {
            if (IsHandleCreated) ChildSessionApi.SetInputBlocked(Handle, blocked);
        }

        public bool FocusInput()
        {
            return IsHandleCreated && ChildSessionApi.FocusInput(Handle);
        }

        public bool IsInputFocused()
        {
            return IsHandleCreated && ChildSessionApi.IsInputFocused(Handle);
        }

        protected override void OnHandleDestroyed(EventArgs e)
        {
            ChildSessionApi.ClearCache(Handle);
            base.OnHandleDestroyed(e);
        }

        private static int Even(int value)
        {
            return (value & 1) == 0 ? value : value - 1;
        }

        private static object ComGet(object target, string name)
        {
            return target.GetType().InvokeMember(name, BindingFlags.GetProperty, null, target, null,
                CultureInfo.InvariantCulture);
        }

        private static void ComSet(object target, string name, object value)
        {
            target.GetType().InvokeMember(name, BindingFlags.SetProperty, null, target,
                new[] { value }, CultureInfo.InvariantCulture);
        }

        private static object InvokeCom(object target, string name, params object[] args)
        {
            return target.GetType().InvokeMember(name, BindingFlags.InvokeMethod, null, target, args,
                CultureInfo.InvariantCulture);
        }

        private static void SetIfSupported(object target, string name, object value)
        {
            try { ComSet(target, name, value); }
            catch (Exception ex) when (ex is COMException || ex is TargetInvocationException) { }
        }

        private static void TrySetExtended(IMsRdpExtendedSettings settings, string name, object value)
        {
            try { settings.set_Property(name, ref value); }
            catch (Exception ex) when (ex is COMException || ex is TargetInvocationException) { }
        }

        [ComImport]
        [Guid("302D8188-0052-4807-806A-362B628F9AC5")]
        [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
        private interface IMsRdpExtendedSettings
        {
            void set_Property(
                [In, MarshalAs(UnmanagedType.BStr)] string propertyName,
                [In, MarshalAs(UnmanagedType.Struct)] ref object value);

            [return: MarshalAs(UnmanagedType.Struct)]
            object get_Property([In, MarshalAs(UnmanagedType.BStr)] string propertyName);
        }

        // Trimmed to the write-only ClearTextPassword member. The password is only
        // handed to the local RDP control and is never written to cua state or logs.
        [ComImport]
        [Guid("2F079C4C-87B2-4AFD-97AB-20CDB43038AE")]
        [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
        private interface IMsRdpClientNonScriptable
        {
            void put_ClearTextPassword([In, MarshalAs(UnmanagedType.BStr)] string value);
        }
    }
}
