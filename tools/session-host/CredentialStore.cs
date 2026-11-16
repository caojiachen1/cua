using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
using System.Security.Principal;
using System.Text;

namespace CuaSessionHost
{
    internal sealed class ChildCredential
    {
        public ChildCredential(string userName, string password)
        {
            UserName = userName;
            Password = password;
        }

        public string UserName { get; private set; }
        public string Password { get; private set; }

        // RDP expects Domain + UserName. Microsoft/Azure identities stay qualified
        // in UserName; a machine or AD prefix becomes Domain.
        public void RdpIdentity(out string domain, out string userName)
        {
            int split = UserName.IndexOf('\\');
            if (split <= 0)
            {
                domain = "";
                userName = UserName;
                return;
            }

            string prefix = UserName.Substring(0, split);
            if (prefix.Equals("MicrosoftAccount", StringComparison.OrdinalIgnoreCase) ||
                prefix.Equals("AzureAD", StringComparison.OrdinalIgnoreCase))
            {
                domain = "";
                userName = UserName;
                return;
            }

            domain = prefix;
            userName = UserName.Substring(split + 1);
        }
    }

    // Windows Credential Manager, encrypted for this Windows user. The password is
    // only read for an RDP logon attempt and is never put in argv, pipes or logs.
    internal static class CredentialStore
    {
        private const string Target = "cua-child-session";
        private const uint CredTypeGeneric = 1;
        private const uint CredPersistLocalMachine = 2;
        private const int ErrorNotFound = 1168;

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct CREDENTIAL
        {
            public uint Flags;
            public uint Type;
            public IntPtr TargetName;
            public IntPtr Comment;
            public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
            public uint CredentialBlobSize;
            public IntPtr CredentialBlob;
            public uint Persist;
            public uint AttributeCount;
            public IntPtr Attributes;
            public IntPtr TargetAlias;
            public IntPtr UserName;
        }

        [DllImport("advapi32.dll", EntryPoint = "CredReadW", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool CredRead(string target, uint type, uint flags, out IntPtr credential);

        [DllImport("advapi32.dll", EntryPoint = "CredWriteW", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool CredWrite(ref CREDENTIAL credential, uint flags);

        [DllImport("advapi32.dll", EntryPoint = "CredDeleteW", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool CredDelete(string target, uint type, uint flags);

        [DllImport("advapi32.dll")]
        private static extern void CredFree(IntPtr credential);

        public static string CurrentAccountName()
        {
            using (var identity = WindowsIdentity.GetCurrent())
                return identity.Name ?? Environment.UserName;
        }

        public static ChildCredential Load()
        {
            IntPtr raw;
            if (!CredRead(Target, CredTypeGeneric, 0, out raw))
            {
                int error = Marshal.GetLastWin32Error();
                if (error == ErrorNotFound) return null;
                throw new Win32Exception(error, "CredRead failed");
            }

            try
            {
                var credential = (CREDENTIAL)Marshal.PtrToStructure(raw, typeof(CREDENTIAL));
                string user = Marshal.PtrToStringUni(credential.UserName) ?? "";
                string password = credential.CredentialBlobSize > 1
                    ? Marshal.PtrToStringUni(credential.CredentialBlob,
                        (int)(credential.CredentialBlobSize / 2)) ?? ""
                    : "";
                if (string.IsNullOrWhiteSpace(user) || password.Length == 0) return null;
                return new ChildCredential(user, password);
            }
            finally
            {
                CredFree(raw);
            }
        }

        public static void Save(string userName, string password)
        {
            userName = (userName ?? "").Trim();
            if (userName.Length == 0 || string.IsNullOrEmpty(password))
                throw new ArgumentException("account and password must not be empty");

            byte[] secret = Encoding.Unicode.GetBytes(password);
            IntPtr target = IntPtr.Zero;
            IntPtr comment = IntPtr.Zero;
            IntPtr user = IntPtr.Zero;
            IntPtr blob = IntPtr.Zero;
            try
            {
                target = Marshal.StringToCoTaskMemUni(Target);
                comment = Marshal.StringToCoTaskMemUni("cua child session automatic logon");
                user = Marshal.StringToCoTaskMemUni(userName);
                blob = Marshal.AllocHGlobal(secret.Length);
                Marshal.Copy(secret, 0, blob, secret.Length);

                var credential = new CREDENTIAL
                {
                    Type = CredTypeGeneric,
                    TargetName = target,
                    Comment = comment,
                    CredentialBlobSize = (uint)secret.Length,
                    CredentialBlob = blob,
                    Persist = CredPersistLocalMachine,
                    UserName = user
                };

                if (!CredWrite(ref credential, 0))
                    throw new Win32Exception(Marshal.GetLastWin32Error(), "CredWrite failed");
            }
            finally
            {
                if (blob != IntPtr.Zero)
                {
                    Marshal.Copy(new byte[secret.Length], 0, blob, secret.Length);
                    Marshal.FreeHGlobal(blob);
                }
                if (user != IntPtr.Zero) Marshal.FreeCoTaskMem(user);
                if (comment != IntPtr.Zero) Marshal.FreeCoTaskMem(comment);
                if (target != IntPtr.Zero) Marshal.FreeCoTaskMem(target);
                Array.Clear(secret, 0, secret.Length);
            }
        }

        public static void Delete()
        {
            if (!CredDelete(Target, CredTypeGeneric, 0))
            {
                int error = Marshal.GetLastWin32Error();
                if (error != ErrorNotFound)
                    throw new Win32Exception(error, "CredDelete failed");
            }
        }
    }
}
