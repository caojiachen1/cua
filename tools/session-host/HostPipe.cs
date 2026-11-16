using System;
using System.IO;
using System.IO.Pipes;
using System.Text;
using System.Threading;

namespace CuaSessionHost
{
    internal sealed class HostPipe
    {
        private readonly string _name;
        private readonly HostForm _form;
        private Thread _thread;
        private volatile bool _stopped;

        public HostPipe(string name, HostForm form)
        {
            _name = name;
            _form = form;
        }

        public void Start()
        {
            _thread = new Thread(Loop);
            _thread.IsBackground = true;
            _thread.Name = "cua-session-host-pipe";
            _thread.Start();
        }

        public void Stop()
        {
            _stopped = true;
        }

        private void Loop()
        {
            while (!_stopped)
            {
                try
                {
                    using (var server = new NamedPipeServerStream(
                        _name, PipeDirection.InOut, 1, PipeTransmissionMode.Message,
                        PipeOptions.None, 1 << 20, 32 << 20))
                    {
                        server.WaitForConnection();
                        using (var reader = new StreamReader(server, new UTF8Encoding(false),
                                   false, 65536, true))
                        using (var writer = new StreamWriter(server, new UTF8Encoding(false),
                                   65536, true))
                        {
                            writer.AutoFlush = true;
                            string line = reader.ReadLine();
                            if (line != null)
                            {
                                var request = Json.Parse(line);
                                string response = _form.HandleHostCommand(request);
                                writer.WriteLine(response);
                                writer.Flush();
                                try { server.WaitForPipeDrain(); } catch { }
                            }
                        }
                    }
                }
                catch (Exception)
                {
                    if (_stopped) return;
                    Thread.Sleep(150);
                }
            }
        }
    }
}
