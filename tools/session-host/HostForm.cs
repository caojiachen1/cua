using System;
using System.Collections;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace CuaSessionHost
{
    internal sealed class HostForm : Form
    {
        private const int WsMinimizeBox = 0x00020000;

        // 画中画手势：在消息分发前截获，才能盖住 RDP ActiveX / RFB 子控件
        private const int WmMouseMove = 0x0200;
        private const int WmLButtonDown = 0x0201;
        private const int WmLButtonUp = 0x0202;
        private const int WmLButtonDblClk = 0x0203;
        private const int WmNcLButtonDblClk = 0x00A3;
        private const int WmSetCursor = 0x0020;
        private const int HtCaption = 2;

        [Flags]
        private enum PipZone
        {
            None = 0,
            Left = 1,
            Right = 2,
            Top = 4,
            Bottom = 8
        }

        private enum PipGesture
        {
            None,
            Move,
            Resize
        }

        /// <summary>
        /// 画面贴边后边框热区被 _bar/_content/RDP 控件盖住，它们的 WM_SETCURSOR
        /// 才是唯一能改光标的地方（Form 自己的 WndProc 收不到）。
        /// </summary>
        private sealed class CursorRelay : NativeWindow
        {
            private readonly HostForm _form;

            internal CursorRelay(HostForm form)
            {
                _form = form;
            }

            protected override void WndProc(ref Message m)
            {
                if (m.Msg == WmSetCursor && _form.RelayCursor(m.HWnd))
                {
                    m.Result = (IntPtr)1;
                    return;
                }
                base.WndProc(ref m);
            }
        }

        /// <summary>
        /// 画中画里 RDP 控件的点击会先经过本线程的消息泵；在这里截获鼠标，
        /// 观察（只读）状态下把画面变成可拖动/可双击的窗口，接管状态下放行。
        /// </summary>
        private sealed class PipFilter : IMessageFilter
        {
            private readonly HostForm _form;

            internal PipFilter(HostForm form)
            {
                _form = form;
            }

            public bool PreFilterMessage(ref Message m)
            {
                return _form.FilterMessage(ref m);
            }
        }

        private readonly Options _opt;
        private readonly Panel _bar;
        private readonly Panel _content;
        private readonly SessionRdp _rdp;
        private readonly Label _title;
        private readonly Label _meta;
        private readonly StatusPill _statusPill;
        private readonly FluentButton _mode;
        private readonly FluentButton _takeover;
        private readonly FluentButton _release;
        private readonly FluentButton _settings;
        private readonly FluentButton _edge;
        private readonly FluentButton _minimize;
        private readonly FluentButton _maximize;
        private readonly FluentButton _close;
        private readonly System.Windows.Forms.Timer _poll;
        private readonly DateTime _started = DateTime.UtcNow;
        private readonly PipFilter _filter;
        private readonly System.Collections.Generic.List<CursorRelay> _relays =
            new System.Collections.Generic.List<CursorRelay>();

        private NotifyIcon _tray;
        private ContextMenuStrip _trayMenu;
        private ToolStripMenuItem _menuTakeover;
        private ToolStripMenuItem _menuRelease;
        private ToolStripMenuItem _menuStart;
        private ToolStripMenuItem _menuRestart;
        private ToolStripMenuItem _menuLogoff;
        private ToolStripMenuItem _menuExit;
        private ToolTip _tips;

        private HostPipe _pipe;
        private uint? _childId;
        private string _agentPipe;
        private bool _connected;
        private bool _agentReady;
        private bool _agentTried;
        private int _agentAttempts;
        private DateTime _agentRetryAt = DateTime.MinValue;
        private bool _viewOnly = true;
        private bool _viewerShown;
        private bool _viewerInitialized;
        private DateTime _reconnectAt = DateTime.MinValue;
        private bool _vncAutoDone;
        private uint? _lastChildId;
        private int _reconnectAttempts;
        private bool _plainTried;
        private RfbClientControl _rfbCtl;
        private bool _vncMode;
        private System.Windows.Forms.ContextMenuStrip _modeMenu;
        private System.Windows.Forms.ToolStripMenuItem _modeRdpItem;
        private System.Windows.Forms.ToolStripMenuItem _modeVncItem;
        private System.Windows.Forms.ToolStripMenuItem _menuHeadless;
        private bool _readyStatus;
        private bool _stopping;
        private bool _trayTipShown;
        private bool _isMaximized;
        private StartupQuarantine _startup;
        private bool _startupReleased = true;
        private DateTime _startupReleaseAt = DateTime.MinValue;
        private string _lastError;
        private ChildCredential _credential;
        private int _scalePercent = 100;
        private float _uiScale = 1f;

        // 画中画：画面贴边，没有外框留白；靠边缘几像素的透明热区缩放
        private Rectangle _pipBounds = Rectangle.Empty;
        private int _grip = 6;
        private double _aspect = 16.0 / 9.0;
        private PipGesture _gesture;
        private PipZone _gestureZone;
        private Rectangle _gestureStart;
        private Point _gestureCursor;
        private PipZone _cursorZone;
        private DateTime _lastDownAt = DateTime.MinValue;
        private Point _lastDownPos = Point.Empty;
        private DateTime _lastCaptionDown = DateTime.MinValue;

        public HostForm(Options opt)
        {
            _opt = opt;
            // 明确给了桌面尺寸（--width/--height）就先按它定比例；连接后再由实际桌面覆盖
            if (opt.ExplicitSize && opt.Width > 0 && opt.Height > 0)
                _aspect = (double)opt.Width / opt.Height;
            Text = "cua · child session";
            Icon = Theme.AppIcon(false);
            FormBorderStyle = FormBorderStyle.None;
            AutoScaleMode = AutoScaleMode.None;
            StartPosition = FormStartPosition.Manual;
            BackColor = Theme.Window;
            ShowInTaskbar = false;
            MinimizeBox = true;
            MaximizeBox = true;

            Rectangle work = Screen.PrimaryScreen.WorkingArea;
            // 默认就是画中画小窗；几何在 ApplyDpi 后（已知 DPI/栏高）再算
            Bounds = work;
            WindowState = FormWindowState.Normal;

            _bar = new Panel
            {
                BackColor = Theme.Bar,
                Height = 56
            };
            _bar.Paint += delegate(object sender, PaintEventArgs e) { PaintBar(e.Graphics); };

            _title = new Label
            {
                Text = "cua child session",
                AutoSize = true,
                ForeColor = Theme.Text,
                BackColor = Color.Transparent,
                Font = Theme.Title(11f)
            };
            _meta = new Label
            {
                Text = "connecting...",
                AutoSize = false,
                AutoEllipsis = true,
                TextAlign = ContentAlignment.MiddleLeft,
                ForeColor = Theme.Muted,
                BackColor = Color.Transparent,
                Font = Theme.Ui(9f)
            };
            _statusPill = new StatusPill();
            _statusPill.SetState("只读观察 · 键鼠已屏蔽", Theme.Warn);

            _takeover = new FluentButton { Text = "接管控制", Accent = true };
            _release = new FluentButton { Text = "释放控制", Accent = false, Visible = false };
            _settings = new FluentButton { Text = "设置", Accent = false };
            _edge = new FluentButton { Text = "浏览器", Accent = false };
            _mode = new FluentButton { Text = "观察模式", Accent = false };

            _minimize = new FluentButton
            {
                Text = "\uE921",
                Font = Theme.Icon(8f),
                CornerRadius = 8,
                Ghost = true
            };
            _maximize = new FluentButton
            {
                Text = "\uE922",
                Font = Theme.Icon(8f),
                CornerRadius = 8,
                Ghost = true
            };
            _close = new FluentButton
            {
                Text = "\uE8BB",
                Font = Theme.Icon(8f),
                CornerRadius = 8,
                Ghost = true,
                Danger = true
            };
            _tips = new ToolTip();
            _tips.SetToolTip(_minimize, "最小化到任务栏");
            _tips.SetToolTip(_maximize, "最大化 / 还原（双击画面也可以）");
            _tips.SetToolTip(_close, "隐藏到托盘（子会话继续运行）");
            _tips.SetToolTip(_settings, "子会话设置");
            _tips.SetToolTip(_edge, "在子会话中打开独立浏览器（CuaEdge / CuaChrome）");
            _tips.SetToolTip(_mode, "在 RDP（老式直连）与 VNC（自研抓屏）观察模式间切换");
            _tips.SetToolTip(_statusPill, "画中画：拖动画面移动小窗，拖动四周边缘等比缩放（Shift 自由缩放），双击画面最大化");

            // 无边框窗口：按住标题栏空白处拖动 = 原生移动（含 Aero 贴边）；
            // 双击标题栏 = 最大化/还原。原生拖动会吃掉第二次点击，所以双击
            // 判定要自己做：按下时先比时间，再决定是拖动还是切换最大化。
            foreach (var c in new Control[] { _bar, _title, _meta, _statusPill })
            {
                c.MouseDown += (sdr, ev) =>
                {
                    if (ev.Button != MouseButtons.Left) return;
                    if (_lastCaptionDown != DateTime.MinValue &&
                        (DateTime.UtcNow - _lastCaptionDown).TotalMilliseconds <=
                        SystemInformation.DoubleClickTime)
                    {
                        _lastCaptionDown = DateTime.MinValue;
                        ToggleMaximize();
                        return;
                    }
                    _lastCaptionDown = DateTime.UtcNow;
                    Native.ReleaseCapture();
                    Native.SendMessage(Handle, 0xA1 /* WM_NCLBUTTONDOWN */, (IntPtr)2 /* HTCAPTION */, IntPtr.Zero);
                };
            }
            // 双击标题栏的最大化/还原走 WndProc 的 WM_NCLBUTTONDBLCLK：
            // 按住标题栏已经被转成原生拖动，WinForms 收不到第二次点击。

            // 统一紧凑尺寸：同宽同高、小一号字体、圆角收紧
            // 尺寸统一交给 LayoutBar（规避 DPI 自动缩放不一致）；这里只设字体
            foreach (var b in new FluentButton[] { _takeover, _release, _settings, _edge, _mode })
            {
                b.Font = Theme.Ui(8f);
            }
            BuildModeMenu();
            _mode.Click += delegate
            {
                _modeMenu.Show(_mode, new Point(0, _mode.Height + 2));
            };

            _takeover.Click += delegate { SetViewOnly(false); };
            _release.Click += delegate { SetViewOnly(true); };
            _settings.Click += delegate { OpenSettings(); };
            _edge.Click += delegate { OpenChildEdge(); };
            _minimize.Click += delegate { MinimizeViewer(); };
            _maximize.Click += delegate { ToggleMaximize(); };
            _close.Click += delegate { HideViewer(); };

            _bar.Controls.Add(_title);
            _bar.Controls.Add(_statusPill);
            _bar.Controls.Add(_meta);
            _bar.Controls.Add(_settings);
            _bar.Controls.Add(_edge);
            _bar.Controls.Add(_mode);
            _bar.Controls.Add(_takeover);
            _bar.Controls.Add(_release);
            _bar.Controls.Add(_minimize);
            _bar.Controls.Add(_maximize);
            _bar.Controls.Add(_close);
            _bar.Resize += delegate { LayoutBar(); };

            _content = new Panel { BackColor = Color.Black };
            _rdp = new SessionRdp();
            _rdp.Dock = DockStyle.None;
            _content.Controls.Add(_rdp);
            Controls.Add(_content);
            Controls.Add(_bar);

            _poll = new System.Windows.Forms.Timer { Interval = 300 };
            _poll.Tick += delegate { Poll(); };

            Load += delegate
            {
                ApplyDpi();
                AttachCursorRelays();
                Rectangle? saved = LoadViewerBounds();
                if (saved.HasValue) _pipBounds = saved.Value;
                if (_pipBounds.IsEmpty) _pipBounds = DefaultPipBounds(Screen.PrimaryScreen.WorkingArea);
                if (!_isMaximized && _opt.View) Bounds = _pipBounds;
                LayoutShell();
                LayoutBar();
                InitTray();
                try { StartupQuarantine.RestoreStale(_opt.DataDir); } catch { }
                StartPipe();
                // Windows 11 hides new tray icons in the overflow area; say once
                // where the viewer went so "no icon" is not a mystery. Never let
                // a notification failure abort startup (it runs before StartPipe).
                if (!_opt.Preview && !_trayTipShown && _tray != null)
                {
                    _trayTipShown = true;
                    try
                    {
                        _tray.ShowBalloonTip(5000, "cua 子会话",
                            "关掉观察窗只会隐藏到托盘（Windows 11 在 ^ 溢出区），子会话继续运行；双击托盘图标可恢复。",
                            ToolTipIcon.Info);
                    }
                    catch { }
                }
                if (_opt.Preview)
                {
                    _connected = true;
                    _statusPill.SetState("预览模式", Theme.Accent);
                    _meta.Text = "no child session";
                    if (_opt.View) ShowViewer(true);
                    return;
                }
                if (_opt.View)
                {
                    ShowViewer(!_opt.Interactive);
                    _viewerInitialized = true;
                }
                BeginInvoke(new Action(delegate { Connect(true); }));
            };
            Resize += delegate
            {
                LayoutShell();
                LayoutBar();
                UpdateWindowGlyph();
                ApplyTopMost();
            };
            // 边框热区的缩放光标在 WndProc 的 WM_SETCURSOR 里给：只改当前光标，
            // 不改 Form.Cursor 属性——否则子控件会继承成拖拽光标，进画面还粘着。
            FormClosing += OnFormClosing;
            _filter = new PipFilter(this);
            PipLog("ctor filter registered debug=" + PipDebug);
            Application.AddMessageFilter(_filter);
            UpdateButtons();
        }

        protected override void Dispose(bool disposing)
        {
            if (disposing && _filter != null)
            {
                try { Application.RemoveMessageFilter(_filter); } catch { }
            }
            base.Dispose(disposing);
        }

        protected override void OnMouseCaptureChanged(EventArgs e)
        {
            base.OnMouseCaptureChanged(e);
            // Alt+Tab 等抢走捕获时不要让手势卡住
            if (_gesture != PipGesture.None && !Capture) EndGesture();
        }

        protected override void WndProc(ref Message m)
        {
            // 标题栏按住是原生拖动（HTCAPTION），双击消息到不了 WinForms，
            // 这里补上"双击标题栏 = 最大化/还原"。
            if (m.Msg == WmNcLButtonDblClk && m.WParam.ToInt32() == HtCaption)
            {
                ToggleMaximize();
                return;
            }
            // 只有边框热区需要拖拽光标；画面内部一律交回子控件的默认光标
            if (m.Msg == WmSetCursor && m.WParam == Handle)
            {
                Cursor target;
                if (_gesture != PipGesture.None)
                    target = GestureCursor(_gesture, _gestureZone);
                else
                {
                    PipZone zone = ZoneOf(PointToClient(Cursor.Position));
                    target = zone == PipZone.None ? Cursors.Default : GestureCursor(PipGesture.Resize, zone);
                }
                Cursor.Current = target;
                m.Result = (IntPtr)1;
                return;
            }
            base.WndProc(ref m);
        }

        protected override CreateParams CreateParams
        {
            get
            {
                CreateParams cp = base.CreateParams;
                cp.Style |= WsMinimizeBox;
                return cp;
            }
        }

        private void ApplyDpi()
        {
            int dpi = (int)Native.GetDpiForWindow(Handle);
            if (dpi <= 0) dpi = 96;
            _uiScale = Math.Max(1f, dpi / 96f);
            _scalePercent = Math.Max(100, Math.Min(500, (int)Math.Round(dpi * 100.0 / 96.0)));
            _grip = Math.Max(5, (int)Math.Round(6 * _uiScale));
            ApplyChrome();

            int buttonHeight = Math.Max(30, (int)Math.Round(34 * _uiScale));
            _takeover.Size = new Size(Math.Max(96, (int)Math.Round(120 * _uiScale)), buttonHeight);
            _release.Size = _takeover.Size;
            _settings.Size = new Size(Math.Max(56, (int)Math.Round(68 * _uiScale)), buttonHeight);
            int iconWidth = Math.Max(32, (int)Math.Round(38 * _uiScale));
            _minimize.Size = new Size(iconWidth, buttonHeight);
            _maximize.Size = _minimize.Size;
            _close.Size = _minimize.Size;
            Native.ApplyDarkWindow(Handle);
        }

        /// <summary>给盖住边框热区的子窗口挂光标中继（句柄重建后要重挂）。</summary>
        private void AttachCursorRelays()
        {
            if (_relays.Count > 0) return;
            foreach (Control c in new Control[] { _bar, _content, _rdp })
                AttachCursorRelay(c);
        }

        private void AttachCursorRelay(Control control)
        {
            if (control == null) return;
            try
            {
                if (!control.IsHandleCreated) { IntPtr force = control.Handle; GC.KeepAlive(force); }
                var relay = new CursorRelay(this);
                relay.AssignHandle(control.Handle);
                _relays.Add(relay);
            }
            catch { }
        }

        private void ApplyChrome()
        {
            _bar.Height = Math.Max(30, (int)Math.Round((_isMaximized ? 46 : 34) * _uiScale));
            _statusPill.Height = Math.Max(20, (int)Math.Round((_isMaximized ? 28 : 22) * _uiScale));
            _statusPill.Font = Theme.Ui(_isMaximized ? 9f : 8.5f, FontStyle.Bold);
            _statusPill.RefreshSize();
        }

        private void LayoutShell()
        {
            if (_bar == null || _content == null) return;
            int barHeight = _bar.Height;
            _bar.Bounds = new Rectangle(0, 0, ClientSize.Width, barHeight);
            int contentHeight = Math.Max(0, ClientSize.Height - barHeight);
            _content.Bounds = new Rectangle(0, barHeight, ClientSize.Width, contentHeight);
            LayoutContent();
        }

        private void LayoutContent()
        {
            if (_content == null || _rdp == null) return;
            Rectangle area = _content.ClientRectangle;
            if (area.Width <= 0 || area.Height <= 0) return;
            if (_opt.Width > 0 && _opt.Height > 0)
            {
                double aspect = (double)_opt.Width / _opt.Height;
                int width = area.Width;
                int height = (int)Math.Round(width / aspect);
                if (height > area.Height)
                {
                    height = area.Height;
                    width = (int)Math.Round(height * aspect);
                }
                width = Math.Max(1, width);
                height = Math.Max(1, height);
                _rdp.Bounds = new Rectangle(
                    area.X + (area.Width - width) / 2,
                    area.Y + (area.Height - height) / 2,
                    width,
                    height);
                if (_rfbCtl != null) _rfbCtl.Bounds = _rdp.Bounds;
            }
            else
            {
                _rdp.Bounds = area;
                if (_rfbCtl != null) _rfbCtl.Bounds = area;
            }
        }

        private void PaintBar(Graphics g)
        {
            using (var brush = new SolidBrush(Theme.Bar))
                g.FillRectangle(brush, _bar.ClientRectangle);
            using (var pen = new Pen(Theme.Border))
                g.DrawLine(pen, 0, _bar.Height - 1, _bar.Width, _bar.Height - 1);
        }

        private void LayoutBar()
        {
            if (_bar == null || _title == null) return;
            // 统一尺寸必须在布局时强制覆写：控件在构造期设置的 Size 会被
            // WinForms 按 DPI 逐个自动缩放，加入父容器的时机不同导致高低不一。
            int ubw = 0;
            foreach (var b in new FluentButton[] { _takeover, _release, _settings, _edge, _mode })
            {
                ubw = Math.Max(ubw, TextRenderer.MeasureText(b.Text, b.Font).Width + 14);
            }
            int ubh = Math.Min((int)Math.Round(25 * _uiScale),
                _bar.Height - (int)Math.Round(8 * _uiScale));
            ubh = Math.Max(18, ubh);
            int wbw = (int)Math.Round(ubh * 1.15); // 左右钮（窗口控制）更窄
            foreach (var b in new FluentButton[] { _takeover, _release, _settings, _edge, _mode })
            {
                b.Size = new Size(ubw, ubh);
            }
            foreach (var b in new FluentButton[] { _minimize, _maximize, _close })
            {
                b.Size = new Size(wbw, ubh);
            }
            int pad = Math.Max(8, (int)Math.Round(10 * _uiScale));
            int gap = Math.Max(2, (int)Math.Round(4 * _uiScale));
            int barWidth = _bar.ClientSize.Width;

            // 画中画：标题栏只留必要的东西（状态、接管、窗口按钮）。
            // 最大化时按宽度分层放回次要控件，接管/窗口按钮永不挤掉。
            bool pip = !_isMaximized;
            _title.Visible = !pip && barWidth >= (int)Math.Round(330 * _uiScale);
            _meta.Visible = !pip && barWidth >= (int)Math.Round(700 * _uiScale);
            _edge.Visible = !pip && barWidth >= (int)Math.Round(880 * _uiScale);
            _mode.Visible = _edge.Visible;
            _settings.Visible = !pip && barWidth >= (int)Math.Round(700 * _uiScale);
            _statusPill.Visible = barWidth >= (int)Math.Round(430 * _uiScale);

            int buttonTop = Math.Max(0, (_bar.Height - _takeover.Height) / 2);
            int right = barWidth - pad;
            var stack = new FluentButton[]
            {
                _close, _maximize, _minimize, _mode,
                _viewOnly ? _takeover : _release, _settings, _edge
            };
            foreach (var b in stack)
            {
                if (!b.Visible) continue;
                b.Left = right - b.Width;
                b.Top = buttonTop;
                right = b.Left - gap;
            }

            int left = pad;
            int titleTop = Math.Max(0, (_bar.Height - _title.Height) / 2);
            int pillTop = Math.Max(0, (_bar.Height - _statusPill.Height) / 2);
            // 左侧状态胶囊：放不下就让位给按钮，绝不叠压
            if (_statusPill.Visible && _statusPill.Width + gap > right - pad)
                _statusPill.Visible = false;
            if (_title.Visible)
            {
                _title.Left = left;
                _title.Top = titleTop;
                left = _title.Right + gap;
            }
            if (_statusPill.Visible)
            {
                _statusPill.Left = left;
                _statusPill.Top = pillTop;
                left = _statusPill.Right + gap;
            }
            if (_meta.Visible)
            {
                _meta.Left = left;
                _meta.Top = titleTop;
                _meta.Height = _title.Height;
                _meta.Width = Math.Max(0, right - left);
            }
        }

        private void InitTray()
        {
            _trayMenu = new ContextMenuStrip
            {
                ShowImageMargin = false,
                BackColor = Theme.Bar,
                ForeColor = Theme.Text,
                Font = Theme.Ui(9f),
                Padding = new Padding(0, 6, 0, 6),
                Renderer = new TrayMenuRenderer(),
                DropShadowEnabled = true
            };
            _trayMenu.Items.Add(MenuItem("打开观察窗", delegate { ShowViewer(!_viewOnly); }));
            _trayMenu.Items.Add(MenuItem("子会话设置…", OpenSettings));
            _trayMenu.Items.Add(MenuItem("在子会话打开浏览器", OpenChildEdge));
            _trayMenu.Items.Add(MenuItem("观察模式：RDP (子会话)", delegate { SwitchViewMode(false); }));
            _trayMenu.Items.Add(MenuItem("观察模式：VNC (子会话)", delegate { SwitchViewMode(true); }));
            _menuHeadless = new System.Windows.Forms.ToolStripMenuItem("下次启动后自动关闭路由（无头模式）");
            _menuHeadless.Checked = VncHelper.HeadlessAutoDisable;
            _menuHeadless.Click += delegate
            {
                VncHelper.HeadlessAutoDisable = !_menuHeadless.Checked;
                _menuHeadless.Checked = VncHelper.HeadlessAutoDisable;
            };
            _trayMenu.Items.Add(_menuHeadless);
            _trayMenu.Items.Add(new ToolStripSeparator());
            _menuTakeover = MenuItem("接管控制", delegate { ShowViewer(false); });
            _menuRelease = MenuItem("释放控制", delegate { ShowViewer(true); });
            _trayMenu.Items.Add(_menuTakeover);
            _trayMenu.Items.Add(_menuRelease);
            _trayMenu.Items.Add(new ToolStripSeparator());
            _menuStart = MenuItem("启动子会话", delegate { StartFromSettings(); });
            _menuRestart = MenuItem("重新启动子会话…", delegate { RestartFromSettings(); });
            _menuLogoff = MenuItem("注销子会话…", delegate { LogoffFromSettings(); });
            _menuExit = MenuItem("注销并退出观察器…", delegate { ExitFromSettings(); });
            _trayMenu.Items.Add(_menuStart);
            _trayMenu.Items.Add(_menuRestart);
            _trayMenu.Items.Add(_menuLogoff);
            _trayMenu.Items.Add(new ToolStripSeparator());
            _trayMenu.Items.Add(_menuExit);
            _trayMenu.Opening += delegate { UpdateTrayMenu(); };
            foreach (ToolStripItem item in _trayMenu.Items)
            {
                if (item is ToolStripSeparator)
                    item.Margin = new Padding(0, 3, 0, 3);
                else
                {
                    item.Padding = new Padding(16, 5, 16, 5);
                    ((ToolStripMenuItem)item).TextAlign = ContentAlignment.MiddleCenter;
                }
            }
            _trayMenu.Opened += delegate { TrayMenuRenderer.RoundCorners(_trayMenu, 8); };
            _trayMenu.SizeChanged += delegate { TrayMenuRenderer.RoundCorners(_trayMenu, 8); };

            _tray = new NotifyIcon
            {
                Icon = Theme.AppIcon(true),
                Text = "cua child session",
                Visible = true,
                ContextMenuStrip = _trayMenu
            };
            _tray.DoubleClick += delegate { ShowViewer(!_viewOnly); };
            UpdateTrayMenu();
            UpdateTrayText();
            PromoteTrayIcon();
        }

        /// <summary>
        /// The tray icon is the "session is alive" indicator and must be visible
        /// in every mode (viewer shown or hidden). Windows 11 files new icons
        /// into the overflow area; Explorer keeps that per icon under
        /// HKCU\Control Panel\NotifyIconSettings, so mark ours promoted and
        /// re-add it once to apply. Best effort: a locked-down shell just keeps
        /// the icon in the overflow, never breaks startup.
        /// </summary>
        private void PromoteTrayIcon()
        {
            try
            {
                string exe = Process.GetCurrentProcess().MainModule.FileName;
                bool changed = false;
                using (var root = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(
                           @"Control Panel\NotifyIconSettings", true))
                {
                    if (root == null) return;
                    foreach (string name in root.GetSubKeyNames())
                    {
                        using (var sub = root.OpenSubKey(name, true))
                        {
                            if (sub == null) continue;
                            if (!string.Equals(sub.GetValue("ExecutablePath") as string, exe,
                                    StringComparison.OrdinalIgnoreCase))
                                continue;
                            object promoted = sub.GetValue("IsPromoted");
                            if (!(promoted is int) || (int)promoted != 1)
                            {
                                sub.SetValue("IsPromoted", 1, Microsoft.Win32.RegistryValueKind.DWord);
                                changed = true;
                            }
                        }
                    }
                }
                if (changed && _tray != null)
                {
                    // re-add so Explorer re-reads the setting for this icon
                    _tray.Visible = false;
                    _tray.Visible = true;
                }
            }
            catch { }
        }

        private static ToolStripMenuItem MenuItem(string text, Action action)
        {
            var item = new ToolStripMenuItem(text);
            item.Click += delegate { action(); };
            return item;
        }

        private void UpdateTrayMenu()
        {
            if (_trayMenu == null) return;
            _menuTakeover.Enabled = _viewOnly;
            _menuRelease.Enabled = !_viewOnly;
            _menuStart.Enabled = !SessionConnected;
            _menuRestart.Enabled = SessionConnected;
            _menuLogoff.Enabled = SessionConnected;
            _menuExit.Enabled = SessionConnected;
        }

        private void UpdateTrayText()
        {
            if (_tray == null) return;
            string id = _childId.HasValue ? _childId.Value.ToString() : "—";
            _tray.Text = "cua child session · session " + id +
                         (_viewOnly ? " · view only" : " · user control");
        }

        private void StartPipe()
        {
            try
            {
                _pipe = new HostPipe(_opt.HostPipe, this);
                _pipe.Start();
            }
            catch (Exception ex)
            {
                Fail("host pipe: " + ex.GetBaseException().Message);
            }
        }

        private void Connect(bool initial)
        {
            if (_connected) return;
            try
            {
                if (!ChildSessionApi.IsEnabled())
                    throw new InvalidOperationException(
                        "Child Sessions are disabled. Run `cua session setup` as administrator once, then sign out and back in.");
                bool fromPolicy;
                if (ChildSessionApi.IsRemoteDesktopDisabled(out fromPolicy))
                    throw new InvalidOperationException(
                        "Remote Desktop host is disabled" + (fromPolicy ? " by policy" : "") +
                        ". Enable Remote Desktop in Settings > System > Remote Desktop.");
                if (ChildSessionApi.IsRdpWrapperInstalled())
                    throw new InvalidOperationException(
                        "RDP Wrapper is installed. It is incompatible with Windows Child Sessions; uninstall it first.");

                try { _credential = CredentialStore.Load(); }
                catch (Exception ex) { SetError("saved credential is unreadable: " + ex.GetBaseException().Message); }

                if (!_opt.KeepStartup)
                {
                    try
                    {
                        _startup = new StartupQuarantine(_opt.DataDir);
                        _startup.Activate();
                        _startupReleased = false;
                        _startupReleaseAt = DateTime.MinValue;
                    }
                    catch (Exception ex)
                    {
                        _startup = null;
                        _startupReleased = true;
                        _lastError = "startup quarantine failed: " + ex.GetBaseException().Message;
                    }
                }
                else
                {
                    _startup = null;
                    _startupReleased = true;
                }

                Size size = SetChildSize(ChildSize());
                UpdateMeta();
                _statusPill.SetState("正在创建子会话", Theme.Accent);
                LayoutBar();
                _rdp.Connect(size, _scalePercent, _credential);
                _poll.Start();
            }
            catch (Exception ex)
            {
                string message = "start child session failed: " + ex.GetBaseException().Message;
                if (initial) Fail(message);
                else
                {
                    SetError(message);
                    _statusPill.SetState("启动失败", Theme.Warn);
                    UpdateMeta();
                    WriteState();
                }
            }
        }

        /// <summary>
        /// 子会话桌面尺寸一旦确定，观察窗就按这个宽高比缩放（画中画留黑边），
        /// 所以它不跟随画中画小窗，仍取整块工作区：远端分辨率不因窗口变小而降级。
        /// </summary>
        private Size ChildSize()
        {
            if (_opt.ExplicitSize)
                return EvenSize(_opt.Width, _opt.Height);

            Rectangle work = Screen.PrimaryScreen.WorkingArea;
            return EvenSize(work.Width, work.Height - _bar.Height);
        }

        /// <summary>记录子会话桌面尺寸并重排内容（比例由它决定）。</summary>
        private Size SetChildSize(Size size)
        {
            _opt.Width = size.Width;
            _opt.Height = size.Height;
            if (size.Width > 0 && size.Height > 0)
                _aspect = (double)size.Width / size.Height;
            LayoutContent();
            return size;
        }

        private static Size EvenSize(int width, int height)
        {
            width = Math.Max(640, Math.Min(8192, width));
            height = Math.Max(480, Math.Min(8192, height));
            if ((width & 1) != 0) width--;
            if ((height & 1) != 0) height--;
            return new Size(width, height);
        }

        private void Poll()
        {
            if (_stopping) return;
            // The tray icon means "this child session is alive" and must never
            // stay hidden, whether the viewer is showing or hidden.
            if (_tray != null && !_tray.Visible)
            {
                try { _tray.Visible = true; } catch { }
            }
            int state = _rdp.Connected;
            if (state == 2)
            {
                _statusPill.SetState("正在连接子会话", Theme.Accent);
                return;
            }
            if (state == 0)
            {
                if (_connected)
                {
                    uint? lastSession = _childId;
                    _lastChildId = lastSession ?? _lastChildId;
                    _connected = false;
                    _childId = null;
                    _agentPipe = null;
                    _agentReady = false;
                    _agentTried = false;
                    _agentAttempts = 0;
                    _agentRetryAt = DateTime.MinValue;
                    _viewerInitialized = false;
                    _readyStatus = false;
                    _statusPill.SetState("子会话已断开", Theme.Warn);
                    UpdateMeta();
                    WriteState();
                    if (!_opt.Preview && lastSession.HasValue &&
                        !ChildSessionApi.SessionExists(lastSession.Value))
                        StopSession();
                    // Self-heal: the WTS session may still exist while the viewer
                    // link dropped (e.g. the routing flag was flipped after
                    // creation) — schedule a full reconnect.
                    _reconnectAt = DateTime.UtcNow.AddSeconds(10);
                    // The session and its agent are still alive — keep the state
                    // fields so `cua child`/`cua vnc` keep working even though the
                    // viewer link is down.
                    if (lastSession.HasValue && ChildSessionApi.SessionExists(lastSession.Value))
                    {
                        _childId = lastSession;
                        _agentPipe = "cua-child-agent-" + lastSession.Value;
                    }
                }
                // Self-heal reconnect (only while the link is down): RDP resumes
                // the disconnected same-user session and the normal flow
                // re-attaches the agent.
                uint? healId = _lastChildId ?? ChildSessionApi.TryGetId();
                if (!_opt.Preview && DateTime.UtcNow >= _reconnectAt &&
                    healId.HasValue && ChildSessionApi.SessionExists(healId.Value))
                {
                    _reconnectAttempts++;
                    // 路由开关关闭后 ConnectToChildSession 机制整体失效——重连不可
                    // 能成功。3 次失败后自动切到 VNC 观察方式并停止重试。
                    if (_reconnectAttempts >= 3 && !_plainTried)
                    {
                        // 标准 RDP（不带子会话属性）重连：恢复断开的同用户子会话
                        // 显示器——RDP 优先恢复同用户断开会话，不影响控制台。
                        _plainTried = true;
                        _statusPill.SetState("标准 RDP 重连子会话", Theme.Accent);
                        try
                        {
                            Size size = SetChildSize(ChildSize());
                            _rdp.Connect(size, _scalePercent, _credential, false);
                        }
                        catch { }
                        _reconnectAt = DateTime.UtcNow.AddSeconds(15);
                        return;
                    }
                    if (_reconnectAttempts >= 3 && _plainTried)
                    {
                        // 标准 RDP 重连也会终结子会话（单会话策略）——
                        // 无显示器状态下无画面可看；任务与自动化继续运行。
                        _reconnectAt = DateTime.MaxValue;
                        _statusPill.SetState("子会话已无显示器（无头运行）", Theme.Warn);
                        return;
                    }
                    _reconnectAt = DateTime.UtcNow.AddSeconds(15);
                    _statusPill.SetState("重连子会话", Theme.Accent);
                    try
                    {
                        Size size = SetChildSize(ChildSize());
                        _rdp.Connect(size, _scalePercent, _credential, !_plainTried);
                        _lastError = null;
                        WriteState();
                    }
                    catch (Exception ex)
                    {
                        _lastError = "reconnect: " + ex.GetBaseException().Message;
                    }
                }
                return;
            }


            if (!_connected)
            {
                _connected = true;
                _reconnectAt = DateTime.MinValue;
                _reconnectAttempts = 0;
                _plainTried = false;
                _childId = null;
                _agentPipe = null;
                _agentReady = false;
                _agentTried = false;
                _agentAttempts = 0;
                _agentRetryAt = DateTime.MinValue;
                _viewerInitialized = false;
                _readyStatus = false;
            }

            // The RDP client can report Connected before Windows publishes the
            // child session id. Keep polling instead of dereferencing a null id.
            if (!_childId.HasValue)
            {
                // 路由开关关闭后 WTSGetChildSessionId 不再返回子会话 id，
                // 因此重连时优先使用断开前记住的 id。
                _childId = _lastChildId ?? ChildSessionApi.TryGetId();
                if (!_childId.HasValue)
                {
                    _statusPill.SetState("已连接 · 等待子会话", Theme.Accent);
                    UpdateMeta();
                    WriteState();
                    return;
                }
                _agentPipe = "cua-child-agent-" + _childId.Value;
                UpdateMeta();
            }

            if (!_viewerInitialized)
            {
                _viewerInitialized = true;
                if (_opt.View) ShowViewer(!_opt.Interactive);
                else HideViewer();
            }

            if (!_agentReady && !_agentTried && DateTime.UtcNow >= _agentRetryAt)
            {
                _agentTried = true;
                _agentAttempts++;
                _statusPill.SetState("启动子会话 agent", Theme.Accent);
                bool launched = false;
                if (AgentPipeAlive(_agentPipe))
                {
                    // A reconnect reuses a session whose previous agent survived
                    // the viewer drop — its pipe is already serving; do not
                    // launch a second one (CreateNamedPipeW would collide).
                    _agentReady = true;
                    _lastError = null;
                    launched = true;
                }
                else
                {
                    // Stale agents from before the drop would own the pipe name
                    // and break the fresh agent — clear them first.
                    foreach (var pr in System.Diagnostics.Process.GetProcessesByName("cua"))
                    {
                        try { if (pr.SessionId == _childId.Value) pr.Kill(); } catch { }
                    }
                    try
                    {
                        TaskLauncher.Launch(_childId.Value, _opt.CuaExe, _opt.DataDir, _opt.DataDir);
                        launched = true;
                    }
                    catch (Exception ex)
                    {
                        // The child session can be published before Task Scheduler
                        // accepts RunEx for it. Retry briefly instead of failing the seat.
                        if (_agentAttempts < 20)
                        {
                            _agentTried = false;
                            _agentRetryAt = DateTime.UtcNow.AddMilliseconds(500);
                            _lastError = "agent launch pending: " + ex.GetBaseException().Message;
                        }
                        else
                        {
                            SetError("agent launch failed: " + ex.GetBaseException().Message);
                        }
                    }
                }

                if (launched)
                {
                    _agentReady = WaitForAgent(_agentPipe);
                    if (_agentReady)
                    {
                        _lastError = null;
                    }
                    else if (_agentAttempts < 6)
                    {
                        _agentTried = false;
                        _agentRetryAt = DateTime.UtcNow.AddMilliseconds(800);
                        _lastError = "agent did not answer; retrying " + _agentPipe;
                    }
                    else
                    {
                        SetError("agent did not answer on " + _agentPipe);
                    }
                }
                UpdateMeta();
                WriteState();
            }

            if (_agentReady)
            {
                if (!_readyStatus)
                {
                    _readyStatus = true;
                    SetViewOnly(_viewOnly);
                    UpdateButtons();
                    if (!_startupReleased) _startupReleaseAt = DateTime.UtcNow.AddSeconds(20);
                }
            }
            else
            {
                _statusPill.SetState("子会话已连接 · agent 未就绪", Theme.Warn);
            }
            if (!_startupReleased && _startupReleaseAt != DateTime.MinValue &&
                DateTime.UtcNow >= _startupReleaseAt)
                ReleaseStartupQuarantine();
            LayoutBar();
        }

        private void ReleaseStartupQuarantine()
        {
            if (_startup == null || _startupReleased) return;
            try
            {
                _startup.Restore();
                _startupReleased = true;
                _startupReleaseAt = DateTime.MinValue;
                _startup = null;
            }
            catch (Exception ex)
            {
                _lastError = "startup restore failed: " + ex.GetBaseException().Message;
            }
        }

        private bool AgentPipeAlive(string pipeName)
        {
            if (string.IsNullOrEmpty(pipeName)) return false;
            try
            {
                using (var pipe = new System.IO.Pipes.NamedPipeClientStream(".", pipeName,
                    System.IO.Pipes.PipeDirection.InOut))
                {
                    pipe.Connect(300);
                    byte[] request = Encoding.UTF8.GetBytes("{\"op\":\"ping\"}\n");
                    pipe.Write(request, 0, request.Length);
                    pipe.Flush();
                    byte[] response = new byte[4096];
                    int n = pipe.Read(response, 0, response.Length);
                    return n > 0;
                }
            }
            catch
            {
                return false;
            }
        }

        private bool WaitForAgent(string pipeName)
        {
            if (string.IsNullOrEmpty(pipeName)) return false;
            for (int i = 0; i < 40; i++)
            {
                try
                {
                    using (var pipe = new System.IO.Pipes.NamedPipeClientStream(".", pipeName,
                        System.IO.Pipes.PipeDirection.InOut))
                    {
                        pipe.Connect(500);
                        byte[] request = Encoding.UTF8.GetBytes("{\"op\":\"ping\"}\n");
                        pipe.Write(request, 0, request.Length);
                        pipe.Flush();
                        byte[] response = new byte[4096];
                        int n = pipe.Read(response, 0, response.Length);
                        return n > 0;
                    }
                }
                catch
                {
                    Thread.Sleep(250);
                }
            }
            return false;
        }

        /// 子会话内启动 tvnserver（抓子会话桌面），并在主会话打开 viewer。
        /// flag 关闭后观察窗链路已死，这是用户唯一能看见子会话的方式。
        private void StartChildServerAndMainViewer()
        {
            try
            {
                if (!_childId.HasValue || string.IsNullOrEmpty(_agentPipe))
                {
                    SetError("看子会话需要先启动子会话");
                    return;
                }
                VncHelper.EnsureServerInChild(_agentPipe);
                VncHelper.LaunchViewerHere();
                _lastError = null;
            }
            catch (Exception ex)
            {
                SetError("vnc: " + ex.GetBaseException().Message);
            }
        }

        /// 观察窗模式切换：RDP 与 VNC（子会话 RFB）无缝切换。
        /// VNC 模式不依赖路由开关，flag 关闭后的唯一可用观察方式。
        private void BuildModeMenu()
        {
            _modeMenu = new System.Windows.Forms.ContextMenuStrip
            {
                                BackColor = Theme.Bar,
                ForeColor = Theme.Text,
                Font = Theme.Ui(9f),
                ShowImageMargin = false,
                Renderer = new TrayMenuRenderer(),
                DropShadowEnabled = true
            };
            _modeRdpItem = new System.Windows.Forms.ToolStripMenuItem("RDP（ActiveX 直连）")
            {
                Checked = !_vncMode
            };
            _modeRdpItem.Click += delegate { SwitchViewMode(false); };
            _modeVncItem = new System.Windows.Forms.ToolStripMenuItem("VNC（自研抓屏）")
            {
                Checked = _vncMode
            };
            _modeVncItem.Click += delegate { SwitchViewMode(true); };
            _modeMenu.Items.Add(_modeRdpItem);
            _modeMenu.Items.Add(_modeVncItem);
        }

        /// 设置对话框改动后同步托盘菜单的勾选状态
        /// 子会话是否在运行（供设置对话框动态文案/立即生效用）
        /// 主会话 VNC 视图（子会话内的 tvnviewer）是否在运行
        public bool IsChildRunning
        {
            get { return _childId.HasValue; }
        }

        /// 关闭子会话内的主会话 VNC 视图（tvnviewer）

        public void SyncTrayToggleStates()
        {
            if (_menuHeadless != null)
            {
                _menuHeadless.Checked = VncHelper.HeadlessAutoDisable;
            }
        }

        private void SwitchViewMode(bool vnc)
        {
            try
            {
                _vncMode = vnc;
                if (vnc)
                {
                    if (!_childId.HasValue)
                    {
                        SetError("VNC 模式需要先启动子会话");
                        return;
                    }
                    VncHelper.EnsureChildRfbServer(_childId.Value, _opt.CuaExe, _opt.DataDir);
                    if (_rfbCtl == null)
                    {
                        _rfbCtl = new RfbClientControl();
                        _content.Controls.Add(_rfbCtl);
                        AttachCursorRelay(_rfbCtl);   // 贴边后边框热区落在它身上
                    }
                    _rfbCtl.Bounds = _rdp.Bounds;
                    _rfbCtl.Visible = true;
                    _rfbCtl.BringToFront();
                    _rfbCtl.Connect(5901);
                    _rdp.Visible = false;
                    _statusPill.SetState("观察模式：VNC (子会话)", Theme.Accent);
                }
                else
                {
                    _rfbCtl?.Disconnect();
                    _rfbCtl.Visible = false;
                    _rdp.Visible = true;
                    _rdp.BringToFront();
                    _statusPill.SetState("观察模式：RDP", Theme.Accent);
                }
                if (_modeRdpItem != null)
                {
                    _modeRdpItem.Checked = !vnc;
                    _modeVncItem.Checked = vnc;
                }
                UpdateMeta();
                _lastError = null;
            }
            catch (Exception ex)
            {
                SetError("切换观察模式: " + ex.GetBaseException().Message);
            }
        }

        private void ShowViewer(bool viewOnly)
        {
            if (InvokeRequired)
            {
                BeginInvoke(new Action<bool>(ShowViewer), viewOnly);
                return;
            }
            try
            {
                ShowInTaskbar = true;
                Opacity = 1;
                WindowState = FormWindowState.Normal;
                if (_pipBounds.IsEmpty) _pipBounds = DefaultPipBounds(Screen.PrimaryScreen.WorkingArea);
                Bounds = _isMaximized ? Screen.PrimaryScreen.WorkingArea : _pipBounds;
                Show();
                ApplyTopMost();
                LayoutShell();
                LayoutBar();
                Activate();
                SetViewOnly(viewOnly);
                _viewerShown = true;
                WriteState();
            }
            catch (Exception ex)
            {
                SetError("show viewer: " + ex.GetBaseException().Message);
            }
        }

        private void HideViewer()
        {
            if (InvokeRequired)
            {
                BeginInvoke(new Action(HideViewer));
                return;
            }
            Hide();
            ShowInTaskbar = false;
            _viewerShown = false;
            ApplyTopMost();
            WriteState();
            if (!_trayTipShown && _tray != null)
            {
                _trayTipShown = true;
                _tray.ShowBalloonTip(3000, "cua 子会话",
                    "观察窗已隐藏到托盘。子会话仍在运行，双击托盘图标或右键菜单可恢复。",
                    ToolTipIcon.Info);
            }
        }

        private void MinimizeViewer()
        {
            if (!Visible) return;
            WindowState = FormWindowState.Minimized;
        }

        private void ToggleMaximize()
        {
            if (!_isMaximized) _pipBounds = Bounds;   // 记住画中画的当前位置
            _isMaximized = !_isMaximized;
            ApplyChrome();                            // 画中画的标题栏更矮
            if (WindowState == FormWindowState.Minimized)
                WindowState = FormWindowState.Normal;
            if (_isMaximized)
            {
                Bounds = Screen.PrimaryScreen.WorkingArea;
            }
            else
            {
                if (_pipBounds.IsEmpty) _pipBounds = DefaultPipBounds(Screen.PrimaryScreen.WorkingArea);
                Bounds = _pipBounds;
            }
            ApplyTopMost();
            LayoutShell();
            LayoutBar();
            UpdateWindowGlyph();
            if (Visible) Activate();
        }

        /// <summary>画中画悬浮在其他窗口之上；最大化时退回普通窗口。</summary>
        private void ApplyTopMost()
        {
            bool top = Visible && !_isMaximized && WindowState == FormWindowState.Normal;
            if (TopMost != top) TopMost = top;
        }

        private void UpdateWindowGlyph()
        {
            if (_maximize == null) return;
            _maximize.Text = _isMaximized ? "\uE923" : "\uE922";
            _tips?.SetToolTip(_maximize, _isMaximized ? "还原窗口（双击画面）" : "最大化窗口（双击画面）");
        }

        private void SetViewOnly(bool viewOnly)
        {
            if (InvokeRequired)
            {
                BeginInvoke(new Action<bool>(SetViewOnly), viewOnly);
                return;
            }
            _viewOnly = viewOnly;
            try
            {
                // 缩放/重连会让 mstscax 重建隐藏的输入窗口，缓存句柄可能已失效——
                // 只读必须真的挡住输入，所以每次都重新解析再设禁用状态。
                ChildSessionApi.ClearCache(_rdp.Handle);
                _rdp.SetInputBlocked(viewOnly);
            }
            catch { }
            if (viewOnly)
            {
                _statusPill.SetState("只读观察", Theme.Warn);
            }
            else
            {
                _statusPill.SetState("正在控制", Theme.Accent);
                if (Visible)
                {
                    Activate();
                    ReassertInput();
                }
            }
            UpdateButtons();
            LayoutBar();
            WriteState();
        }

        /// <summary>
        /// 接管控制时重新启用 RDP 的 Input Capture Window。窗口一缩放，mstscax
        /// 会重建这个隐藏子窗口；旧句柄上残留的禁用状态正是"只有最大化全屏才点
        /// 得动"的典型症状，所以接管/缩放之后都重新解析并启用一次。
        /// </summary>
        private void ReassertInput()
        {
            if (_viewOnly) return;
            try
            {
                ChildSessionApi.ClearCache(_rdp.Handle);
                _rdp.SetInputBlocked(false);
                _rdp.FocusInput();
            }
            catch { }
        }

        // ---------------- 画中画：默认几何 / 手势 ----------------

        private static readonly bool PipDebug =
            Environment.GetEnvironmentVariable("CUA_PIP_DEBUG") == "1";

        private void PipLog(string text)
        {
            if (!PipDebug) return;
            try
            {
                File.AppendAllText(Path.Combine(_opt.DataDir, "pip-debug.log"),
                    DateTime.Now.ToString("HH:mm:ss.fff") + " " + text + Environment.NewLine);
            }
            catch { }
        }

        /// <summary>默认画中画：右下角一块按子会话比例的小窗（画面贴边，无外框）。</summary>
        private Rectangle DefaultPipBounds(Rectangle work)
        {
            double aspect = _aspect > 0 ? _aspect : 16.0 / 9.0;
            int barHeight = _bar != null
                ? _bar.Height
                : Math.Max(30, (int)Math.Round(34 * _uiScale));
            int width = (int)Math.Round(work.Width * 0.28);
            width = Math.Max(MinPipWidth(), Math.Min(work.Width - 16, width));
            int height = barHeight + Math.Max(1, (int)Math.Round(width / aspect));
            int margin = Math.Max(12, (int)Math.Round(24 * _uiScale));
            return new Rectangle(
                Math.Max(work.Left + margin, work.Right - width - margin),
                Math.Max(work.Top + margin, work.Bottom - height - margin),
                width,
                height);
        }

        /// <summary>再小也要放得下接管按钮和三个窗口按钮。</summary>
        private int MinPipWidth()
        {
            return Math.Max(300, (int)Math.Round(300 * _uiScale));
        }

        /// <summary>画中画位置存盘：下次打开观察窗还在原地，不用每次重新摆。</summary>
        private string ViewerGeometryPath()
        {
            return Path.Combine(_opt.DataDir, "viewer.json");
        }

        private Rectangle? LoadViewerBounds()
        {
            try
            {
                var d = Json.Parse(File.ReadAllText(ViewerGeometryPath()));
                int x = Json.Int(d, "x", int.MinValue);
                int y = Json.Int(d, "y", int.MinValue);
                int width = Json.Int(d, "width", 0);
                int height = Json.Int(d, "height", 0);
                if (width < MinPipWidth() || height < _bar.Height + 120) return null;
                Rectangle screen = SystemInformation.VirtualScreen;
                if (x + width < screen.Left + 96 || x > screen.Right - 96 ||
                    y + height < screen.Top + 48 || y > screen.Bottom - 48)
                    return null;   // 换过显示器/分辨率，旧位置没意义
                return new Rectangle(x, y, width, height);
            }
            catch
            {
                return null;
            }
        }

        private void SaveViewerBounds()
        {
            try
            {
                File.WriteAllText(ViewerGeometryPath(), Json.Serialize(
                    new Dictionary<string, object>
                    {
                        { "x", Bounds.X },
                        { "y", Bounds.Y },
                        { "width", Bounds.Width },
                        { "height", Bounds.Height }
                    }));
            }
            catch { }
        }

        /// <summary>窗口高度：内容按子会话比例算，再加标题栏与边框带。</summary>
        private int HeightForWidth(int width)
        {
            double aspect = _aspect > 0 ? _aspect : 16.0 / 9.0;
            int chromeH = _bar.Height;
            int contentWidth = Math.Max(1, width);
            return chromeH + Math.Max(1, (int)Math.Round(contentWidth / aspect));
        }

        /// <summary>边缘/角落拖拽手势带：整个内容外侧的边框。</summary>
        private PipZone ZoneOf(Point p)
        {
            int grip = Math.Max(4, _grip);
            PipZone zone = PipZone.None;
            if (p.X < grip) zone |= PipZone.Left;
            else if (p.X >= ClientSize.Width - grip) zone |= PipZone.Right;
            if (p.Y < grip) zone |= PipZone.Top;
            else if (p.Y >= ClientSize.Height - grip) zone |= PipZone.Bottom;
            return zone;
        }

        private bool InContent(Point p)
        {
            return _content != null && _content.Visible && _content.Bounds.Contains(p);
        }

        private bool IsDoubleClick(Point cursor)
        {
            DateTime now = DateTime.UtcNow;
            Size tolerance = SystemInformation.DoubleClickSize;
            bool hit = (now - _lastDownAt).TotalMilliseconds <= SystemInformation.DoubleClickTime
                       && Math.Abs(cursor.X - _lastDownPos.X) <= Math.Max(1, tolerance.Width / 2)
                       && Math.Abs(cursor.Y - _lastDownPos.Y) <= Math.Max(1, tolerance.Height / 2);
            _lastDownAt = hit ? DateTime.MinValue : now;
            _lastDownPos = cursor;
            return hit;
        }

        /// <summary>
        /// 在消息分发前处理画中画手势。返回 true = 消息已被吞掉。
        /// 只读观察时画面就是窗口：中心拖动 = 移动，双击 = 最大化；
        /// 边框带任意角落 = 等比缩放。接管控制时全部放行给子会话。
        /// </summary>
        internal bool FilterMessage(ref Message m)
        {
            int msg = m.Msg;
            if (msg != WmLButtonDown && msg != WmLButtonDblClk &&
                msg != WmLButtonUp && msg != WmMouseMove) return false;
            if (_stopping || IsDisposed) return false;
            if (!Visible || !Enabled || !IsHandleCreated)
            {
                if (msg != WmMouseMove) PipLog("skip state visible=" + Visible + " enabled=" + Enabled + " msg=" + msg);
                return false;
            }
            if (m.HWnd != Handle && !Native.IsChild(Handle, m.HWnd))
            {
                if (msg != WmMouseMove) PipLog("skip foreign hwnd=" + m.HWnd + " handle=" + Handle);
                return false;
            }
            if (msg != WmMouseMove) PipLog("msg=" + msg + " hwnd=" + m.HWnd + " self=" + (m.HWnd == Handle));

            Point cursor = Cursor.Position;
            Point p = PointToClient(cursor);
            if (msg != WmMouseMove)
                PipLog("p=" + p.X + "," + p.Y + " content=" + InContent(p) +
                       " zone=" + ZoneOf(p) + " viewOnly=" + _viewOnly + " max=" + _isMaximized);

            if (msg == WmLButtonDown || msg == WmLButtonDblClk)
            {
                if (_gesture != PipGesture.None) return false;
                PipZone zone = ZoneOf(p);
                if (zone != PipZone.None)
                {
                    _lastDownAt = DateTime.MinValue;
                    BeginGesture(PipGesture.Resize, zone, cursor);
                    return true;
                }
                if (InContent(p))
                {
                    if (_viewOnly)
                    {
                        if (IsDoubleClick(cursor))
                        {
                            ToggleMaximize();
                            return true;
                        }
                        if (!_isMaximized)
                        {
                            BeginGesture(PipGesture.Move, PipZone.None, cursor);
                            return true;
                        }
                        return false;
                    }
                    // 接管控制：点击前再启用一次 RDP 输入窗口，防止窗口缩放后点击落空
                    ReassertInput();
                }
                return false;
            }

            if (msg == WmMouseMove)
            {
                if (_gesture == PipGesture.Move)
                {
                    DragTo(cursor);
                    return true;
                }
                if (_gesture == PipGesture.Resize)
                {
                    ResizeTo(cursor);
                    return true;
                }
                UpdateEdgeCursor(p);
                return false;
            }

            if (_gesture != PipGesture.None)
            {
                EndGesture();
                return true;
            }
            return false;
        }

        /// <summary>子控件的 WM_SETCURSOR 转过来：热区给缩放光标，其余返回 false 交还原控件。</summary>
        internal bool RelayCursor(IntPtr hwnd)
        {
            if (_gesture != PipGesture.None) return false;
            PipZone zone = ZoneOf(PointToClient(Cursor.Position));
            if (zone == PipZone.None) return false;
            Cursor.Current = GestureCursor(PipGesture.Resize, zone);
            return true;
        }

        /// <summary>
        /// 边框热区光标：只在进出热区时改一次 Cursor.Current，画面内部立刻还回
        /// 默认箭头（不改 Form.Cursor 属性，子控件才不会继承成拖拽光标）。
        /// </summary>
        private void UpdateEdgeCursor(Point p)
        {
            PipZone zone = ZoneOf(p);
            if (zone == _cursorZone) return;
            bool wasEdge = _cursorZone != PipZone.None;
            _cursorZone = zone;
            if (zone != PipZone.None) Cursor.Current = GestureCursor(PipGesture.Resize, zone);
            else if (wasEdge) Cursor.Current = Cursors.Default;
        }

        private void BeginGesture(PipGesture gesture, PipZone zone, Point cursor)
        {
            PipLog("begin " + gesture + " zone=" + zone + " cursor=" + cursor + " bounds=" + Bounds);
            try { if (!Focused) Activate(); } catch { }
            _gesture = gesture;
            _gestureZone = zone;
            _gestureStart = Bounds;
            _gestureCursor = cursor;
            _cursorZone = zone;
            Cursor.Current = GestureCursor(gesture, zone);
            try { Capture = true; } catch { }
        }

        private void EndGesture()
        {
            if (_gesture == PipGesture.None) return;
            _gesture = PipGesture.None;
            _gestureZone = PipZone.None;
            _cursorZone = PipZone.None;
            try { Capture = false; } catch { }
            Cursor.Current = Cursors.Default;
            PipLog("end bounds=" + Bounds);
            if (!_isMaximized)
            {
                _pipBounds = Bounds;
                SaveViewerBounds();
                ReassertInput();      // 缩放后 mstscax 可能重建了输入窗口
            }
            WriteState();
        }

        private void DragTo(Point cursor)
        {
            Rectangle b = _gestureStart;
            b.X += cursor.X - _gestureCursor.X;
            b.Y += cursor.Y - _gestureCursor.Y;
            Bounds = ClampToScreen(b);
        }

        /// <summary>等比缩放：宽度是唯一自由度，高度由子会话比例推出。</summary>
        private void ResizeTo(Point cursor)
        {
            Rectangle s = _gestureStart;
            PipZone zone = _gestureZone;
            bool hasLeft = (zone & PipZone.Left) != 0;
            bool hasRight = (zone & PipZone.Right) != 0;
            bool hasTop = (zone & PipZone.Top) != 0;
            bool hasBottom = (zone & PipZone.Bottom) != 0;

            int dx = cursor.X - _gestureCursor.X;
            int dy = cursor.Y - _gestureCursor.Y;
            int stepX = hasLeft ? -dx : hasRight ? dx : 0;
            int stepY = hasTop ? -dy : hasBottom ? dy : 0;

            double aspect = _aspect > 0 ? _aspect : 16.0 / 9.0;
            int chromeH = _bar.Height;
            int minW = MinPipWidth();
            int minH = chromeH + 120;
            int maxW = Math.Max(minW, Math.Min(SystemInformation.VirtualScreen.Width,
                (int)Math.Round((SystemInformation.VirtualScreen.Height - chromeH) * aspect)));
            int maxH = Math.Max(minH, SystemInformation.VirtualScreen.Height);

            int width;
            int height;
            if ((Control.ModifierKeys & Keys.Shift) != 0)
            {
                // Shift：自由缩放，画面按比例居中、多出来的部分留黑边
                width = Math.Max(minW, Math.Min(maxW, s.Width + stepX));
                height = Math.Max(minH, Math.Min(maxH, s.Height + stepY));
            }
            else
            {
                if (stepX != 0 && (stepY == 0 || Math.Abs(stepX) >= Math.Abs((int)Math.Round(stepY * aspect))))
                    width = s.Width + stepX;                       // 横向拖动主导
                else
                    width = (int)Math.Round((s.Height + stepY - chromeH) * aspect);
                width = Math.Max(minW, Math.Min(maxW, width));
                height = HeightForWidth(width);
            }

            int x = hasLeft ? s.Right - width
                : hasRight ? s.Left
                : s.Left + (s.Width - width) / 2;   // 纯上下拖动：水平居中
            int y = hasTop ? s.Bottom - height
                : s.Top;                            // 左右拖动时标题栏不跟着上下跳
            Bounds = new Rectangle(x, y, width, height);
        }

        private static Rectangle ClampToScreen(Rectangle b)
        {
            Rectangle v = SystemInformation.VirtualScreen;
            const int keep = 96;   // 至少留一条可抓回来的边
            if (b.Right < v.Left + keep) b.X = v.Left + keep - b.Width;
            if (b.Left > v.Right - keep) b.X = v.Right - keep;
            if (b.Bottom < v.Top + keep) b.Y = v.Top + keep - b.Height;
            if (b.Top > v.Bottom - keep) b.Y = v.Bottom - keep;
            return b;
        }

        private static Cursor GestureCursor(PipGesture gesture, PipZone zone)
        {
            if (gesture == PipGesture.Move) return Cursors.SizeAll;
            bool left = (zone & PipZone.Left) != 0;
            bool right = (zone & PipZone.Right) != 0;
            bool top = (zone & PipZone.Top) != 0;
            bool bottom = (zone & PipZone.Bottom) != 0;
            if ((left && top) || (right && bottom)) return Cursors.SizeNWSE;
            if ((right && top) || (left && bottom)) return Cursors.SizeNESW;
            if (left || right) return Cursors.SizeWE;
            if (top || bottom) return Cursors.SizeNS;
            return Cursors.Default;
        }

        private void UpdateButtons()
        {
            _takeover.Visible = _viewOnly;
            _release.Visible = !_viewOnly;
            _takeover.Enabled = _viewOnly;
            _release.Enabled = !_viewOnly;
            _edge.Enabled = SessionConnected;
            LayoutBar();
            UpdateTrayMenu();
            UpdateTrayText();
        }

        /// 托盘菜单文案随会话状态动态变化
        private void SyncTrayTexts()
        {
            if (_menuStart == null) return;
            bool running = _childId.HasValue;
            _menuStart.Text = running
                ? "子会话运行中（session " + _childId.Value + "）"
                : "启动子会话";
            _menuStart.Enabled = !running;
            _menuRestart.Enabled = running;

        }

        private void UpdateMeta()
        {
            string id = _childId.HasValue ? _childId.Value.ToString() : "—";
            string cred = _credential != null ? " · 已保存凭据" : " · 自动登录";
            string startup = (_startup != null && !_startupReleased) ? " · 启动项隔离" : "";
            _meta.Text = "session " + id + "  ·  " + _opt.Width + "×" + _opt.Height +
                         "  ·  " + _scalePercent + "%" + cred + startup;
            LayoutBar();
            UpdateTrayText();
            SyncTrayTexts();
        }

        internal bool SessionConnected
        {
            get { return _connected && _childId.HasValue; }
        }

        internal bool ViewOnly
        {
            get { return _viewOnly; }
        }

        internal string SessionSummary
        {
            get
            {
                string id = _childId.HasValue ? _childId.Value.ToString() : "—";
                return "Session " + id + " · " + _opt.Width + "×" + _opt.Height +
                       " · " + _scalePercent + "% · " +
                       (_viewOnly ? "只读观察" : "用户控制");
            }
        }

        internal string CredentialSummary
        {
            get
            {
                return _credential != null
                    ? "凭据：Windows Credential Manager（" + _credential.UserName + "）"
                    : "凭据：Windows 自动登录（PIN/Hello 账户建议运行 `cua session login`）";
            }
        }

        internal bool AgentReady
        {
            get { return _agentReady; }
        }

        internal string EdgeSummary
        {
            get
            {
                var d = MergedEdgeStatus();
                string profile = Json.Str(d, "profile");
                if (string.IsNullOrEmpty(profile))
                    profile = Path.Combine(_opt.DataDir, "browser", ProfileKindName(d));
                bool exists = Directory.Exists(profile);
                string exe = Json.Str(d, "browser_exe");
                if (string.IsNullOrEmpty(exe)) exe = Json.Str(d, "edge_exe");
                string kindLabel = string.Equals(Json.Str(d, "browser"), "chrome", StringComparison.OrdinalIgnoreCase) ? "Chrome" : "Edge";
                string exeName = kindLabel == "Chrome" ? "chrome.exe" : "msedge.exe";
                string edge;
                if (string.IsNullOrEmpty(exe))
                {
                    edge = "未安装";
                }
                else
                {
                    string version = null;
                    try
                    {
                        if (File.Exists(exe))
                            version = FileVersionInfo.GetVersionInfo(exe).FileVersion;
                    }
                    catch { }
                    edge = string.IsNullOrEmpty(version) ? exeName + "（版本未知）" : kindLabel + " " + version;
                }
                string cdp = "未开启";
                object cdpObj;
                if (d.TryGetValue("cdp", out cdpObj) && cdpObj is Dictionary<string, object>)
                {
                    int port = Json.Int((Dictionary<string, object>)cdpObj, "port", 0);
                    if (port > 0) cdp = "127.0.0.1:" + port;
                }
                return kindLabel + " profile：" + profile + (exists ? "" : "（未创建）") + Environment.NewLine +
                       "版本：" + edge + Environment.NewLine +
                       "状态：" + EdgeStateText(d) + Environment.NewLine +
                       "CDP：" + cdp + "（仅浏览器运行时）";
            }
        }

        private static string ProfileKindName(Dictionary<string, object> d)
        {
            string kind = Json.Str(d, "profile_kind");
            if (!string.IsNullOrEmpty(kind)) return kind;
            return string.Equals(Json.Str(d, "browser"), "chrome", StringComparison.OrdinalIgnoreCase)
                ? "CuaChrome" : "CuaEdge";
        }

        private static string EdgeStateText(Dictionary<string, object> d)
        {
            string lockfile = Json.Str(d, "lockfile");
            object holdersObj;
            if (d.TryGetValue("holders", out holdersObj) && holdersObj is IList)
            {
                var holders = (IList)holdersObj;
                if (holders.Count > 0)
                {
                    var first = holders[0] as Dictionary<string, object>;
                    string text = "被占用";
                    if (first != null)
                        text += "：PID " + Json.Int(first, "pid", 0) +
                                "（session " + Json.Int(first, "session", 0) + "）";
                    if (holders.Count > 1) text += "，另有 " + (holders.Count - 1) + " 个进程";
                    return text;
                }
            }
            if (lockfile == "locked" || lockfile == "unknown")
                return "lockfile " + lockfile + "，未识别持有者";
            return "空闲";
        }

        private static int HolderCount(Dictionary<string, object> d)
        {
            object holdersObj;
            return d.TryGetValue("holders", out holdersObj) && holdersObj is IList
                ? ((IList)holdersObj).Count
                : 0;
        }

        private Dictionary<string, object> EdgeStatus()
        {
            var r = RunCua("edge --status", 15000);
            try
            {
                return Json.Parse(r.Stdout);
            }
            catch
            {
                return new Dictionary<string, object>();
            }
        }

        private Dictionary<string, object> ChildEdgeStatus()
        {
            var r = RunCua("child edge --status", 20000);
            try
            {
                return Json.Parse(r.Stdout);
            }
            catch
            {
                return new Dictionary<string, object>();
            }
        }

        /// The host may not be allowed to read command lines of the child
        /// session's Edge processes. Merge the main-session view with the child
        /// agent's view so holders and lock state are complete.
        private Dictionary<string, object> MergedEdgeStatus()
        {
            var main = EdgeStatus();
            var child = ChildEdgeStatus();
            if (child.Count == 0) return main;
            var merged = new Dictionary<string, object>(main);
            merged["holders"] = MergeHolders(main, child);
            merged["lockfile"] = PreferredLock(
                Json.Str(main, "lockfile"), Json.Str(child, "lockfile"));
            object cdpObj;
            if (child.TryGetValue("cdp", out cdpObj) && cdpObj is Dictionary<string, object>)
                merged["cdp"] = cdpObj;
            if (string.IsNullOrEmpty(Json.Str(merged, "profile")))
                merged["profile"] = Json.Str(child, "profile");
            if (string.IsNullOrEmpty(Json.Str(merged, "browser_exe")))
                merged["browser_exe"] = Json.Str(child, "browser_exe");
            if (string.IsNullOrEmpty(Json.Str(merged, "browser")))
                merged["browser"] = Json.Str(child, "browser");
            if (string.IsNullOrEmpty(Json.Str(merged, "profile_kind")))
                merged["profile_kind"] = Json.Str(child, "profile_kind");
            return merged;
        }

        private static object[] MergeHolders(
            Dictionary<string, object> a, Dictionary<string, object> b)
        {
            var list = new List<object>();
            var seen = new HashSet<int>();
            Collect(a);
            Collect(b);
            return list.ToArray();

            void Collect(Dictionary<string, object> d)
            {
                object v;
                if (!d.TryGetValue("holders", out v) || !(v is IList)) return;
                foreach (object item in (IList)v)
                {
                    var h = item as Dictionary<string, object>;
                    if (h == null) continue;
                    int pid = Json.Int(h, "pid", 0);
                    if (pid > 0 && seen.Add(pid)) list.Add(h);
                }
            }
        }

        private static string PreferredLock(string a, string b)
        {
            if (a == "locked" || a == "unknown") return a;
            if (b == "locked" || b == "unknown") return b;
            return !string.IsNullOrEmpty(a) ? a : b;
        }

        internal void OpenChildEdgeFromSettings()
        {
            OpenChildEdge();
        }

        private void OpenChildEdge()
        {
            if (!SessionConnected)
            {
                MessageBox.Show(this, "子会话未连接。先启动子会话，再打开浏览器。",
                    "cua · 浏览器", MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }
            if (!_agentReady)
            {
                MessageBox.Show(this, "子会话 agent 尚未就绪，请稍等或查看状态。",
                    "cua · 浏览器", MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }
            Cursor previous = Cursor;
            try
            {
                Cursor = Cursors.WaitCursor;
                var r = RunCua("child edge --wait-ms 20000", 30000);
                if (r.ExitCode != 0)
                {
                    MessageBox.Show(this, "打开子会话 Edge 失败：\n\n" + CuaDetail(r),
                        "cua · 浏览器", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                }
            }
            finally
            {
                Cursor = previous;
            }
        }

        internal void BackupEdgeProfileFromSettings()
        {
            var d = MergedEdgeStatus();
            string profile = Json.Str(d, "profile");
            if (string.IsNullOrEmpty(profile))
                profile = Path.Combine(_opt.DataDir, "browser", ProfileKindName(d));
            if (!Directory.Exists(profile))
            {
                MessageBox.Show(this, "浏览器 profile 还不存在。先在主会话运行 `cua session edge --sign-in` 登录。",
                    "cua · 浏览器", MessageBoxButtons.OK, MessageBoxIcon.Information);
                return;
            }
            string lockfile = Json.Str(d, "lockfile");
            if (HolderCount(d) > 0 || (lockfile != null && lockfile != "absent" && lockfile != "stale"))
            {
                MessageBox.Show(this, "浏览器 profile 正在被使用（" + EdgeStateText(d) +
                    "）。\n先关闭所有使用它的浏览器窗口再备份。",
                    "cua · 浏览器", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return;
            }
            if (!Warn("备份会复制该 profile 的 cookie 和已保存密码，仍然保存在本机同一用户目录。\n\n继续备份？"))
                return;
            string dest = profile + "-backup-" + DateTime.Now.ToString("yyyyMMdd-HHmmss");
            Cursor previous = Cursor;
            try
            {
                Cursor = Cursors.WaitCursor;
                CopyDirectory(profile, dest);
                MessageBox.Show(this, "已备份到：\n" + dest,
                    "cua · 浏览器", MessageBoxButtons.OK, MessageBoxIcon.Information);
            }
            catch (Exception ex)
            {
                MessageBox.Show(this, "备份失败：\n" + ex.GetBaseException().Message,
                    "cua · 浏览器", MessageBoxButtons.OK, MessageBoxIcon.Error);
            }
            finally
            {
                Cursor = previous;
            }
        }

        internal void DeleteEdgeProfileFromSettings()
        {
            if (!Warn("删除浏览器 profile 会删除其中的 cookie 和已保存密码，无法撤销。\n" +
                      "子会话之后需要重新登录。\n\n确定删除？"))
                return;
            var d = MergedEdgeStatus();
            string lockfile = Json.Str(d, "lockfile");
            if (HolderCount(d) > 0 || (lockfile != null && lockfile != "absent" && lockfile != "stale"))
            {
                MessageBox.Show(this, "浏览器 profile 正在被使用（" + EdgeStateText(d) +
                    "）。\n先关闭所有使用它的浏览器窗口再删除。",
                    "cua · 浏览器", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                return;
            }
            var r = RunCua("session edge --delete-profile --confirm", 30000);
            if (r.ExitCode == 0)
            {
                MessageBox.Show(this, CuaDetail(r),
                    "cua · 浏览器", MessageBoxButtons.OK, MessageBoxIcon.Information);
            }
            else
            {
                MessageBox.Show(this, "删除失败：\n\n" + CuaDetail(r),
                    "cua · 浏览器", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            }
        }

        private static void CopyDirectory(string source, string dest)
        {
            Directory.CreateDirectory(dest);
            foreach (string file in Directory.GetFiles(source))
                File.Copy(file, Path.Combine(dest, Path.GetFileName(file)), true);
            foreach (string dir in Directory.GetDirectories(source))
            {
                if ((File.GetAttributes(dir) & FileAttributes.ReparsePoint) != 0) continue;
                CopyDirectory(dir, Path.Combine(dest, Path.GetFileName(dir)));
            }
        }

        private CuaResult RunCua(string arguments, int timeoutMs)
        {
            var result = new CuaResult();
            try
            {
                var psi = new ProcessStartInfo(_opt.CuaExe, arguments)
                {
                    UseShellExecute = false,
                    CreateNoWindow = true,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    WorkingDirectory = Path.GetDirectoryName(_opt.CuaExe) ?? _opt.DataDir
                };
                psi.EnvironmentVariables["CUA_DATA_DIR"] = _opt.DataDir;
                using (var p = new Process { StartInfo = psi })
                {
                    p.Start();
                    Task<string> stdout = p.StandardOutput.ReadToEndAsync();
                    Task<string> stderr = p.StandardError.ReadToEndAsync();
                    if (!p.WaitForExit(timeoutMs))
                    {
                        try { p.Kill(); } catch { }
                        result.ExitCode = -1;
                        result.Stderr = "cua " + arguments + " timed out after " + timeoutMs + " ms";
                    }
                    else
                    {
                        result.ExitCode = p.ExitCode;
                    }
                    try { Task.WaitAll(new Task[] { stdout, stderr }, 3000); } catch { }
                    result.Stdout = stdout.Status == TaskStatus.RanToCompletion ? stdout.Result : "";
                    result.Stderr = stderr.Status == TaskStatus.RanToCompletion ? stderr.Result : result.Stderr ?? "";
                }
            }
            catch (Exception ex)
            {
                result.ExitCode = -1;
                result.Stderr = ex.GetBaseException().Message;
            }
            return result;
        }

        private static string CuaDetail(CuaResult r)
        {
            string text = null;
            try
            {
                var d = Json.Parse(r.Stdout);
                text = Json.Str(d, "detail");
                string status = Json.Str(d, "status");
                string code = Json.Str(d, "code");
                if (string.IsNullOrEmpty(text) && !string.IsNullOrEmpty(status))
                    text = "status: " + status;
                if (!string.IsNullOrEmpty(code))
                    text = (text ?? "") + " (" + code + ")";
            }
            catch { }
            if (string.IsNullOrEmpty(text))
                text = string.IsNullOrWhiteSpace(r.Stderr) ? r.Stdout : r.Stderr;
            if (string.IsNullOrWhiteSpace(text))
                text = "exit code " + r.ExitCode;
            text = text.Trim();
            return text.Length > 1600 ? text.Substring(0, 1600) + "…" : text;
        }

        private sealed class CuaResult
        {
            internal int ExitCode;
            internal string Stdout = "";
            internal string Stderr = "";
        }

        private void OpenSettings()
        {
            if (!Visible) ShowViewer(!_viewOnly);
            using (var form = new SessionSettingsForm(this))
                form.ShowDialog(this);
        }

        internal void TakeOverFromSettings()
        {
            ShowViewer(false);
        }

        internal void ReleaseFromSettings()
        {
            ShowViewer(true);
        }

        internal bool StartFromSettings()
        {
            if (SessionConnected) return false;
            _statusPill.SetState("正在启动子会话", Theme.Accent);
            Connect(false);
            return true;
        }

        internal bool LogoffFromSettings()
        {
            if (!SessionConnected) return false;
            if (!Warn("注销子会话会关闭其中的所有应用（包括哔哩哔哩、文档等）。观察窗和托盘图标会保留，之后可以再启动。\n\n确定注销？"))
                return false;
            LogoffSession();
            return true;
        }

        internal bool RestartFromSettings()
        {
            if (!SessionConnected) return StartFromSettings();
            if (!Warn("重新启动会先注销当前子会话，关闭其中的所有应用，然后创建一个新的子会话。\n\n确定继续？"))
                return false;
            LogoffSession();
            var timer = new System.Windows.Forms.Timer { Interval = 800 };
            timer.Tick += delegate
            {
                timer.Stop();
                timer.Dispose();
                Connect(false);
            };
            timer.Start();
            return true;
        }

        internal bool ExitFromSettings()
        {
            if (!Warn("注销并退出观察器会关闭子会话中的所有应用，并移除托盘图标。\n\n确定继续？"))
                return false;
            StopSession();
            return true;
        }

        internal void OpenCredentialsFromSettings()
        {
            using (var dialog = new CredentialDialog())
            {
                if (dialog.ShowDialog(this) != DialogResult.OK) return;
                try { _credential = CredentialStore.Load(); }
                catch (Exception ex) { SetError("saved credential is unreadable: " + ex.GetBaseException().Message); }
                UpdateMeta();
                WriteState();
            }
        }

        internal void HideViewerFromSettings()
        {
            HideViewer();
        }

        private bool Warn(string message)
        {
            return MessageBox.Show(this, message, "cua 子会话",
                       MessageBoxButtons.YesNo, MessageBoxIcon.Warning,
                       MessageBoxDefaultButton.Button2) == DialogResult.Yes;
        }

        private void LogoffSession()
        {
            try { _poll.Stop(); } catch { }
            try { _rdp.Disconnect(); } catch { }
            try
            {
                if (_childId.HasValue) ChildSessionApi.Logoff(_childId.Value);
            }
            catch (Exception ex)
            {
                SetError("log off failed: " + ex.GetBaseException().Message);
            }

            _connected = false;
            _childId = null;
            _agentPipe = null;
            _agentReady = false;
            _agentTried = false;
            _agentAttempts = 0;
            _agentRetryAt = DateTime.MinValue;
            _viewerInitialized = true;
            _readyStatus = false;
            _lastError = null;
            ReleaseStartupQuarantine();
            _startup = null;
            _startupReleased = true;
            _startupReleaseAt = DateTime.MinValue;
            _statusPill.SetState("子会话已注销", Theme.Warn);
            UpdateMeta();
            UpdateButtons();
            WriteState();
        }

        private void OnFormClosing(object sender, FormClosingEventArgs e)
        {
            if (_stopping) return;
            // Closing the viewer must never kill the child session. Use the
            // settings menu or `cua session stop` to log it off.
            e.Cancel = true;
            HideViewer();
        }

        public string HandleHostCommand(Dictionary<string, object> request)
        {
            if (InvokeRequired)
            {
                return (string)Invoke(new Func<Dictionary<string, object>, string>(HandleHostCommand), request);
            }
            string op = Json.Str(request, "op") ?? "status";
            try
            {
                switch (op)
                {
                    case "ping":
                        break;
                    case "status":
                        break;
                    case "view":
                        ShowViewer(Json.Bool(request, "view_only", true));
                        break;
                    case "hide":
                        HideViewer();
                        break;
                    case "takeover":
                        ShowViewer(false);
                        break;
                    case "release":
                        ShowViewer(true);
                        break;
                    case "vnc.child":
                        SwitchViewMode(true);
                        break;
                    case "stop":
                        {
                            string response = Json.Serialize(Status());
                            BeginInvoke(new Action(StopSession));
                            return response;
                        }
                    default:
                        return Json.Serialize(new Dictionary<string, object>
                        {
                            { "ok", false },
                            { "error", "unknown op: " + op }
                        });
                }
                return Json.Serialize(Status());
            }
            catch (Exception ex)
            {
                return Json.Serialize(new Dictionary<string, object>
                {
                    { "ok", false },
                    { "error", ex.GetBaseException().Message }
                });
            }
        }

        private Dictionary<string, object> Status()
        {
            return new Dictionary<string, object>
            {
                { "ok", true },
                { "schema", "cua-session-status-v1" },
                { "host_pid", Process.GetCurrentProcess().Id },
                { "child_session_id", _childId.HasValue ? (object)_childId.Value : 0 },
                { "agent_pipe", _agentPipe ?? "" },
                { "agent_ready", _agentReady },
                { "connected", _connected },
                { "viewer_visible", Visible && WindowState != FormWindowState.Minimized },
                { "viewer_shown", _viewerShown },
                { "viewer_maximized", _isMaximized },
                { "viewer_x", Bounds.X },
                { "viewer_y", Bounds.Y },
                { "viewer_width", Bounds.Width },
                { "viewer_height", Bounds.Height },
                { "view_only", _viewOnly },
                { "width", _opt.Width },
                { "height", _opt.Height },
                { "scale", _scalePercent },
                { "credential_saved", _credential != null },
                { "credential_user", _credential != null ? _credential.UserName : "" },
                { "startup_quarantined", _startup != null && !_startupReleased },
                { "keep_startup", _opt.KeepStartup },
                { "started", _started.ToString("o") },
                { "last_error", _lastError ?? "" }
            };
        }

        private void WriteState()
        {
            try
            {
                var state = Status();
                state["schema"] = "cua-session-v1";
                state["host_pipe"] = _opt.HostPipe;
                state["cua_exe"] = _opt.CuaExe;
                state["data_dir"] = _opt.DataDir;
                File.WriteAllText(Path.Combine(_opt.DataDir, "session.json"), Json.Serialize(state));
                if (string.IsNullOrEmpty(_lastError))
                {
                    string err = Path.Combine(_opt.DataDir, "session-error.json");
                    if (File.Exists(err)) File.Delete(err);
                }
            }
            catch { }
        }

        private void SetError(string message)
        {
            _lastError = message;
            _statusPill.SetState("错误", Theme.Warn);
            try
            {
                File.WriteAllText(Path.Combine(_opt.DataDir, "session-error.json"),
                    Json.Serialize(new Dictionary<string, object>
                    {
                        { "ok", false },
                        { "kind", "session" },
                        { "error", message }
                    }));
            }
            catch { }
        }

        private void Fail(string message)
        {
            SetError(message);
            WriteState();
            if (_opt.View)
                MessageBox.Show(message, "cua child session", MessageBoxButtons.OK, MessageBoxIcon.Error);
            BeginInvoke(new Action(StopSession));
        }

        private void DisposeTray()
        {
            if (_tray == null) return;
            try { _tray.Visible = false; } catch { }
            try { _tray.Dispose(); } catch { }
            _tray = null;
            try { _trayMenu?.Dispose(); } catch { }
            _trayMenu = null;
            try { _tips?.Dispose(); } catch { }
            _tips = null;
        }

        private void StopSession()
        {
            if (_stopping) return;
            _stopping = true;
            try { _poll.Stop(); } catch { }
            try { _rdp.Disconnect(); } catch { }
            try
            {
                if (_childId.HasValue) ChildSessionApi.Logoff(_childId.Value);
            }
            catch { }
            try
            {
                string state = Path.Combine(_opt.DataDir, "session.json");
                if (File.Exists(state)) File.Delete(state);
            }
            catch { }
            try { _pipe.Stop(); } catch { }
            ReleaseStartupQuarantine();
            DisposeTray();
            Close();
            Application.Exit();
        }
    }
}
