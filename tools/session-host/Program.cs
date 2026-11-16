using System;
using System.IO;
using System.Windows.Forms;

namespace CuaSessionHost
{
    internal sealed class Options
    {
        public bool Setup;
        public bool EnableRdp;
        public bool Preview;
        public bool View;
        public bool Interactive;
        public bool Login;
        public bool Logout;
        public bool KeepStartup;
        public bool RestoreStartup;
        public int Width;
        public int Height;
        public bool ExplicitSize;
        public string CuaExe;
        public string DataDir;
        public string HostPipe = "cua-session-host";

        public static Options Parse(string[] args)
        {
            var o = new Options();
            string local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            o.DataDir = Path.Combine(local, "cua");
            string envDir = Environment.GetEnvironmentVariable("CUA_DATA_DIR");
            if (!string.IsNullOrWhiteSpace(envDir)) o.DataDir = envDir;
            o.CuaExe = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "cua.exe");

            for (int i = 0; i < args.Length; i++)
            {
                string a = args[i];
                switch (a)
                {
                    case "--setup": o.Setup = true; break;
                    case "--enable-rdp": o.EnableRdp = true; break;
                    case "--preview": o.Preview = true; o.View = true; break;
                    case "--start": break;
                    case "--view": o.View = true; break;
                    case "--interactive": o.Interactive = true; o.View = true; break;
                    case "--login": o.Login = true; break;
                    case "--logout": o.Logout = true; break;
                    case "--keep-startup": o.KeepStartup = true; break;
                    case "--restore-startup": o.RestoreStartup = true; break;
                    case "--width": o.Width = ParseInt(Next(args, ref i), o.Width); o.ExplicitSize = true; break;
                    case "--height": o.Height = ParseInt(Next(args, ref i), o.Height); o.ExplicitSize = true; break;
                    case "--cua-exe": o.CuaExe = Next(args, ref i); break;
                    case "--data-dir": o.DataDir = Next(args, ref i); break;
                    case "--pipe": o.HostPipe = Next(args, ref i); break;
                    default:
                        throw new ArgumentException("unknown argument: " + a);
                }
            }

            if (o.Width != 0 && (o.Width < 640 || o.Width > 8192))
                throw new ArgumentException("--width must be 640..8192");
            if (o.Height != 0 && (o.Height < 480 || o.Height > 8192))
                throw new ArgumentException("--height must be 480..8192");
            if (o.Width == 0 || o.Height == 0) o.ExplicitSize = false;
            if (string.IsNullOrWhiteSpace(o.CuaExe)) throw new ArgumentException("--cua-exe is empty");
            if (string.IsNullOrWhiteSpace(o.DataDir)) throw new ArgumentException("--data-dir is empty");
            Directory.CreateDirectory(o.DataDir);
            return o;
        }

        private static string Next(string[] args, ref int i)
        {
            if (i + 1 >= args.Length) throw new ArgumentException("missing value for " + args[i]);
            return args[++i];
        }

        private static int ParseInt(string s, int fallback)
        {
            int v;
            return int.TryParse(s, out v) ? v : fallback;
        }
    }

    internal static class Program
    {
        [STAThread]
        private static int Main(string[] args)
        {
            Options opt = null;
            try
            {
                opt = Options.Parse(args);
                if (opt.RestoreStartup) return StartupQuarantine.RestoreAfterDelay(opt.DataDir, 90);
                Native.SetProcessDpiAwarenessContext(new IntPtr(-4));
                Application.EnableVisualStyles();
                Application.SetCompatibleTextRenderingDefault(false);

                if (opt.Login) return CredentialDialog.Run();
                if (opt.Logout)
                {
                    CredentialStore.Delete();
                    return 0;
                }
                if (opt.Setup) return Setup.Run(opt);

                using (var form = new HostForm(opt))
                {
                    Application.Run(form);
                }
                return 0;
            }
            catch (Exception ex)
            {
                string dir = opt != null ? opt.DataDir : Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "cua");
                try
                {
                    Directory.CreateDirectory(dir);
                    File.WriteAllText(Path.Combine(dir, "session-error.json"),
                        Json.Serialize(new System.Collections.Generic.Dictionary<string, object>
                        {
                            { "ok", false },
                            { "error", ex.GetBaseException().Message },
                            { "kind", "host" }
                        }));
                }
                catch { }
                if (opt == null || opt.View || opt.Setup || opt.Login || opt.Logout)
                {
                    MessageBox.Show(ex.GetBaseException().Message, "cua child session",
                        MessageBoxButtons.OK, MessageBoxIcon.Error);
                }
                return 1;
            }
        }
    }
}
