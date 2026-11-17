using System;
using System.Diagnostics;
using System.IO;
using Microsoft.Win32;

namespace CuaSessionHost
{
    /// <summary>
    /// Main-session VNC: tvnserver captures THIS (main) session's desktop and
    /// serves it on 127.0.0.1:5900; a viewer running inside the child session
    /// lets the user watch and drive the main session without touching the
    /// physical console. Independent of the child-session routing flag.
    /// </summary>
    internal static class VncHelper
    {
        public const int DefaultPort = 5900;

        private static string ExeDir
        {
            get { return AppDomain.CurrentDomain.BaseDirectory; }
        }
        public static string ServerPath
        {
            get { return Path.Combine(ExeDir, "vnc", "tvnserver.exe"); }
        }
        public static string ViewerPath
        {
            get { return Path.Combine(ExeDir, "vnc", "tvnviewer.exe"); }
        }

        private static bool _configured;
        public static void ConfigureRegistry()
        {
            if (_configured) return;
            using (var key = Registry.CurrentUser.CreateSubKey(@"Software\TightVNC\Server"))
            {
                key.SetValue("AllowLoopback", 1, RegistryValueKind.DWord);
                key.SetValue("UseVncAuthentication", 0, RegistryValueKind.DWord);
                key.SetValue("LoopbackOnly", 1, RegistryValueKind.DWord);
                key.SetValue("PortNumber", DefaultPort, RegistryValueKind.DWord);
            }
            _configured = true;
        }

        public static bool ServerRunning
        {
            get { return Process.GetProcessesByName("tvnserver").Length > 0; }
        }

        public static void EnsureServer()
        {
            if (ServerRunning) return;
            if (!File.Exists(ServerPath)) throw new FileNotFoundException("tvnserver.exe 未找到", ServerPath);
            ConfigureRegistry();
            Process.Start(new ProcessStartInfo(ServerPath) { UseShellExecute = true });
        }

        public static void LaunchViewerHere()
        {
            if (!File.Exists(ViewerPath)) throw new FileNotFoundException("tvnviewer.exe 未找到", ViewerPath);
            Process.Start(new ProcessStartInfo(ViewerPath)
            {
                UseShellExecute = true,
                Arguments = "-host=127.0.0.1 -port=" + DefaultPort
            });
        }

        public static void LaunchViewerInChild(uint sessionId)
        {
            if (!File.Exists(ViewerPath)) throw new FileNotFoundException("tvnviewer.exe 未找到", ViewerPath);
            TaskLauncher.LaunchArgs(sessionId, ViewerPath,
                "-host=127.0.0.1 -port=" + DefaultPort, ExeDir);
        }

        /// 经 agent 管道在子会话内启动 tvnserver（抓子会话桌面）。
        /// agent 协议为"一连接一请求"，每次调用独立开管道。
        public static void EnsureServerInChild(string agentPipe)
        {
            if (ServerRunning) return; // 端口 5900 全机唯一，只可能有一个实例
            if (!File.Exists(ServerPath)) throw new FileNotFoundException("tvnserver.exe 未找到", ServerPath);
            ConfigureRegistry();
            CallAgent(agentPipe, "{\"op\":\"exec\",\"argv\":[\"open\",\"--name\",\"" +
                ServerPath.Replace("/", "\\") + "\"]}");
        }

        private static void CallAgent(string pipeName, string json)
        {
            using (var pipe = new System.IO.Pipes.NamedPipeClientStream(".", pipeName,
                System.IO.Pipes.PipeDirection.InOut))
            {
                pipe.Connect(3000);
                using (var w = new System.IO.StreamWriter(pipe, new System.Text.UTF8Encoding(false), 65536, true))
                using (var r = new System.IO.StreamReader(pipe, new System.Text.UTF8Encoding(false), false, 65536, true))
                {
                    w.AutoFlush = true;
                    w.WriteLine(json);
                    r.ReadLine();
                }
            }
        }

        public static bool PortListening(int port)
        {
            try
            {
                var c = new System.Net.Sockets.TcpClient();
                c.Connect("127.0.0.1", port);
                c.Close();
                return true;
            }
            catch { return false; }
        }

        /// 在子会话内确保 cua rfb-server --port 5901 在跑（TaskLauncher 直启）。
        public static void EnsureChildRfbServer(uint sessionId, string cuaExe, string dataDir)
        {
            if (PortListening(5901)) return;
            TaskLauncher.LaunchArgs(sessionId, cuaExe, "rfb-server --port 5901", dataDir);
        }

        /// 主会话内确保 cua rfb-server --port 5900 在跑。
        public static void EnsureMainRfbServer(string cuaExe, string dataDir)
        {
            if (PortListening(5900)) return;
            System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(cuaExe)
            {
                Arguments = "rfb-server --port 5900",
                UseShellExecute = false,
                CreateNoWindow = true,
                WorkingDirectory = dataDir
            });
        }

        public static bool HeadlessAutoDisable
        {
            get
            {
                using (var k = Registry.CurrentUser.OpenSubKey(SettingsKey))
                    return k != null && (k.GetValue("HeadlessAutoDisable") as int? == 1);
            }
            set
            {
                using (var k = Registry.CurrentUser.CreateSubKey(SettingsKey))
                    k.SetValue("HeadlessAutoDisable", value ? 1 : 0, RegistryValueKind.DWord);
            }
        }

        private const string SettingsKey = @"Software\cua\Vnc";
    }
}
