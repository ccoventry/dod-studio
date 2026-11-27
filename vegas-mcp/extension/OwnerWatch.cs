// Ends this bridge VEGAS once the process that launched it (the MCP server)
// has gone, however it went: the server ends its VEGAS itself when it can,
// but one killed outright (Task Manager, a crashed Claude Code) cannot, and
// its VEGAS would stay up, minimised, for good.
//
// The owner is named by its PID and creation time (Session.OwnerPid,
// OwnerStart). A handle to it is opened once and waited on; holding it keeps
// Windows from giving that PID to another process meanwhile, and the start
// time shows the PID still meant the launcher when the handle was opened.
//
// Ending: VEGAS deletes its autosave (<pid as 8 hex digits>.autosave.veg)
// only on a clean exit, and a leftover one makes the next VEGAS offer to
// restore it. VEGAS does not hold that file open between saves, and a VEGAS
// that kills itself leaves it (both measured on build 315), so it is deleted
// first, then the process is terminated. Not vegas.Exit(), which can ask
// "cancel the background operation?" on screen. An autosave VEGAS writes in
// the moment between the two is left over; the launcher then refuses to start
// beside it, so it is never restored by accident.
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;

namespace DodStudio.VegasBridge {

public static class OwnerWatch {
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
    [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr h, uint ms);
    [DllImport("kernel32.dll")] static extern bool GetProcessTimes(IntPtr h, out long creation, out long exit, out long kernel, out long user);
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll")] static extern bool TerminateProcess(IntPtr h, uint code);
    const uint SYNCHRONIZE = 0x00100000;
    const uint PROCESS_QUERY_LIMITED_INFORMATION = 0x1000;
    const uint INFINITE = 0xFFFFFFFF;
    const uint ExitCode = 1;

    // The handle stays open for the whole session, on purpose.
    static IntPtr owner;

    public static void Start() {
        if (Session.OwnerPid == 0) {
            Log.Write("no owner process given: this VEGAS runs until it is ended");
            return;
        }
        owner = OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, false, Session.OwnerPid);
        if (owner == IntPtr.Zero) {
            EndSelf("owner PID " + Session.OwnerPid + " is not running (error " + Marshal.GetLastWin32Error() + ")");
            return;
        }
        long created, unused1, unused2, unused3;
        if (!GetProcessTimes(owner, out created, out unused1, out unused2, out unused3) || created != Session.OwnerStart) {
            EndSelf("PID " + Session.OwnerPid + " is no longer the process that launched this VEGAS");
            return;
        }
        Thread t = new Thread(Watch);
        t.IsBackground = true;
        t.Name = "dodstudio-vegas owner";
        t.Start();
        Log.Write("ends itself when owner PID " + Session.OwnerPid + " exits");
    }

    static void Watch() {
        WaitForSingleObject(owner, INFINITE);
        EndSelf("owner PID " + Session.OwnerPid + " has exited");
    }

    static void EndSelf(string why) {
        Log.Write(why + ": ending this VEGAS without saving");
        foreach (string path in AutosaveFiles()) {
            try {
                File.Delete(path);
                Log.Write("removed its autosave " + path);
            } catch (Exception e) {
                Log.Write("could not remove its autosave " + path + ": " + e.Message);
            }
        }
        TerminateProcess(GetCurrentProcess(), ExitCode);
    }

    // VEGAS 21's default autosave folder only, as the launcher checks it.
    static string[] AutosaveFiles() {
        string folder = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "VEGAS Pro", "21.0");
        if (!Directory.Exists(folder)) return new string[0];
        return Directory.GetFiles(folder, Session.Pid.ToString("x8") + ".autosave.veg*");
    }
}

}
