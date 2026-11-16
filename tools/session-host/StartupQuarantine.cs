using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Threading;
using System.Windows.Forms;
using System.Web.Script.Serialization;
using Microsoft.Win32;

namespace CuaSessionHost
{
    // Windows has no per-session startup switch. To keep the user's startup apps
    // out of the child session, briefly ignore the current user's Run lists and
    // disable that user's non-Microsoft logon tasks, then restore them as soon as
    // the child agent is ready. The parent session is already logged on, so its
    // running apps are not affected.
    internal sealed class StartupQuarantine
    {
        private const string PolicyPath = @"Software\Microsoft\Windows\CurrentVersion\Policies\Explorer";
        private const string BackupName = "startup-quarantine.json";
        private static readonly string[] PolicyNames =
        {
            "DisableCurrentUserRun",
            "DisableCurrentUserRunOnce",
            "DisableLocalMachineRun",
            "DisableLocalMachineRunOnce"
        };

        private sealed class Backup
        {
            public Dictionary<string, int?> Policies { get; set; }
            public List<TaskBackup> Tasks { get; set; }
        }

        private sealed class TaskBackup
        {
            public string Path { get; set; }
            public string Folder { get; set; }
            public string Name { get; set; }
        }

        private static readonly JavaScriptSerializer Ser =
            new JavaScriptSerializer { MaxJsonLength = 4 * 1024 * 1024 };

        private readonly string _backupPath;
        private Dictionary<string, int?> _policies;
        private List<TaskBackup> _tasks;
        private bool _active;

        public StartupQuarantine(string dataDir)
        {
            Directory.CreateDirectory(dataDir);
            _backupPath = Path.Combine(dataDir, BackupName);
        }

        public bool Active
        {
            get { return _active; }
        }

        public void Activate()
        {
            if (_active) return;

            _policies = ReadPolicies();
            _tasks = FindUserLogonTasks();
            WriteBackup(new Backup { Policies = _policies, Tasks = _tasks });
            StartWatchdog(Path.GetDirectoryName(_backupPath));

            try
            {
                ApplyPolicies();
                DisableTasks();
                _active = true;
            }
            catch
            {
                try { Restore(); } catch { }
                throw;
            }
        }

        public void Restore()
        {
            Backup backup = ReadBackup();
            if (backup == null) return;
            RestorePolicies(backup.Policies);
            RestoreTasks(backup.Tasks);
            _active = false;
            try { File.Delete(_backupPath); } catch { }
        }

        private static void StartWatchdog(string dataDir)
        {
            try
            {
                string exe = Application.ExecutablePath;
                string args = "--restore-startup --data-dir \"" + dataDir.Replace("\"", "\\\"") + "\"";
                Process.Start(new ProcessStartInfo(exe, args)
                {
                    UseShellExecute = false,
                    CreateNoWindow = true
                });
            }
            catch { }
        }

        public static int RestoreAfterDelay(string dataDir, int seconds)
        {
            string backup = Path.Combine(dataDir, BackupName);
            for (int i = 0; i < seconds; i++)
            {
                if (!File.Exists(backup)) return 0;
                Thread.Sleep(1000);
            }
            try { RestoreStale(dataDir); } catch { }
            return 0;
        }

        public static void RestoreStale(string dataDir)
        {
            string path = Path.Combine(dataDir, BackupName);
            if (!File.Exists(path)) return;
            Backup backup = ReadBackup(path);
            if (backup == null) return;
            RestorePolicies(backup.Policies);
            RestoreTasks(backup.Tasks);
            try { File.Delete(path); } catch { }
        }

        private static Dictionary<string, int?> ReadPolicies()
        {
            var result = new Dictionary<string, int?>();
            using (RegistryKey key = Registry.CurrentUser.OpenSubKey(PolicyPath, false))
            {
                foreach (string name in PolicyNames)
                {
                    object value = key == null ? null : key.GetValue(name, null);
                    if (value == null) result[name] = null;
                    else
                    {
                        try { result[name] = Convert.ToInt32(value); }
                        catch { result[name] = null; }
                    }
                }
            }
            return result;
        }

        private static void ApplyPolicies()
        {
            using (RegistryKey key = Registry.CurrentUser.CreateSubKey(PolicyPath))
            {
                if (key == null) throw new InvalidOperationException("cannot open user Explorer policy key");
                foreach (string name in PolicyNames)
                    key.SetValue(name, 1, RegistryValueKind.DWord);
            }
        }

        private static void RestorePolicies(Dictionary<string, int?> policies)
        {
            if (policies == null) return;
            using (RegistryKey key = Registry.CurrentUser.CreateSubKey(PolicyPath))
            {
                if (key == null) return;
                foreach (var pair in policies)
                {
                    if (pair.Value.HasValue)
                        key.SetValue(pair.Key, pair.Value.Value, RegistryValueKind.DWord);
                    else
                        key.DeleteValue(pair.Key, false);
                }
            }
        }

        private static List<TaskBackup> FindUserLogonTasks()
        {
            var result = new List<TaskBackup>();
            object serviceObject = null;
            object rootObject = null;
            try
            {
                Type schedulerType = Type.GetTypeFromProgID("Schedule.Service");
                if (schedulerType == null) return result;
                serviceObject = Activator.CreateInstance(schedulerType);
                dynamic service = serviceObject;
                service.Connect();
                rootObject = service.GetFolder("\\");
                dynamic root = rootObject;
                WalkFolder(root, result);
            }
            catch
            {
                // A task query failure must not prevent the child session.
            }
            finally
            {
                Release(rootObject);
                Release(serviceObject);
            }
            return result;
        }

        private static void WalkFolder(dynamic folder, List<TaskBackup> result)
        {
            object tasksObject = null;
            object foldersObject = null;
            try
            {
                tasksObject = folder.GetTasks(1);
                dynamic tasks = tasksObject;
                int count = Convert.ToInt32(tasks.Count);
                for (int i = 1; i <= count; i++)
                {
                    object taskObject = null;
                    try
                    {
                        taskObject = tasks.Item(i);
                        dynamic task = taskObject;
                        string path = (string)task.Path;
                        if (path == null || path.StartsWith(@"\Microsoft\", StringComparison.OrdinalIgnoreCase))
                            continue;
                        if (!Convert.ToBoolean(task.Enabled)) continue;
                        dynamic definition = task.Definition;
                        string userId = (string)definition.Principal.UserId;
                        if (!IsCurrentUser(userId)) continue;
                        if (!HasLogonTrigger(definition.Triggers)) continue;

                        int slash = path.LastIndexOf('\\');
                        result.Add(new TaskBackup
                        {
                            Path = path,
                            Folder = slash <= 0 ? "\\" : path.Substring(0, slash),
                            Name = path.Substring(slash + 1)
                        });
                    }
                    catch { }
                    finally
                    {
                        Release(taskObject);
                    }
                }
            }
            catch { }
            finally
            {
                Release(tasksObject);
                if (foldersObject != null) Release(foldersObject);
                foldersObject = null;
            }

            try
            {
                foldersObject = folder.GetFolders(0);
                dynamic folders = foldersObject;
                int count = Convert.ToInt32(folders.Count);
                for (int i = 1; i <= count; i++)
                {
                    object childObject = null;
                    try
                    {
                        childObject = folders.Item(i);
                        WalkFolder(childObject, result);
                    }
                    finally
                    {
                        Release(childObject);
                    }
                }
            }
            catch { }
            finally
            {
                Release(foldersObject);
            }
        }

        private static bool HasLogonTrigger(dynamic triggers)
        {
            int count = Convert.ToInt32(triggers.Count);
            for (int i = 1; i <= count; i++)
            {
                object triggerObject = null;
                try
                {
                    triggerObject = triggers.Item(i);
                    dynamic trigger = triggerObject;
                    if (Convert.ToInt32(trigger.Type) == 9) // TASK_TRIGGER_LOGON
                        return true;
                }
                catch { }
                finally
                {
                    Release(triggerObject);
                }
            }
            return false;
        }

        private static bool IsCurrentUser(string userId)
        {
            if (string.IsNullOrWhiteSpace(userId)) return false;
            using (WindowsIdentity identity = WindowsIdentity.GetCurrent())
            {
                if (string.Equals(userId, identity.Name, StringComparison.OrdinalIgnoreCase))
                    return true;
                if (string.Equals(userId, Environment.UserName, StringComparison.OrdinalIgnoreCase))
                    return true;
                if (userId.EndsWith("\\" + Environment.UserName, StringComparison.OrdinalIgnoreCase))
                    return true;
                try
                {
                    var sid = new SecurityIdentifier(userId);
                    if (sid.Equals(identity.User)) return true;
                }
                catch { }
                try
                {
                    var account = new NTAccount(userId);
                    var sid = account.Translate(typeof(SecurityIdentifier)) as SecurityIdentifier;
                    if (sid != null && identity.User != null && sid.Equals(identity.User))
                        return true;
                }
                catch { }
            }
            return false;
        }

        private void DisableTasks()
        {
            object serviceObject = null;
            try
            {
                Type schedulerType = Type.GetTypeFromProgID("Schedule.Service");
                if (schedulerType == null) return;
                serviceObject = Activator.CreateInstance(schedulerType);
                dynamic service = serviceObject;
                service.Connect();
                foreach (TaskBackup saved in _tasks)
                {
                    object folderObject = null;
                    object taskObject = null;
                    try
                    {
                        folderObject = service.GetFolder(saved.Folder);
                        dynamic folder = folderObject;
                        taskObject = folder.GetTask(saved.Name);
                        dynamic task = taskObject;
                        task.Enabled = false;
                    }
                    catch { }
                    finally
                    {
                        Release(taskObject);
                        Release(folderObject);
                    }
                }
            }
            catch { }
            finally
            {
                Release(serviceObject);
            }
        }

        private static void RestoreTasks(List<TaskBackup> tasks)
        {
            if (tasks == null || tasks.Count == 0) return;
            object serviceObject = null;
            try
            {
                Type schedulerType = Type.GetTypeFromProgID("Schedule.Service");
                if (schedulerType == null) return;
                serviceObject = Activator.CreateInstance(schedulerType);
                dynamic service = serviceObject;
                service.Connect();
                foreach (TaskBackup saved in tasks)
                {
                    object folderObject = null;
                    object taskObject = null;
                    try
                    {
                        folderObject = service.GetFolder(saved.Folder);
                        dynamic folder = folderObject;
                        taskObject = folder.GetTask(saved.Name);
                        dynamic task = taskObject;
                        task.Enabled = true;
                    }
                    catch { }
                    finally
                    {
                        Release(taskObject);
                        Release(folderObject);
                    }
                }
            }
            catch { }
            finally
            {
                Release(serviceObject);
            }
        }

        private void WriteBackup(Backup backup)
        {
            File.WriteAllText(_backupPath, Ser.Serialize(backup));
        }

        private Backup ReadBackup()
        {
            return ReadBackup(_backupPath);
        }

        private static Backup ReadBackup(string path)
        {
            try
            {
                return Ser.Deserialize<Backup>(File.ReadAllText(path));
            }
            catch
            {
                return null;
            }
        }

        private static void Release(object value)
        {
            if (value == null || !Marshal.IsComObject(value)) return;
            try { Marshal.FinalReleaseComObject(value); } catch { }
        }
    }
}
