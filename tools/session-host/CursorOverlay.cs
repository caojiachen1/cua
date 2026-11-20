using System;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Windows.Forms;

namespace CuaSessionHost
{
    /// <summary>
    /// The agent's pointer, drawn by the viewer instead of the remote session.
    ///
    /// Why not rely on the pointer preview that runs inside the child session?
    /// That overlay lives on the child desktop, so it travels through RDP as
    /// picture content only when the client happens to repaint that region —
    /// and mstscax draws the *remote cursor* as a local cursor that Windows
    /// only displays while the user's own mouse is over the client area. The
    /// result: a user watching the PIP sees clicks land with no cursor leading
    /// up to them. This overlay is owned by the host process, is topmost and
    /// click-through, and therefore stays visible regardless of where the
    /// user's physical mouse is.
    ///
    /// Two layered windows: a small arrow sprite (moved with SetWindowPos, no
    /// repaint per frame) and a ripple window (repainted only while it runs).
    /// The animation is driven by a WinForms timer on the UI thread, so the
    /// message pump keeps servicing drag/resize while the cursor glides.
    /// </summary>
    internal sealed class CursorOverlay : IDisposable
    {
        private const int GlideMs = 180;
        private const int RippleMs = 360;
        private const int IdleHideMs = 8000;
        // the art is drawn with a glow margin; a real Windows arrow is about
        // 12x19 at 100%, so shrink the sprite to sit naturally on the RDP
        // picture instead of covering two icons at 150%
        private const float SpriteZoom = 0.55f;

        private readonly HostForm _owner;
        private readonly IntPtr _parent;
        private readonly SpriteWindow _cursor;
        private readonly SpriteWindow _ripple;
        private readonly Timer _timer = new Timer { Interval = 15 };

        private float _scale;
        private Size _cursorSize;
        private Size _rippleSize;
        private int _hotX;
        private int _hotY;

        private PointF _pos;            // cursor tip, in screen coordinates
        private PointF _from;
        private PointF _to;
        private int _childX;
        private int _childY;
        private bool _anchored;
        private bool _visible;

        private readonly Stopwatch _clock = Stopwatch.StartNew();
        private long _glideStart = -1;
        private long _rippleStart = -1;
        private bool _clickPending;
        private long _lastAction;
        private bool _idleHidden;

        private bool _disposed;

        internal CursorOverlay(HostForm owner)
        {
            _owner = owner;
            _parent = owner.CursorLayerParent;
            _cursor = new SpriteWindow(_parent);
            _ripple = new SpriteWindow(_parent);
            _scale = Math.Max(1f, owner.UiScale) * SpriteZoom;
            RebuildSprites();
            _timer.Tick += delegate { OnTick(); };
            _timer.Start();
        }

        /// <summary>Move the fake cursor to a child-desktop coordinate.</summary>
        internal void MoveTo(int childX, int childY, bool click)
        {
            if (_disposed) return;
            Point screen;
            if (!_owner.TryMapChildPoint(childX, childY, out screen)) return;
            _childX = childX;
            _childY = childY;
            _lastAction = _clock.ElapsedMilliseconds;
            _idleHidden = false;
            if (_visible) _cursor.Show();
            _to = new PointF(screen.X, screen.Y);
            if (!_anchored)
            {
                _pos = _to;
                _from = _to;
                _anchored = true;
                _glideStart = -1;
                Place();
            }
            else
            {
                _from = _pos;
                _glideStart = _clock.ElapsedMilliseconds;
            }
            if (click)
            {
                // the ripple starts when the glide lands, not when the action
                // is issued — the dot should arrive first, then flash
                _clickPending = true;
            }
        }

        /// <summary>
        /// False once the window underneath us dies — WinForms recreates the
        /// form handle on DPI change, and that takes every child window with
        /// it. The host watches this on every poll and rebuilds the layer.
        /// </summary>
        internal bool IsAlive
        {
            get
            {
                if (_disposed) return false;
                if (_parent != _owner.CursorLayerParent) return false;
                return _cursor.IsAlive && _ripple.IsAlive;
            }
        }

        /// <summary>Diagnostics for `cua session status`.</summary>
        internal bool IsShown
        {
            get { return !_disposed && _visible && _anchored && !_idleHidden; }
        }

        internal long IdleAgeMs
        {
            get { return _anchored ? _clock.ElapsedMilliseconds - _lastAction : 0; }
        }

        internal void SetVisible(bool visible)
        {
            if (_disposed || _visible == visible) return;
            _visible = visible;
            if (!visible)
            {
                _cursor.Hide();
                _ripple.Hide();
                return;
            }
            // viewer was hidden while the last action aged out: show a blank
            // surface instead of a pointer parked where the agent worked ages
            // ago (next action reappears it)
            if (_anchored && _clock.ElapsedMilliseconds - _lastAction > IdleHideMs)
                _idleHidden = true;
            Place();
            if (!_idleHidden && _anchored) _cursor.Show();
            if (_rippleStart >= 0) _ripple.Show();
        }

        /// <summary>Re-map on layout/DPI change; drop a stale animation.</summary>
        internal void SyncLayout()
        {
            if (_disposed) return;
            float scale = Math.Max(1f, _owner.UiScale) * SpriteZoom;
            if (Math.Abs(scale - _scale) > 0.01f)
            {
                _scale = scale;
                RebuildSprites();
            }
            if (!_anchored) return;
            // keep the tip on the same child-desktop pixel and re-project it
            // after the viewer moved/resized
            Point mapped;
            if (_owner.TryMapChildPoint(_childX, _childY, out mapped))
            {
                _pos = new PointF(mapped.X, mapped.Y);
                _to = _pos;
                _from = _pos;
                _glideStart = -1;
            }
            Place();
        }

        private void RebuildSprites()
        {
            _cursorSize = new Size((int)Math.Round(22 * _scale), (int)Math.Round(32 * _scale));
            _hotX = (int)Math.Round(2 * _scale);
            _hotY = (int)Math.Round(2 * _scale);
            _cursor.Apply(DrawCursor(_cursorSize, _scale));
            _rippleSize = new Size((int)Math.Round(64 * _scale), (int)Math.Round(64 * _scale));
            Place();
        }

        private void OnTick()
        {
            if (_disposed) return;
            long now = _clock.ElapsedMilliseconds;
            bool busy = false;
            if (_glideStart >= 0)
            {
                double t = (now - _glideStart) / (double)GlideMs;
                if (t >= 1)
                {
                    _pos = _to;
                    _glideStart = -1;
                    if (_clickPending)
                    {
                        _clickPending = false;
                        _rippleStart = now;
                    }
                }
                else
                {
                    double e = 1 - Math.Pow(1 - t, 3);
                    _pos = new PointF(
                        (float)(_from.X + (_to.X - _from.X) * e),
                        (float)(_from.Y + (_to.Y - _from.Y) * e));
                    busy = true;
                }
                Place();
            }

            if (_rippleStart >= 0)
            {
                double t = (now - _rippleStart) / (double)RippleMs;
                if (t >= 1)
                {
                    _rippleStart = -1;
                    _ripple.Hide();
                }
                else
                {
                    _ripple.Apply(DrawRipple(_rippleSize, _scale, t));
                    if (_visible) _ripple.Show();
                    busy = true;
                }
            }

            // a parked fake cursor becomes residue (and can be mistaken for
            // the user's own pointer) — hide after a quiet period, reappear on
            // the next action
            if (!busy && _visible && _anchored && !_idleHidden &&
                now - _lastAction > IdleHideMs)
            {
                _idleHidden = true;
                _cursor.Hide();
                _ripple.Hide();
            }

            if (!busy) return;
        }

        private void Place()
        {
            if (!_anchored) return;
            if (_visible) _cursor.Move((int)Math.Round(_pos.X - _hotX), (int)Math.Round(_pos.Y - _hotY));
            if (_visible && _rippleStart >= 0)
                _ripple.Move(
                    (int)Math.Round(_pos.X - _rippleSize.Width / 2.0),
                    (int)Math.Round(_pos.Y - _rippleSize.Height / 2.0));
        }

        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;
            _timer.Stop();
            _timer.Dispose();
            _cursor.Dispose();
            _ripple.Dispose();
        }

        // ---------------------------------------------------------------- art

        private static Bitmap DrawCursor(Size size, float scale)
        {
            var bmp = new Bitmap(size.Width, size.Height, PixelFormat.Format32bppArgb);
            using (var g = Graphics.FromImage(bmp))
            {
                g.Clear(Color.Transparent);
                g.SmoothingMode = SmoothingMode.AntiAlias;
                g.PixelOffsetMode = PixelOffsetMode.HighQuality;
                float s = scale;
                var pts = new[]
                {
                    new PointF(2.2f * s, 2.0f * s),
                    new PointF(2.2f * s, 22.0f * s),
                    new PointF(7.0f * s, 17.6f * s),
                    new PointF(11.2f * s, 27.0f * s),
                    new PointF(15.0f * s, 25.4f * s),
                    new PointF(10.9f * s, 16.2f * s),
                    new PointF(18.4f * s, 15.4f * s),
                };
                using (var glow = new SolidBrush(Color.FromArgb(64, 96, 205, 255)))
                using (var gp = new GraphicsPath())
                {
                    gp.AddPolygon(pts);
                    g.FillPath(glow, gp);
                }
                using (var path = new GraphicsPath())
                {
                    path.AddPolygon(pts);
                    using (var fill = new SolidBrush(Color.FromArgb(246, 248, 250)))
                    using (var pen = new Pen(Color.FromArgb(18, 22, 26), 1.3f * scale))
                    {
                        pen.LineJoin = LineJoin.Round;
                        g.FillPath(fill, path);
                        g.DrawPath(pen, path);
                    }
                }
                // accent ring at the tip so the sprite reads as "agent", not
                // as the user's own pointer
                using (var pen = new Pen(Color.FromArgb(210, 96, 205, 255), 1.6f * scale))
                {
                    g.DrawEllipse(pen, 0.2f * scale, 0.2f * scale, 3.6f * scale, 3.6f * scale);
                }
            }
            return bmp;
        }

        private static Bitmap DrawRipple(Size size, float scale, double t)
        {
            var bmp = new Bitmap(size.Width, size.Height, PixelFormat.Format32bppArgb);
            using (var g = Graphics.FromImage(bmp))
            {
                g.Clear(Color.Transparent);
                g.SmoothingMode = SmoothingMode.AntiAlias;
                float cx = size.Width / 2f;
                float cy = size.Height / 2f;
                float maxR = 25f * scale;
                float r = (float)(4f * scale + (maxR - 4f * scale) * t);
                int alpha = (int)Math.Round(190 * (1 - t));
                using (var pen = new Pen(Color.FromArgb(Math.Max(0, alpha), 96, 205, 255), 2.0f * scale))
                {
                    g.DrawEllipse(pen, cx - r, cy - r, r * 2, r * 2);
                }
                float dotR = Math.Max(1.5f, 4f * scale * (1 - (float)t));
                using (var fill = new SolidBrush(Color.FromArgb(Math.Max(0, (int)(120 * (1 - t))), 96, 205, 255)))
                {
                    g.FillEllipse(fill, cx - dotR, cy - dotR, dotR * 2, dotR * 2);
                }
            }
            return bmp;
        }

        // ------------------------------------------------------------- layered

        /// <summary>One borderless topmost layered window. No WndProc work:
        /// UpdateLayeredWindow owns every pixel and the window never paints.</summary>
        private sealed class SpriteWindow : IDisposable
        {
            private const int WS_EX_LAYERED = 0x00080000;
            private const int WS_EX_TRANSPARENT = 0x00000020;
            private const int WS_EX_TOOLWINDOW = 0x00000080;
            private const int WS_EX_NOACTIVATE = 0x08000000;
            private const int WS_EX_TOPMOST = 0x00000008;
            private const int WS_POPUP = unchecked((int)0x80000000);
            private const int WS_CHILD = 0x40000000;
            private const int SW_HIDE = 0;
            private const int SW_SHOWNOACTIVATE = 4;
            private const int SWP_NOSIZE = 0x0001;
            private const int SWP_NOMOVE = 0x0002;
            private const int SWP_NOACTIVATE = 0x0010;
            private const int SWP_SHOWWINDOW = 0x0040;
            private const uint ULW_ALPHA = 0x00000002;
            private const byte AC_SRC_OVER = 0x00;
            private const byte AC_SRC_ALPHA = 0x01;

            private static string _className;
            private static WndProcDelegate _wndProc;

            private IntPtr _hwnd;
            private Size _size;
            private int _x;
            private int _y;

            public bool IsAlive
            {
                get { return _hwnd != IntPtr.Zero && IsWindow(_hwnd); }
            }

            public SpriteWindow(IntPtr parent)
            {
                EnsureClass();
                _hwnd = CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
                    _className, "", WS_CHILD, 0, 0, 1, 1, parent, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero);
                if (_hwnd == IntPtr.Zero)
                    throw new InvalidOperationException("cursor overlay window could not be created");
                ShowWindow(_hwnd, SW_HIDE);
            }

            /// <summary>
            /// Upload one GDI+ bitmap as the window's pixels. UpdateLayeredWindow
            /// wants premultiplied BGRA, but GDI+ hands back straight alpha, so
            /// the copy into the DIB section premultiplies explicitly (the
            /// GetHbitmap shortcut drops alpha and renders the window invisible).
            /// </summary>
            public void Apply(Bitmap bitmap)
            {
                _size = bitmap.Size;
                int w = bitmap.Width;
                int h = bitmap.Height;
                var bmi = new BITMAPINFO
                {
                    bmiHeader = new BITMAPINFOHEADER
                    {
                        biSize = Marshal.SizeOf(typeof(BITMAPINFOHEADER)),
                        biWidth = w,
                        biHeight = -h,          // top-down: row 0 is the top row
                        biPlanes = 1,
                        biBitCount = 32,
                        biCompression = 0,      // BI_RGB
                        biSizeImage = w * h * 4
                    }
                };
                IntPtr screen = GetDC(IntPtr.Zero);
                IntPtr mem = CreateCompatibleDC(screen);
                IntPtr bits;
                IntPtr hbitmap = CreateDIBSection(mem, ref bmi, 0 /* DIB_RGB_COLORS */,
                    out bits, IntPtr.Zero, 0);
                if (hbitmap == IntPtr.Zero || bits == IntPtr.Zero)
                {
                    DeleteDC(mem);
                    ReleaseDC(IntPtr.Zero, screen);
                    return;
                }
                IntPtr old = SelectObject(mem, hbitmap);
                try
                {
                    BitmapData data = bitmap.LockBits(new Rectangle(0, 0, w, h),
                        ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
                    try
                    {
                        var row = new byte[w * 4];
                        for (int y = 0; y < h; y++)
                        {
                            Marshal.Copy(data.Scan0 + y * data.Stride, row, 0, w * 4);
                            IntPtr dst = bits + y * w * 4;
                            for (int x = 0; x < w; x++)
                            {
                                int i = x * 4;
                                int a = row[i + 3];
                                row[i + 0] = (byte)((row[i + 0] * a + 127) / 255);
                                row[i + 1] = (byte)((row[i + 1] * a + 127) / 255);
                                row[i + 2] = (byte)((row[i + 2] * a + 127) / 255);
                            }
                            Marshal.Copy(row, 0, dst, w * 4);
                        }
                    }
                    finally
                    {
                        bitmap.UnlockBits(data);
                    }

                    var dstPoint = new POINT { x = _x, y = _y };
                    var src = new POINT { x = 0, y = 0 };
                    var size = new SIZE { cx = w, cy = h };
                    var blend = new BLENDFUNCTION
                    {
                        BlendOp = AC_SRC_OVER,
                        BlendFlags = 0,
                        SourceConstantAlpha = 255,
                        AlphaFormat = AC_SRC_ALPHA
                    };
                    UpdateLayeredWindow(_hwnd, screen, ref dstPoint, ref size, mem, ref src,
                        0, ref blend, ULW_ALPHA);
                }
                finally
                {
                    SelectObject(mem, old);
                    DeleteObject(hbitmap);
                    DeleteDC(mem);
                    ReleaseDC(IntPtr.Zero, screen);
                }
            }

            public void Move(int x, int y)
            {
                _x = x;
                _y = y;
                SetWindowPos(_hwnd, IntPtr.Zero /* HWND_TOP among siblings */, x, y,
                    _size.Width, _size.Height, SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            }

            public void Show()
            {
                // reassert the sibling z-order: the RDP control is a sibling too
                // and a repaint may have pushed it above us
                SetWindowPos(_hwnd, IntPtr.Zero, 0, 0, 0, 0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            }

            public void Hide()
            {
                ShowWindow(_hwnd, SW_HIDE);
            }

            public void Dispose()
            {
                if (_hwnd != IntPtr.Zero) DestroyWindow(_hwnd);
                _hwnd = IntPtr.Zero;
            }

            private static void EnsureClass()
            {
                if (_className != null) return;
                _className = "CuaCursorOverlayWindow";
                _wndProc = delegate(IntPtr h, uint msg, IntPtr w, IntPtr l)
                {
                    return DefWindowProcW(h, msg, w, l);
                };
                var wc = new WNDCLASSEX
                {
                    cbSize = (uint)Marshal.SizeOf(typeof(WNDCLASSEX)),
                    style = 0,
                    lpfnWndProc = _wndProc,
                    cbClsExtra = 0,
                    cbWndExtra = 0,
                    hInstance = GetModuleHandleW(null),
                    hIcon = IntPtr.Zero,
                    hCursor = IntPtr.Zero,
                    hbrBackground = IntPtr.Zero,
                    lpszMenuName = null,
                    lpszClassName = _className,
                    hIconSm = IntPtr.Zero
                };
                if (RegisterClassExW(ref wc) == 0)
                    throw new InvalidOperationException("cursor overlay window class could not be registered");
            }

            private delegate IntPtr WndProcDelegate(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);

            [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
            private struct WNDCLASSEX
            {
                public uint cbSize;
                public uint style;
                public WndProcDelegate lpfnWndProc;
                public int cbClsExtra;
                public int cbWndExtra;
                public IntPtr hInstance;
                public IntPtr hIcon;
                public IntPtr hCursor;
                public IntPtr hbrBackground;
                public string lpszMenuName;
                public string lpszClassName;
                public IntPtr hIconSm;
            }

            [StructLayout(LayoutKind.Sequential)]
            private struct POINT
            {
                public int x;
                public int y;
            }

            [StructLayout(LayoutKind.Sequential)]
            private struct SIZE
            {
                public int cx;
                public int cy;
            }

            [StructLayout(LayoutKind.Sequential, Pack = 1)]
            private struct BLENDFUNCTION
            {
                public byte BlendOp;
                public byte BlendFlags;
                public byte SourceConstantAlpha;
                public byte AlphaFormat;
            }

            [StructLayout(LayoutKind.Sequential)]
            private struct BITMAPINFOHEADER
            {
                public int biSize;
                public int biWidth;
                public int biHeight;
                public short biPlanes;
                public short biBitCount;
                public int biCompression;
                public int biSizeImage;
                public int biXPelsPerMeter;
                public int biYPelsPerMeter;
                public int biClrUsed;
                public int biClrImportant;
            }

            [StructLayout(LayoutKind.Sequential)]
            private struct BITMAPINFO
            {
                public BITMAPINFOHEADER bmiHeader;
                public int bmiColors;
            }

            [DllImport("gdi32.dll")]
            private static extern IntPtr CreateDIBSection(IntPtr dc, ref BITMAPINFO bmi,
                uint usage, out IntPtr bits, IntPtr section, uint offset);

            [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
            private static extern IntPtr CreateWindowExW(int exStyle, string className, string windowName,
                int style, int x, int y, int width, int height, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);

            [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
            private static extern ushort RegisterClassExW(ref WNDCLASSEX wc);

            [DllImport("user32.dll")]
            private static extern IntPtr DefWindowProcW(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);

            [DllImport("user32.dll")]
            private static extern bool ShowWindow(IntPtr hwnd, int cmd);

            [DllImport("user32.dll")]
            private static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, int flags);

            [DllImport("user32.dll")]
            private static extern bool IsWindow(IntPtr hwnd);

            [DllImport("user32.dll")]
            private static extern bool DestroyWindow(IntPtr hwnd);

            [DllImport("user32.dll")]
            private static extern IntPtr GetDC(IntPtr hwnd);

            [DllImport("user32.dll")]
            private static extern int ReleaseDC(IntPtr hwnd, IntPtr dc);

            [DllImport("user32.dll")]
            private static extern bool UpdateLayeredWindow(IntPtr hwnd, IntPtr dstDc, ref POINT dst,
                ref SIZE size, IntPtr srcDc, ref POINT src, uint colorKey, ref BLENDFUNCTION blend, uint flags);

            [DllImport("gdi32.dll")]
            private static extern IntPtr CreateCompatibleDC(IntPtr dc);

            [DllImport("gdi32.dll")]
            private static extern bool DeleteDC(IntPtr dc);

            [DllImport("gdi32.dll")]
            private static extern IntPtr SelectObject(IntPtr dc, IntPtr obj);

            [DllImport("gdi32.dll")]
            private static extern bool DeleteObject(IntPtr obj);

            [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
            private static extern IntPtr GetModuleHandleW(string name);
        }
    }
}
