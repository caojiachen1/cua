using System;
using System.Drawing;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace CuaSessionHost
{
    internal sealed class SessionSettingsForm : Form
    {
        private readonly HostForm _owner;
        private readonly Label _heading;
        private readonly Label _hint;
        private readonly Label _info;
        private readonly Label _edgeInfo;
        private readonly FluentButton _openEdge;
        private readonly FluentButton _refreshEdge;
        private readonly FluentButton _backupEdge;
        private readonly FluentButton _deleteEdge;
        private readonly FluentButton _takeover;
        private readonly FluentButton _release;
        private readonly FluentButton _hide;
        private readonly FluentButton _credentials;
        private readonly FluentButton _start;
        private readonly FluentButton _restart;
        private readonly FluentButton _logoff;
        private readonly FluentButton _exit;
        private readonly FluentButton _close;
        private System.Windows.Forms.CheckBox _chkHeadless;
        private Panel _capStrip;
        private Label _capTitle;
        private FluentButton _capClose;
        private float _scale = 1f;
        private bool _edgeLoading;

        internal SessionSettingsForm(HostForm owner)
        {
            _owner = owner;
            Text = "cua · 子会话设置";
            Icon = Theme.AppIcon(false);
            FormBorderStyle = FormBorderStyle.None;
            StartPosition = FormStartPosition.CenterParent;
            MaximizeBox = false;
            MinimizeBox = false;
            ShowInTaskbar = false;
            BackColor = Theme.Window;
            ForeColor = Theme.Text;
            AutoScaleMode = AutoScaleMode.None;
            ClientSize = new Size(640, 560);
            Font = Theme.Ui(9f);

            _heading = new Label
            {
                Text = "子会话设置",
                Font = Theme.Title(15f),
                ForeColor = Theme.Text,
                AutoSize = true
            };
            _hint = new Label
            {
                Text = "这些操作作用于同一个 Windows 子会话。注销和重新启动会关闭其中的所有应用，执行前会再次确认。",
                Font = Theme.Ui(9f),
                ForeColor = Theme.Muted,
                AutoSize = false
            };
            _info = new Label
            {
                Font = Theme.Ui(9.5f),
                ForeColor = Theme.Text,
                BackColor = Theme.Bar,
                AutoSize = false,
                TextAlign = ContentAlignment.MiddleLeft,
                Padding = new Padding(12, 0, 12, 0)
            };
            _edgeInfo = new Label
            {
                Font = Theme.Ui(9.5f),
                ForeColor = Theme.Text,
                BackColor = Theme.Bar,
                AutoSize = false,
                AutoEllipsis = true,
                TextAlign = ContentAlignment.TopLeft,
                Padding = new Padding(12, 10, 12, 10)
            };

            _openEdge = MakeButton("在子会话打开浏览器", true);
            _refreshEdge = MakeButton("刷新浏览器状态", false);
            _backupEdge = MakeButton("备份 profile…", false);
            _deleteEdge = MakeButton("删除浏览器 profile…", false);
            _deleteEdge.Danger = true;
            _takeover = MakeButton("接管控制", true);
            _release = MakeButton("释放控制", false);
            _hide = MakeButton("隐藏到托盘", false);
            _credentials = MakeButton("自动登录凭据…", false);
            _start = MakeButton("启动子会话", true);
            _restart = MakeButton("重新启动子会话…", false);
            _logoff = MakeButton("注销子会话…", false);
            _exit = MakeButton("注销并退出观察器…", false);
            _exit.Danger = true;
            _close = MakeButton("关闭窗口", false);

            _chkHeadless = new System.Windows.Forms.CheckBox
            {
                Text = "下次启动后自动关闭路由（无头模式：远程软件接管主会话）",
                ForeColor = Theme.Text,
                BackColor = Theme.Window,
                AutoSize = true
            };
            _chkHeadless.Checked = VncHelper.HeadlessAutoDisable;
            _chkHeadless.CheckedChanged += delegate
            {
                VncHelper.HeadlessAutoDisable = _chkHeadless.Checked;
                _owner.SyncTrayToggleStates();
            };

            // 自绘标题条：颜色精确 32,32,32，可拖拽，右上关闭
            _capStrip = new Panel
            {
                BackColor = Color.FromArgb(32, 32, 32),
                Dock = DockStyle.Top,
                Height = 40
            };
            _capTitle = new Label
            {
                Text = "cua · 子会话设置",
                Font = Theme.Ui(9.5f),
                ForeColor = Theme.Text,
                BackColor = Color.FromArgb(32, 32, 32),
                AutoSize = true,
                Location = new Point(14, 10)
            };
            _capClose = new FluentButton { Text = "\uE8BB", Font = Theme.Icon(8f), Ghost = true, Danger = true, CornerRadius = 0 };
            _capClose.Size = new Size(56, 40);
            _capClose.Anchor = AnchorStyles.Top | AnchorStyles.Right;
            _capClose.Location = new Point(Math.Max(0, ClientSize.Width - 36), 0);
            _capClose.Click += delegate { Close(); };

            foreach (var c in new Control[] { _capStrip, _capTitle })
            {
                c.MouseDown += (sdr, ev) =>
                {
                    if (ev.Button == MouseButtons.Left)
                    {
                        Native.ReleaseCapture();
                        Native.SendMessage(Handle, 0xA1 /* WM_NCLBUTTONDOWN */, (IntPtr)2 /* HTCAPTION */, IntPtr.Zero);
                    }
                };
            }
            Controls.Add(_capTitle);
            Controls.Add(_capClose);
            Controls.Add(_capStrip);

            _openEdge.Click += delegate { _owner.OpenChildEdgeFromSettings(); RefreshState(); };
            _refreshEdge.Click += delegate { RefreshState(); };
            _backupEdge.Click += delegate { _owner.BackupEdgeProfileFromSettings(); RefreshState(); };
            _deleteEdge.Click += delegate { _owner.DeleteEdgeProfileFromSettings(); RefreshState(); };
            _takeover.Click += delegate { _owner.TakeOverFromSettings(); RefreshState(); };
            _release.Click += delegate { _owner.ReleaseFromSettings(); RefreshState(); };
            _hide.Click += delegate { Close(); _owner.HideViewerFromSettings(); };
            _credentials.Click += delegate { _owner.OpenCredentialsFromSettings(); RefreshState(); };
            _start.Click += delegate { if (_owner.StartFromSettings()) Close(); };
            _restart.Click += delegate { if (_owner.RestartFromSettings()) Close(); };
            _logoff.Click += delegate { if (_owner.LogoffFromSettings()) Close(); };
            _exit.Click += delegate { if (_owner.ExitFromSettings()) Close(); };
            _close.Click += delegate { Close(); };

            Controls.Add(_heading);
            Controls.Add(_hint);
            Controls.Add(_info);
            Controls.Add(_edgeInfo);
            Controls.Add(_openEdge);
            Controls.Add(_refreshEdge);
            Controls.Add(_backupEdge);
            Controls.Add(_deleteEdge);
            Controls.Add(_takeover);
            Controls.Add(_release);
            Controls.Add(_hide);
            Controls.Add(_credentials);
            Controls.Add(_start);
            Controls.Add(_restart);
            Controls.Add(_logoff);
            Controls.Add(_exit);
            Controls.Add(_close);
            Controls.Add(_chkHeadless);

            Load += delegate
            {
                ApplyScale();
                Native.ApplyDarkWindow(Handle);
                RefreshState();
            };
            Shown += delegate { Native.ApplyDarkWindow(Handle); };
        }

        private static FluentButton MakeButton(string text, bool accent)
        {
            return new FluentButton
            {
                Text = text,
                Accent = accent,
                Size = new Size(200, 38)
            };
        }

        private void ApplyScale()
        {
            _scale = Math.Max(1f, Native.GetDpiForWindow(Handle) / 96f);
            int S(int value) { return (int)Math.Round(value * _scale); }
            int capH = S(40); // 自绘标题条高度：所有内容坐标统一内联下移（幂等）

            ClientSize = new Size(S(640), S(620) + capH);
            _heading.Location = new Point(S(24), S(20) + capH);
            _hint.Location = new Point(S(24), S(58) + capH);
            _hint.Size = new Size(S(592), S(52));
            _info.Location = new Point(S(24), S(122) + capH);
            _info.Size = new Size(S(592), S(62));
            _edgeInfo.Location = new Point(S(24), S(192) + capH);
            _edgeInfo.Size = new Size(S(592), S(112));

            _openEdge.Size = new Size(S(200), S(38));
            _refreshEdge.Size = new Size(S(160), S(38));
            _backupEdge.Size = new Size(S(200), S(38));
            _deleteEdge.Size = new Size(S(240), S(38));
            _takeover.Size = new Size(S(200), S(38));
            _release.Size = _takeover.Size;
            _hide.Size = new Size(S(160), S(38));
            _credentials.Size = new Size(S(200), S(38));
            _start.Size = new Size(S(200), S(38));
            _restart.Size = _start.Size;
            _logoff.Size = new Size(S(160), S(38));
            _exit.Size = new Size(S(240), S(38));
            _close.Size = new Size(S(160), S(38));

            _openEdge.Location = new Point(S(24), S(318) + capH);
            _refreshEdge.Location = new Point(S(240), S(318) + capH);
            _backupEdge.Location = new Point(S(416), S(318) + capH);
            _takeover.Location = new Point(S(24), S(368) + capH);
            _release.Location = _takeover.Location;
            _hide.Location = new Point(S(240), S(368) + capH);
            _credentials.Location = new Point(S(416), S(368) + capH);
            _start.Location = new Point(S(24), S(418) + capH);
            _restart.Location = _start.Location;
            _logoff.Location = new Point(S(240), S(418) + capH);
            _close.Location = new Point(S(416), S(418) + capH);
            _deleteEdge.Location = new Point(S(24), S(478) + capH);
            _exit.Location = new Point(S(288), S(478) + capH);
            _chkHeadless.Location = new Point(S(24), S(576) + capH);

            // 标题条按钮贴最右上角（不含边距）
            _capClose.Location = new Point(ClientSize.Width - _capClose.Width, 0);
            CenterToScreen();
        }

        private void RefreshState()
        {
            _info.Text = _owner.SessionSummary + Environment.NewLine + _owner.CredentialSummary;
            RefreshEdgeInfo();
            _openEdge.Enabled = _owner.SessionConnected;
            _takeover.Visible = _owner.ViewOnly;
            _release.Visible = !_owner.ViewOnly;
            _start.Visible = !_owner.SessionConnected;
            _restart.Visible = _owner.SessionConnected;
            _logoff.Enabled = _owner.SessionConnected;
            _exit.Enabled = _owner.SessionConnected;
        }

        private void RefreshEdgeInfo()
        {
            if (_edgeLoading || IsDisposed || !IsHandleCreated) return;
            _edgeLoading = true;
            _edgeInfo.Text = "正在读取浏览器状态…";
            Task.Run(() => _owner.EdgeSummary).ContinueWith(t =>
            {
                if (IsDisposed) return;
                try
                {
                    BeginInvoke((Action)(delegate
                    {
                        if (IsDisposed) return;
                        _edgeInfo.Text = t.Status == TaskStatus.RanToCompletion
                            ? t.Result
                            : "读取浏览器状态失败";
                        _edgeLoading = false;
                    }));
                }
                catch { }
            });
        }
    }
}
