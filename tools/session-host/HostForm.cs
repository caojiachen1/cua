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

        private readonly Options _opt;
        private readonly Panel _bar;
        private readonly Panel _content;
        private readonly SessionRdp _rdp;
        private readonly Label _title;
        private readonly Label _meta;
        private readonly StatusPill _statusPill;
        private readonly FluentButton _takeover;
        private readonly FluentButton _release;
        private readonly FluentButton _hide;
        private readonly FluentButton _settings;
        private readonly FluentButton _edge;
        private readonly FluentButton _minimize;
        private readonly FluentButton _maximize;
        private readonly FluentButton _close;
        private readonly System.Windows.Forms.Timer _poll;
        private readonly DateTime _started = DateTime.UtcNow;
        private readonly Rectangle _normalBounds;

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
        private bool _readyStatus;
        private bool _stopping;
        private bool _trayTipShown;
        private bool _isMaximized = true;
        private StartupQuarantine _startup;
        private bool _startupReleased = true;
        private DateTime _startupReleaseAt = DateTime.MinValue;
        private string _lastError;
        private ChildCredential _credential;
        private int _scalePercent = 100;
        private float _uiScale = 1f;

        public HostForm(Options opt)
        {
            _opt = opt;
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
            _normalBounds = new Rectangle(
                work.X + work.Width / 10,
                work.Y + work.Height / 10,
                work.Width * 8 / 10,
                work.Height * 8 / 10);
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
            _hide = new FluentButton { Text = "隐藏", Accent = false };
            _settings = new FluentButton { Text = "设置", Accent = false };
            _edge = new FluentButton { Text = "子会话浏览器", Accent = false };

            _minimize = new FluentButton
            {
                Text = "\uE921",
                Font = Theme.Icon(9f),
                CornerRadius = 6
            };
            _maximize = new FluentButton
            {
                Text = "\uE922",
                Font = Theme.Icon(9f),
                CornerRadius = 6
            };
            _close = new FluentButton
            {
                Text = "\uE8BB",
                Font = Theme.Icon(9f),
                CornerRadius = 6,
                Danger = true
            };
            _tips = new ToolTip();
            _tips.SetToolTip(_minimize, "最小化到任务栏");
            _tips.SetToolTip(_maximize, "最大化 / 还原");
            _tips.SetToolTip(_close, "隐藏到托盘（子会话继续运行）");
            _tips.SetToolTip(_hide, "隐藏到托盘");
            _tips.SetToolTip(_settings, "子会话设置");
            _tips.SetToolTip(_edge, "在子会话中打开独立浏览器（CuaEdge / CuaChrome）");

            _takeover.Click += delegate { SetViewOnly(false); };
            _release.Click += delegate { SetViewOnly(true); };
            _hide.Click += delegate { HideViewer(); };
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
            _bar.Controls.Add(_takeover);
            _bar.Controls.Add(_release);
            _bar.Controls.Add(_hide);
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
            };
            FormClosing += OnFormClosing;
            UpdateButtons();
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
            _bar.Height = Math.Max(44, (int)Math.Round(56 * _uiScale));
            _statusPill.Height = Math.Max(24, (int)Math.Round(28 * _uiScale));
            _statusPill.Font = Theme.Ui(9f, FontStyle.Bold);
            _statusPill.RefreshSize();

            int buttonHeight = Math.Max(30, (int)Math.Round(34 * _uiScale));
            _takeover.Size = new Size(Math.Max(96, (int)Math.Round(120 * _uiScale)), buttonHeight);
            _release.Size = _takeover.Size;
            _hide.Size = new Size(Math.Max(60, (int)Math.Round(72 * _uiScale)), buttonHeight);
            _settings.Size = new Size(Math.Max(56, (int)Math.Round(68 * _uiScale)), buttonHeight);
            int iconWidth = Math.Max(32, (int)Math.Round(38 * _uiScale));
            _minimize.Size = new Size(iconWidth, buttonHeight);
            _maximize.Size = _minimize.Size;
            _close.Size = _minimize.Size;
            Native.ApplyDarkWindow(Handle);
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
            }
            else
            {
                _rdp.Bounds = area;
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
            int pad = Math.Max(12, (int)Math.Round(18 * _uiScale));
            int gap = Math.Max(6, (int)Math.Round(10 * _uiScale));
            int top = Math.Max(0, (_bar.Height - _title.Height) / 2);
            int pillTop = Math.Max(0, (_bar.Height - _statusPill.Height) / 2);
            int buttonTop = Math.Max(0, (_bar.Height - _takeover.Height) / 2);

            int right = _bar.ClientSize.Width - pad;
            _close.Left = right - _close.Width;
            right = _close.Left - gap;
            _maximize.Left = right - _maximize.Width;
            right = _maximize.Left - gap;
            _minimize.Left = right - _minimize.Width;
            right = _minimize.Left - gap;
            _hide.Left = right - _hide.Width;
            right = _hide.Left - gap;
            FluentButton action = _viewOnly ? _takeover : _release;
            action.Left = right - action.Width;
            right = action.Left - gap;
            _settings.Left = right - _settings.Width;
            right = _settings.Left - gap;
            _edge.Left = right - _edge.Width;
            right = _edge.Left - gap;

            _close.Top = buttonTop;
            _maximize.Top = buttonTop;
            _minimize.Top = buttonTop;
            _hide.Top = buttonTop;
            _settings.Top = buttonTop;
            _edge.Top = buttonTop;
            _takeover.Top = buttonTop;
            _release.Top = buttonTop;

            int left = pad;
            _title.Left = left;
            _title.Top = top;
            left = _title.Right + gap;
            _statusPill.Left = left;
            _statusPill.Top = pillTop;
            left = _statusPill.Right + gap;
            _meta.Left = left;
            _meta.Top = top;
            _meta.Height = _title.Height;
            _meta.Width = Math.Max(0, right - left);
        }

        private void InitTray()
        {
            _trayMenu = new ContextMenuStrip
            {
                ShowImageMargin = false,
                BackColor = Theme.Bar,
                ForeColor = Theme.Text
            };
            _trayMenu.Items.Add(MenuItem("打开观察窗", delegate { ShowViewer(!_viewOnly); }));
            _trayMenu.Items.Add(MenuItem("子会话设置…", OpenSettings));
            _trayMenu.Items.Add(MenuItem("在子会话打开浏览器", OpenChildEdge));
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

            _tray = new NotifyIcon
            {
                Icon = Theme.AppIcon(true),
                Text = "cua child session",
                Visible = true,
                ContextMenuStrip = _trayMenu
            };
            _tray.DoubleClick += delegate { ShowViewer(!_viewOnly); };
            UpdateTrayMenu();
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

                Size size = ChildSize();
                _opt.Width = size.Width;
                _opt.Height = size.Height;
                LayoutContent();
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

        private Size ChildSize()
        {
            if (_opt.ExplicitSize)
                return EvenSize(_opt.Width, _opt.Height);

            Size content = _content.ClientSize;
            if (content.Width >= 640 && content.Height >= 480)
                return EvenSize(content.Width, content.Height);

            Rectangle work = Screen.PrimaryScreen.WorkingArea;
            return EvenSize(work.Width, work.Height - _bar.Height);
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
                }
                return;
            }

            if (!_connected)
            {
                _connected = true;
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
                _childId = ChildSessionApi.TryGetId();
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
                _isMaximized = true;
                Bounds = Screen.PrimaryScreen.WorkingArea;
                Show();
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
            _isMaximized = !_isMaximized;
            if (WindowState == FormWindowState.Minimized)
                WindowState = FormWindowState.Normal;
            Bounds = _isMaximized ? Screen.PrimaryScreen.WorkingArea : _normalBounds;
            LayoutShell();
            LayoutBar();
            UpdateWindowGlyph();
        }

        private void UpdateWindowGlyph()
        {
            if (_maximize == null) return;
            _maximize.Text = _isMaximized ? "\uE923" : "\uE922";
            _tips?.SetToolTip(_maximize, _isMaximized ? "还原窗口" : "最大化窗口");
        }

        private void SetViewOnly(bool viewOnly)
        {
            if (InvokeRequired)
            {
                BeginInvoke(new Action<bool>(SetViewOnly), viewOnly);
                return;
            }
            _viewOnly = viewOnly;
            try { _rdp.SetInputBlocked(viewOnly); } catch { }
            if (viewOnly)
            {
                _statusPill.SetState("只读观察 · 键鼠已屏蔽", Theme.Warn);
            }
            else
            {
                _statusPill.SetState("你正在控制子会话", Theme.Accent);
                if (Visible)
                {
                    Activate();
                    _rdp.FocusInput();
                }
            }
            UpdateButtons();
            LayoutBar();
            WriteState();
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

        private void UpdateMeta()
        {
            string id = _childId.HasValue ? _childId.Value.ToString() : "—";
            string cred = _credential != null ? " · 已保存凭据" : " · 自动登录";
            string startup = (_startup != null && !_startupReleased) ? " · 启动项隔离" : "";
            _meta.Text = "session " + id + "  ·  " + _opt.Width + "×" + _opt.Height +
                         "  ·  " + _scalePercent + "%" + cred + startup;
            LayoutBar();
            UpdateTrayText();
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
