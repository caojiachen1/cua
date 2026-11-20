using System;
using System.IO;
using System.IO.Pipes;
using System.Text;
using System.Threading;

namespace CuaSessionHost
{
    /// <summary>
    /// Receives pointer events from the child session's cua (the agent writes
    /// one JSON line per action to <cua-cursor-&lt;childId&gt;). Events are
    /// best-effort: a missing/dead viewer must never affect the action itself,
    /// so the server swallows every error and the client never waits for a
    /// reply.
    /// </summary>
    internal sealed class CursorPipe
    {
        private readonly string _name;
        private readonly Action<int, int, bool> _onMove;
        private Thread _thread;
        private volatile bool _stopped;
        private volatile NamedPipeServerStream _server;

        public CursorPipe(string name, Action<int, int, bool> onMove)
        {
            _name = name;
            _onMove = onMove;
        }

        public void Start()
        {
            _thread = new Thread(Loop);
            _thread.IsBackground = true;
            _thread.Name = "cua-cursor-pipe";
            _thread.Start();
        }

        public void Stop()
        {
            _stopped = true;
            // WaitForConnection has no timeout; disposing the instance is the
            // only way to unblock the listener. Without this the old thread
            // keeps the pipe name, so a rebuild (DPI change) could never bind.
            var server = _server;
            if (server != null)
            {
                try { server.Dispose(); }
                catch (ObjectDisposedException) { }
            }
        }

        private void Loop()
        {
            while (!_stopped)
            {
                try
                {
                    using (var server = new NamedPipeServerStream(
                        _name, PipeDirection.In, 1, PipeTransmissionMode.Byte,
                        PipeOptions.None, 4096, 4096))
                    {
                        _server = server;
                        server.WaitForConnection();
                        using (var reader = new StreamReader(server, new UTF8Encoding(false)))
                        {
                            string line = reader.ReadLine();
                            if (line != null)
                            {
                                var d = Json.Parse(line);
                                int x = Json.Int(d, "x", int.MinValue);
                                int y = Json.Int(d, "y", int.MinValue);
                                if (x != int.MinValue && y != int.MinValue)
                                    _onMove(x, y, Json.Bool(d, "click", false));
                            }
                        }
                    }
                }
                catch (Exception)
                {
                    if (_stopped) return;
                    Thread.Sleep(150);
                }
                finally
                {
                    _server = null;
                }
            }
        }
    }
}
