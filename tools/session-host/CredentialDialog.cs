using System;
using System.Drawing;
using System.Windows.Forms;

namespace CuaSessionHost
{
    internal sealed class CredentialDialog : Form
    {
        private readonly Label _heading;
        private readonly Label _hint;
        private readonly Label _accountLabel;
        private readonly Label _passwordLabel;
        private readonly TextBox _account;
        private readonly TextBox _password;
        private readonly Label _status;
        private readonly FluentButton _save;
        private readonly FluentButton _cancel;
        private readonly FluentButton _clear;
        private float _scale = 1f;

        public CredentialDialog()
        {
            Text = "cua · 子会话自动登录";
            Icon = Theme.AppIcon(false);
            FormBorderStyle = FormBorderStyle.FixedDialog;
            StartPosition = FormStartPosition.CenterScreen;
            MaximizeBox = false;
            MinimizeBox = false;
            ShowInTaskbar = true;
            BackColor = Theme.Window;
            ForeColor = Theme.Text;
            AutoScaleMode = AutoScaleMode.None;
            ClientSize = new Size(600, 400);
            Font = Theme.Ui(9f);

            _heading = new Label
            {
                Text = "子会话自动登录",
                Font = Theme.Title(15f),
                ForeColor = Theme.Text,
                AutoSize = true,
                Location = new Point(24, 20)
            };
            _hint = new Label
            {
                Text = "用 PIN / Windows Hello 登录父会话时，Windows 不能把该登录复用到子会话。"
                     + "在这里保存账户密码后，cua 会从 Windows 凭据管理器读取，启动子会话时不再弹密码框。",
                Font = Theme.Ui(9f),
                ForeColor = Theme.Muted,
                AutoSize = false,
                Location = new Point(24, 58),
                Size = new Size(472, 56)
            };
            _accountLabel = new Label
            {
                Text = "Windows 账户",
                ForeColor = Theme.Muted,
                AutoSize = true,
                Location = new Point(24, 130)
            };
            _account = new TextBox
            {
                Location = new Point(24, 154),
                Size = new Size(472, 30),
                BackColor = Theme.Field,
                ForeColor = Theme.Text,
                BorderStyle = BorderStyle.FixedSingle,
                Font = Theme.Ui(10f)
            };
            _passwordLabel = new Label
            {
                Text = "账户密码",
                ForeColor = Theme.Muted,
                AutoSize = true,
                Location = new Point(24, 200)
            };
            _password = new TextBox
            {
                Location = new Point(24, 224),
                Size = new Size(472, 30),
                BackColor = Theme.Field,
                ForeColor = Theme.Text,
                BorderStyle = BorderStyle.FixedSingle,
                Font = Theme.Ui(10f),
                UseSystemPasswordChar = true
            };
            _status = new Label
            {
                ForeColor = Theme.Muted,
                AutoSize = false,
                Location = new Point(24, 264),
                Size = new Size(300, 24)
            };
            _save = new FluentButton
            {
                Text = "保存并启用自动登录",
                Accent = true,
                Size = new Size(220, 38)
            };
            _cancel = new FluentButton
            {
                Text = "取消",
                Size = new Size(80, 38)
            };
            _clear = new FluentButton
            {
                Text = "清除已保存凭据",
                Size = new Size(160, 38),
                Visible = false
            };

            _save.Click += delegate { Save(); };
            _cancel.Click += delegate { DialogResult = DialogResult.Cancel; Close(); };
            _clear.Click += delegate { ClearSaved(); };

            Controls.Add(_heading);
            Controls.Add(_hint);
            Controls.Add(_accountLabel);
            Controls.Add(_account);
            Controls.Add(_passwordLabel);
            Controls.Add(_password);
            Controls.Add(_status);
            Controls.Add(_save);
            Controls.Add(_cancel);
            Controls.Add(_clear);

            Load += delegate
            {
                ApplyScale();
                Native.ApplyDarkWindow(Handle);
                ChildCredential saved = null;
                try { saved = CredentialStore.Load(); }
                catch (Exception ex)
                {
                    _status.Text = "读取已保存凭据失败，可直接重新保存。";
                    _status.ForeColor = Theme.Warn;
                    _status.Tag = ex;
                }

                _account.Text = saved != null ? saved.UserName : CredentialStore.CurrentAccountName();
                if (saved != null)
                {
                    _status.Text = "已保存凭据；留空密码可取消。";
                    _clear.Visible = true;
                }
                Activate();
                _account.Focus();
                _account.SelectAll();
            };
            Shown += delegate { Native.ApplyDarkWindow(Handle); };
        }

        private void ApplyScale()
        {
            _scale = Math.Max(1f, Native.GetDpiForWindow(Handle) / 96f);
            int S(int value) { return (int)Math.Round(value * _scale); }

            ClientSize = new Size(S(600), S(400));
            int field = S(600 - 48);
            _heading.Location = new Point(S(24), S(20));
            _hint.Location = new Point(S(24), S(58));
            _hint.Size = new Size(field, S(90));
            _accountLabel.Location = new Point(S(24), S(158));
            _account.Location = new Point(S(24), S(182));
            _account.Size = new Size(field, S(34));
            _passwordLabel.Location = new Point(S(24), S(228));
            _password.Location = new Point(S(24), S(252));
            _password.Size = new Size(field, S(34));
            _status.Location = new Point(S(24), S(296));
            _status.Size = new Size(field, S(28));
            _clear.Location = new Point(S(24), S(340));
            _clear.Size = new Size(S(160), S(38));
            _save.Size = new Size(S(220), S(38));
            _cancel.Size = new Size(S(80), S(38));
            _cancel.Location = new Point(ClientSize.Width - S(24) - _cancel.Width, S(340));
            _save.Location = new Point(_cancel.Left - S(12) - _save.Width, S(340));
            CenterToScreen();
        }

        private void Save()
        {
            string account = _account.Text.Trim();
            string password = _password.Text;
            if (account.Length == 0)
            {
                _status.Text = "请填写 Windows 账户。";
                _status.ForeColor = Theme.Warn;
                _account.Focus();
                return;
            }
            if (password.Length == 0)
            {
                _status.Text = "请输入账户密码，或取消。";
                _status.ForeColor = Theme.Warn;
                _password.Focus();
                return;
            }

            try
            {
                CredentialStore.Save(account, password);
                _password.Clear();
                DialogResult = DialogResult.OK;
                Close();
            }
            catch (Exception ex)
            {
                _status.Text = "保存失败：" + ex.GetBaseException().Message;
                _status.ForeColor = Theme.Warn;
            }
        }

        private void ClearSaved()
        {
            try
            {
                CredentialStore.Delete();
                _clear.Visible = false;
                _status.Text = "已清除保存的子会话凭据。";
                _status.ForeColor = Theme.Muted;
            }
            catch (Exception ex)
            {
                _status.Text = "清除失败：" + ex.GetBaseException().Message;
                _status.ForeColor = Theme.Warn;
            }
        }

        public static int Run()
        {
            using (var dialog = new CredentialDialog())
                return dialog.ShowDialog() == DialogResult.OK ? 0 : 1;
        }
    }
}
