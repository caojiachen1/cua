using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Windows.Forms;

namespace CuaSessionHost
{
    /// <summary>
    /// 嵌入式 RFB (VNC) 客户端控件：连接本机 cua rfb-server（会话内 GDI 抓屏），
    /// 渲染子会话画面并回传鼠标键盘。与 RDP ActiveX 并排存在，观察窗在两种
    /// 观察模式间无缝切换。
    /// 协议：RFB 3.8 + None 认证 + Raw 编码（参考 TightVNC 行为，纯自研）。
    /// </summary>
    internal sealed class RfbClientControl : Control
    {
        private TcpClient _client;
        private NetworkStream _s;
        private Thread _net;
        private volatile bool _run;
        private readonly object _ioLock = new object();
        private readonly object _frameLock = new object();
        private Bitmap _frame;
        private int _fw, _fh;
        private byte _prevMask;
        private long _frameN;
        private int _port;
        private volatile bool _handshaken;

        public event EventHandler Connected;
        public event Action<string> Failed;

        private static long _logN;
        private static void Clog(string m)
        {
            try
            {
                var dir = System.IO.Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "cua");
                System.IO.File.AppendAllText(System.IO.Path.Combine(dir, "rfb-client.log"),
                    DateTime.Now.ToString("HH:mm:ss.fff") + " " + m + Environment.NewLine);
            }
            catch { }
        }

        public bool IsConnected
        {
            get { return _run && _client != null && _client.Connected && _handshaken; }
        }

        public void Connect(int port)
        {
            Disconnect();
            _port = port;
            _run = true;
            _net = new Thread(NetLoop) { IsBackground = true, Name = "rfb-client" };
            _net.Start();
        }

        public void Disconnect()
        {
            _run = false;
            _handshaken = false;
            try { _client?.Close(); } catch { }
        }

        protected override void Dispose(bool disposing)
        {
            if (disposing) Disconnect();
            base.Dispose(disposing);
        }

        private void NetLoop()
        {
            while (_run)
            {
                try
                {
                    _client = new TcpClient();
                    _client.Connect("127.0.0.1", _port);
                    _s = _client.GetStream();
                    _s.ReadTimeout = 15000;
                    Handshake();
                    _handshaken = true;
                    Clog("handshake ok port=" + _port);
                    SafeConnected();
                    // RFB 的画面更新由客户端请求驱动：先全量请求，之后以 ~15fps
                    // 发增量请求，服务端仅在画面变化时回帧。
                    SendFramebufferRequest(false);
                    var reqThread = new Thread(UpdateRequestLoop) { IsBackground = true, Name = "rfb-req" };
                    reqThread.Start();
                    MessageLoop();
                }
                catch (Exception ex)
                {
                    Clog("net error: " + ex.Message);
                    SafeFailed(ex.Message);
                }
                if (_run) Thread.Sleep(1500);
            }
        }

        private void SafeConnected()
        {
            try { Connected?.Invoke(this, EventArgs.Empty); } catch { }
        }

        private void SafeFailed(string m)
        {
            try { Failed?.Invoke(m); } catch { }
        }

        // ---------------- RFB 协议 ----------------

        private void Handshake()
        {
            var s = _s;
            var banner = ReadExact(s, 12);
            var text = Encoding.ASCII.GetString(banner);
            if (!text.StartsWith("RFB ")) throw new InvalidOperationException("非 RFB 服务端");
            WriteAscii(s, "RFB 003.008\n");

            // 安全类型协商（3.7/3.8 列表式）
            int count = s.ReadByte();
            if (count <= 0) throw new InvalidOperationException("服务端拒绝连接");
            int chosen = -1;
            for (int i = 0; i < count; i++)
            {
                int t = s.ReadByte();
                if (t == 1) chosen = 1; // None
            }
            if (chosen < 0) throw new InvalidOperationException("无可用认证方式");
            s.WriteByte((byte)chosen);
            var result = ReadExact(s, 4);
            if (result[3] != 0) throw new InvalidOperationException("安全协商失败");

            // ClientInit
            s.WriteByte(1); // shared

            // ServerInit：w(2)+h(2) 为帧缓冲头，pixel-format 另占 16 字节
            var head = ReadExact(s, 4);
            _fw = (head[0] << 8) | head[1];
            _fh = (head[2] << 8) | head[3];
            var pix = ReadExact(s, 16); // 声明格式（ bpp32 RGBA shifts 0/8/16）
            var nameLen = ReadExact(s, 4);
            int nl = (nameLen[0] << 24) | (nameLen[1] << 16) | (nameLen[2] << 8) | nameLen[3];
            if (nl > 0 && nl < 4096) ReadExact(s, nl);

            lock (_frameLock)
            {
                _frame?.Dispose();
                _frame = new Bitmap(_fw, _fh, System.Drawing.Imaging.PixelFormat.Format32bppArgb);
            }
        }

        private void UpdateRequestLoop()
        {
            // 增量全屏请求，~15fps 上限；服务端画面未变化时返回空更新
            while (_run)
            {
                SendFramebufferRequest(true);
                Thread.Sleep(66);
            }
        }

        private void SendFramebufferRequest(bool incremental)
        {
            lock (_ioLock)
            {
                var s = _s;
                if (s == null || !_run) return;
                var b = new byte[10];
                b[0] = 3; // FramebufferUpdateRequest
                b[1] = incremental ? (byte)1 : (byte)0;
                // x/y/w/h = 0,0,65535,65535 → 服务端按帧缓冲全幅裁剪
                b[4] = 0xFF; b[5] = 0xFF;
                b[8] = 0xFF; b[9] = 0xFF;
                s.Write(b, 0, 10);
                s.Flush();
            }
        }

        private void MessageLoop()
        {
            var s = _s;
            while (_run)
            {
                int type = s.ReadByte();
                if (type < 0) throw new InvalidOperationException("连接关闭");
                switch (type)
                {
                    case 0: // FramebufferUpdate
                        {
                            var pad = ReadExact(s, 1);
                            var cnt = ReadExact(s, 2);
                            int rects = (cnt[0] << 8) | cnt[1];
                            for (int i = 0; i < rects; i++)
                            {
                                var hdr = ReadExact(s, 12);
                                int rx = (hdr[0] << 8) | hdr[1];
                                int ry = (hdr[2] << 8) | hdr[3];
                                int rw = (hdr[4] << 8) | hdr[5];
                                int rh = (hdr[6] << 8) | hdr[7];
                                int enc = (hdr[8] << 24) | (hdr[9] << 16) | (hdr[10] << 8) | hdr[11];
                                if (enc != 0) throw new InvalidOperationException("不支持的编码 " + enc);
                                var px = ReadExact(s, rw * rh * 4);
                                Blit(rx, ry, rw, rh, px);
                            }
                            long n = Interlocked.Increment(ref _frameN);
                            if (n % 20 == 1) Clog("frame #" + n + " rects=" + rects);
                            Invalidate();
                        }
                        break;
                    case 1: // SetColourMapEntries
                        {
                            var skip = ReadExact(s, 5);
                            var len = ReadExact(s, 2);
                            int n = (len[0] << 8) | len[1];
                            ReadExact(s, n * 6);
                        }
                        break;
                    case 2: // Bell
                        break;
                    case 3: // ServerCutText
                        {
                            ReadExact(s, 3);
                            var len = ReadExact(s, 4);
                            int n = (len[0] << 24) | (len[1] << 16) | (len[2] << 8) | len[3];
                            if (n > 0 && n < 1 << 20) ReadExact(s, n);
                        }
                        break;
                    default:
                        throw new InvalidOperationException("未知服务端消息 " + type);
                }
            }
        }

        private void Blit(int rx, int ry, int rw, int rh, byte[] rgba)
        {
            lock (_frameLock)
            {
                if (_frame == null) return;
                if (rx + rw > _fw || ry + rh > _fh) return;
                var bd = _frame.LockBits(new Rectangle(rx, ry, rw, rh),
                    System.Drawing.Imaging.ImageLockMode.WriteOnly,
                    System.Drawing.Imaging.PixelFormat.Format32bppArgb);
                try
                {
                    // RGBA -> BGRA（无 unsafe：行级 Marshal.Copy）
                    var bgra = new byte[rw * rh * 4];
                    for (int i = 0; i < rw * rh; i++)
                    {
                        bgra[i * 4] = rgba[i * 4 + 2];
                        bgra[i * 4 + 1] = rgba[i * 4 + 1];
                        bgra[i * 4 + 2] = rgba[i * 4];
                        bgra[i * 4 + 3] = 255;
                    }
                    var dstBase = bd.Scan0;
                    int stride = bd.Stride;
                    for (int y = 0; y < rh; y++)
                    {
                        System.Runtime.InteropServices.Marshal.Copy(
                            bgra, y * rw * 4, dstBase + y * stride, rw * 4);
                    }
                }
                finally { _frame.UnlockBits(bd); }
            }
        }

        // ---------------- 输入发送 ----------------

        private void SendPointer(int fx, int fy, byte mask)
        {
            lock (_ioLock)
            {
                var s = _s;
                if (s == null || !_run) return;
                var b = new byte[6];
                b[0] = 5; // PointerEvent
                b[1] = mask;
                b[2] = (byte)(fx >> 8); b[3] = (byte)fx;
                b[4] = (byte)(fy >> 8); b[5] = (byte)fy;
                s.Write(b, 0, 6);
                s.Flush();
            }
        }

        private void SendKey(uint keysym, bool down)
        {
            lock (_ioLock)
            {
                var s = _s;
                if (s == null || !_run) return;
                var b = new byte[8];
                b[0] = 4; // KeyEvent
                b[1] = down ? (byte)1 : (byte)0;
                b[4] = (byte)(keysym >> 24);
                b[5] = (byte)(keysym >> 16);
                b[6] = (byte)(keysym >> 8);
                b[7] = (byte)keysym;
                s.Write(b, 0, 8);
                s.Flush();
            }
        }

        // ---------------- 输入事件（控件层） ----------------

        private Rectangle DisplayRect()
        {
            var inner = ClientRectangle;
            if (_fw <= 0 || _fh <= 0) return inner;
            double sx = (double)inner.Width / _fw, sy = (double)inner.Height / _fh;
            double scale = Math.Min(sx, sy);
            int w = Math.Max(1, (int)(_fw * scale)), h = Math.Max(1, (int)(_fh * scale));
            return new Rectangle(inner.X + (inner.Width - w) / 2, inner.Y + (inner.Height - h) / 2, w, h);
        }

        private Point ToFrame(int cx, int cy)
        {
            var r = DisplayRect();
            double sx = (double)_fw / r.Width, sy = (double)_fh / r.Height;
            return new Point(
                Math.Max(0, Math.Min(_fw - 1, (int)((cx - r.X) * sx))),
                Math.Max(0, Math.Min(_fh - 1, (int)((cy - r.Y) * sy))));
        }

        protected override void OnMouseMove(MouseEventArgs e)
        {
            base.OnMouseMove(e);
            if (!IsConnected || _fw <= 0) return;
            var p = ToFrame(e.X, e.Y);
            SendPointer(p.X, p.Y, _prevMask);
        }

        protected override void OnMouseDown(MouseEventArgs e)
        {
            base.OnMouseDown(e);
            if (!IsConnected || _fw <= 0) return;
            var p = ToFrame(e.X, e.Y);
            if (e.Button == MouseButtons.Left) _prevMask |= 1;
            if (e.Button == MouseButtons.Middle) _prevMask |= 2;
            if (e.Button == MouseButtons.Right) _prevMask |= 4;
            SendPointer(p.X, p.Y, _prevMask);
        }

        protected override void OnMouseUp(MouseEventArgs e)
        {
            base.OnMouseUp(e);
            if (!IsConnected || _fw <= 0) return;
            var p = ToFrame(e.X, e.Y);
            if (e.Button == MouseButtons.Left) _prevMask &= unchecked((byte)~1);
            if (e.Button == MouseButtons.Middle) _prevMask &= unchecked((byte)~2);
            if (e.Button == MouseButtons.Right) _prevMask &= unchecked((byte)~4);
            SendPointer(p.X, p.Y, _prevMask);
        }

        protected override void OnMouseWheel(MouseEventArgs e)
        {
            base.OnMouseWheel(e);
            if (!IsConnected || _fw <= 0) return;
            var p = ToFrame(e.X, e.Y);
            byte mask = _prevMask;
            if (e.Delta > 0) mask |= 8; else mask |= 16;
            SendPointer(p.X, p.Y, mask);   // wheel 按下沿
            mask = _prevMask;
            SendPointer(p.X, p.Y, mask);   // 释放沿
        }

        protected override void OnKeyDown(KeyEventArgs e)
        {
            base.OnKeyDown(e);
            if (!IsConnected) return;
            uint? ks = KeysToKeysym(e.KeyCode);
            if (ks.HasValue) SendKey(ks.Value, true);
        }

        protected override void OnKeyUp(KeyEventArgs e)
        {
            base.OnKeyUp(e);
            if (!IsConnected) return;
            uint? ks = KeysToKeysym(e.KeyCode);
            if (ks.HasValue) SendKey(ks.Value, false);
        }

        protected override void OnKeyPress(KeyPressEventArgs e)
        {
            base.OnKeyPress(e);
            if (!IsConnected) return;
            if (e.KeyChar >= ' ')
            {
                // 可打印字符（含大小写）直接走 latin-1 keysym
                SendKey((uint)e.KeyChar, true);
                SendKey((uint)e.KeyChar, false);
            }
        }

        private static uint? KeysToKeysym(Keys k)
        {
            switch (k)
            {
                case Keys.Return: return 0xFF0D;
                case Keys.Back: return 0xFF08;
                case Keys.Tab: return 0xFF09;
                case Keys.Escape: return 0xFF1B;
                case Keys.Delete: return 0xFFFF;
                case Keys.Home: return 0xFF50;
                case Keys.Left: return 0xFF51;
                case Keys.Up: return 0xFF52;
                case Keys.Right: return 0xFF53;
                case Keys.Down: return 0xFF54;
                case Keys.Prior: return 0xFF55;
                case Keys.Next: return 0xFF56;
                case Keys.End: return 0xFF57;
                case Keys.Insert: return 0xFF63;
                case Keys.Space: return 0x20;
                case Keys.ShiftKey: case Keys.LShiftKey: case Keys.RShiftKey: return 0xFFE1;
                case Keys.ControlKey: case Keys.LControlKey: case Keys.RControlKey: return 0xFFE3;
                case Keys.Menu: case Keys.LMenu: case Keys.RMenu: return 0xFFE9;
                case Keys.LWin: return 0xFFEB;
                case Keys.RWin: return 0xFFEC;
                default:
                    if (k >= Keys.F1 && k <= Keys.F24)
                        return 0xFFBE + (uint)(k - Keys.F1);
                    return null;
            }
        }

        // ---------------- 渲染 ----------------

        protected override void OnPaint(PaintEventArgs e)
        {
            base.OnPaint(e);
            lock (_frameLock)
            {
                if (_frame == null)
                {
                    e.Graphics.Clear(Color.FromArgb(24, 24, 28));
                    return;
                }
                var r = DisplayRect();
                if (r.Width <= 0 || r.Height <= 0) return;
                // 画中画把 2560 宽的帧缩到几百像素，双线性比最近邻干净得多
                e.Graphics.InterpolationMode = InterpolationMode.Bilinear;
                e.Graphics.DrawImage(_frame, r);
            }
        }

        protected override void OnHandleCreated(EventArgs e)
        {
            base.OnHandleCreated(e);
            SetStyle(ControlStyles.OptimizedDoubleBuffer | ControlStyles.AllPaintingInWmPaint | ControlStyles.UserPaint, true);
        }

        private static byte[] ReadExact(NetworkStream s, int count)
        {
            var buf = new byte[count];
            int off = 0;
            while (off < count)
            {
                int n = s.Read(buf, off, count - off);
                if (n <= 0) throw new InvalidOperationException("连接关闭");
                off += n;
            }
            return buf;
        }

        private static void WriteAscii(NetworkStream s, string text)
        {
            var b = Encoding.ASCII.GetBytes(text);
            s.Write(b, 0, b.Length);
            s.Flush();
        }
    }
}
