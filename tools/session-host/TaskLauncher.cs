using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Principal;

namespace CuaSessionHost
{
    internal static class TaskLauncher
    {
        // Run a program in an existing session. The temporary task is deleted
        // after launch; its running instance survives. TASK_RUN_USE_SESSION is
        // the only supported way to target a child session without SYSTEM.
        public static void Launch(uint sessionId, string exe, string workingDir, string dataDir)
        {
            LaunchArgs(sessionId, exe, "__agent --data-dir \"" + dataDir + "\"", workingDir);
        }

        public static void LaunchArgs(uint sessionId, string exe, string arguments, string workingDir)
        {
            Type schedulerType = Type.GetTypeFromProgID("Schedule.Service");
            if (schedulerType == null) throw new InvalidOperationException("Schedule.Service unavailable");

            string taskName = "cua-child-agent-" + Guid.NewGuid().ToString("N");
            string account;
            using (var identity = WindowsIdentity.GetCurrent()) account = identity.Name;

            object schedulerObj = null;
            object rootObj = null;
            object defObj = null;
            object actionObj = null;
            object registeredObj = null;
            object runningObj = null;
            bool registered = false;
            try
            {
                schedulerObj = Activator.CreateInstance(schedulerType);
                dynamic scheduler = schedulerObj;
                scheduler.Connect();

                rootObj = scheduler.GetFolder("\\");
                dynamic root = rootObj;
                defObj = scheduler.NewTask(0);
                dynamic def = defObj;

                def.RegistrationInfo.Author = "cua";
                def.RegistrationInfo.Description = "cua child-session agent";
                def.Settings.Enabled = true;
                def.Settings.Hidden = true;
                def.Settings.AllowDemandStart = true;
                def.Settings.DisallowStartIfOnBatteries = false;
                def.Settings.StopIfGoingOnBatteries = false;
                def.Settings.ExecutionTimeLimit = "PT0S";
                def.Principal.UserId = account;
                def.Principal.LogonType = 3; // TASK_LOGON_INTERACTIVE_TOKEN
                def.Principal.RunLevel = 0;  // least privilege: same as the user's desktop

                actionObj = def.Actions.Create(0); // TASK_ACTION_EXEC
                dynamic action = actionObj;
                action.Path = exe;
                action.Arguments = arguments;
                action.WorkingDirectory = workingDir;

                registeredObj = root.RegisterTaskDefinition(
                    taskName, defObj, 2 /* CREATE */, account, null,
                    3 /* TASK_LOGON_INTERACTIVE_TOKEN */, null);
                registered = true;

                dynamic task = registeredObj;
                runningObj = task.RunEx(
                    null,
                    0x4, // TASK_RUN_USE_SESSION
                    unchecked((int)sessionId),
                    null);
                if (runningObj == null)
                    throw new InvalidOperationException("Task Scheduler returned no running instance");
            }
            finally
            {
                if (registered && rootObj != null)
                {
                    try { ((dynamic)rootObj).DeleteTask(taskName, 0); }
                    catch (COMException) { }
                }
                Release(runningObj);
                Release(registeredObj);
                Release(actionObj);
                Release(defObj);
                Release(rootObj);
                Release(schedulerObj);
            }
        }

        private static void Release(object value)
        {
            if (value != null && Marshal.IsComObject(value))
            {
                try { Marshal.FinalReleaseComObject(value); } catch { }
            }
        }
    }
}
