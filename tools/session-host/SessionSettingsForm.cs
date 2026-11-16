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
        private float _scale = 1f;
        private bool _edgeLoading;

        internal SessionSettingsForm(HostForm owner)
        {
            _owner = owner;
            Text = "cua · 子会话设置";
            Icon = Theme.AppIcon(false);
            FormBorderStyle = FormBorderStyle.FixedDialog;
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

            _openEdge = MakeButton("在子会话打开 Edge", true);
            _refreshEdge = MakeButton("刷新 CuaEdge 状态", false);
            _backupEdge = MakeButton("备份 profile…", false);
            _deleteEdge = MakeButton("删除 CuaEdge profile…", false);
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

            ClientSize = new Size(S(640), S(560));
            _heading.Location = new Point(S(24), S(20));
            _hint.Location = new Point(S(24), S(58));
            _hint.Size = new Size(S(592), S(52));
            _info.Location = new Point(S(24), S(122));
            _info.Size = new Size(S(592), S(62));
            _edgeInfo.Location = new Point(S(24), S(192));
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

            _openEdge.Location = new Point(S(24), S(318));
            _refreshEdge.Location = new Point(S(240), S(318));
            _backupEdge.Location = new Point(S(416), S(318));
            _takeover.Location = new Point(S(24), S(368));
            _release.Location = _takeover.Location;
            _hide.Location = new Point(S(240), S(368));
            _credentials.Location = new Point(S(416), S(368));
            _start.Location = new Point(S(24), S(418));
            _restart.Location = _start.Location;
            _logoff.Location = new Point(S(240), S(418));
            _close.Location = new Point(S(416), S(418));
            _deleteEdge.Location = new Point(S(24), S(478));
            _exit.Location = new Point(S(288), S(478));
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
            _edgeInfo.Text = "正在读取 CuaEdge 状态…";
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
                            : "读取 CuaEdge 状态失败";
                        _edgeLoading = false;
                    }));
                }
                catch { }
            });
        }
    }
}
