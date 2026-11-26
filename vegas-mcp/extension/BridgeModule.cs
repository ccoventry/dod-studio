// DoD Studio VEGAS bridge (#1050): a VEGAS Pro application extension that
// listens on a named pipe for one JSON command per line and runs it with the
// VEGAS scripting API. The MCP wrapper (../mcp/vegas_mcp.py) forwards tool
// calls here.
//
// VEGAS loads an application extension into every VEGAS the user starts,
// their own editing sessions included. The bridge stays silent in all of
// them: it listens only in a VEGAS whose environment says it was started for
// the bridge (Session.ForBridge), so nothing can reach the project someone
// is editing. Each such VEGAS gets its own pipe, named after its process id,
// so several can run beside the user's.
//
// VEGAS's API is only safe on its UI thread, so the pipe thread hands each
// command to UiQueue, which runs it there, and waits for it.
// C# 5 on purpose: it builds with the csc.exe that ships with .NET Framework.
using System;
using System.Collections;
using System.Collections.Generic;
using System.IO;
using System.IO.Pipes;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;
using System.Windows.Forms;
using ScriptPortal.Vegas;

namespace DodStudio.VegasBridge {

public class BridgeModule : ICustomCommandModule {
    public const string Version = "0.5";

    Vegas vegas;
    UiQueue uiThread;
    Commands commands;
    Thread server;
    CustomCommand menuCommand;
    DialogAnswerer dialogs;

    public void InitializeModule(Vegas v) {
        vegas = v;
        if (!Session.ForBridge) {
            Log.Write("VEGAS " + v.Version + " PID " + Session.Pid + ": not started for the bridge (" + Session.WhyOff +
                      "), not listening");
            return;
        }
        Log.Write("VEGAS " + v.Version + " PID " + Session.Pid + ": listening on " + Session.PipeName +
                  ", work folder " + Session.Root + ", AppDomain " + AppDomain.CurrentDomain.FriendlyName);
        OwnerWatch.Start();
        uiThread = new UiQueue();
        dialogs = new DialogAnswerer();
        commands = new Commands(v, dialogs);
        server = new Thread(Serve);
        server.IsBackground = true;
        server.Name = "dodstudio-vegas pipe";
        server.Start();
    }

    // One Tools > Extensions menu item that only reports the bridge's state,
    // so the user can see it loaded and that it is off in their own VEGAS.
    public ICollection GetCustomCommands() {
        menuCommand = new CustomCommand(CommandCategory.Tools, "DodStudioVegasBridge");
        menuCommand.DisplayName = Session.ForBridge ? "DoD Studio bridge (listening)" : "DoD Studio bridge (off in this VEGAS)";
        menuCommand.Invoked += delegate(object s, EventArgs e) {
            string state = Session.ForBridge
                ? "is listening on \\\\.\\pipe\\" + Session.PipeName + ".\nWork folder: " + Session.Root
                : "is off in this VEGAS: it listens only in a VEGAS started for it, never in one you edit in.";
            MessageBox.Show("DoD Studio bridge " + Version + " " + state + "\nLog: " + Log.Path, "DoD Studio bridge");
        };
        return new CustomCommand[] { menuCommand };
    }

    static PipeSecurity CurrentUserOnly() {
        PipeSecurity ps = new PipeSecurity();
        ps.AddAccessRule(new PipeAccessRule(WindowsIdentity.GetCurrent().User,
                                            PipeAccessRights.FullControl, AccessControlType.Allow));
        return ps;
    }

    void Serve() {
        while (true) {
            try {
                using (NamedPipeServerStream pipe = new NamedPipeServerStream(
                           Session.PipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
                           PipeOptions.None, 1 << 16, 1 << 16, CurrentUserOnly())) {
                    pipe.WaitForConnection();
                    Log.Write("client connected");
                    StreamReader reader = new StreamReader(pipe, new UTF8Encoding(false));
                    StreamWriter writer = new StreamWriter(pipe, new UTF8Encoding(false));
                    writer.AutoFlush = true;
                    string line;
                    while ((line = reader.ReadLine()) != null) {
                        if (line.Trim().Length == 0) continue;
                        writer.WriteLine(Handle(line));
                    }
                    Log.Write("client disconnected");
                }
            } catch (Exception e) {
                Log.Write("pipe error: " + e.Message);
                Thread.Sleep(500);
            }
        }
    }

    string Handle(string line) {
        JavaScriptSerializer json = new JavaScriptSerializer();
        json.MaxJsonLength = int.MaxValue;
        Dictionary<string, object> reply = new Dictionary<string, object>();
        object id = null;
        try {
            Dictionary<string, object> req = json.Deserialize<Dictionary<string, object>>(line);
            req.TryGetValue("id", out id);
            object cmdObj;
            if (!req.TryGetValue("cmd", out cmdObj) || !(cmdObj is string)) throw new BridgeError("request has no \"cmd\"");
            string cmd = (string)cmdObj;
            object argsObj;
            req.TryGetValue("args", out argsObj);
            Args args = new Args(argsObj as Dictionary<string, object>);
            Log.Write("cmd " + cmd + " " + line.Length + " bytes");
            int seen = dialogs.Count;
            object result = null;
            Exception error = null;
            try {
                uiThread.Run(delegate {
                    try { result = commands.Run(cmd, args); } catch (Exception e) { error = e; Log.Write(cmd + " failed: " + e); }
                });
                // The UI thread is free between steps, so VEGAS's message loop
                // runs; no other bridge command starts meanwhile, since this
                // pipe thread handles one at a time.
                while (error == null && result is LaterStep) {
                    LaterStep later = (LaterStep)result;
                    Thread.Sleep(later.AfterMs);
                    uiThread.Run(delegate {
                        try { result = later.Next(); } catch (Exception e) { error = e; Log.Write(cmd + " failed: " + e); }
                    });
                }
            } finally {
                // The dialogs VEGAS raised during this command and how each was
                // answered, so the caller learns e.g. that media went offline.
                List<string> answered = dialogs.Since(seen);
                if (answered.Count > 0) reply["dialogs"] = answered;
            }
            if (error != null) throw error;
            reply["ok"] = true;
            reply["result"] = result;
        } catch (Exception e) {
            Exception inner = e is System.Reflection.TargetInvocationException && e.InnerException != null ? e.InnerException : e;
            reply["ok"] = false;
            reply["error"] = inner is BridgeError ? inner.Message : inner.GetType().Name + ": " + inner.Message;
            Log.Write("error: " + inner);
        }
        if (id != null) reply["id"] = id;
        return json.Serialize(reply);
    }
}

public class BridgeError : Exception {
    public BridgeError(string message) : base(message) { }
}

// A command's arguments with typed, named lookups; a missing required one
// fails with its name.
public class Args {
    readonly Dictionary<string, object> d;
    public Args(Dictionary<string, object> d) { this.d = d ?? new Dictionary<string, object>(); }
    public bool Has(string k) { return d.ContainsKey(k) && d[k] != null; }
    public object Raw(string k) { return Has(k) ? d[k] : null; }
    public string Str(string k) { if (!Has(k)) throw new BridgeError("missing argument \"" + k + "\""); return Convert.ToString(d[k]); }
    public string Str(string k, string def) { return Has(k) ? Convert.ToString(d[k]) : def; }
    public double Num(string k) { if (!Has(k)) throw new BridgeError("missing argument \"" + k + "\""); return Convert.ToDouble(d[k]); }
    public double Num(string k, double def) { return Has(k) ? Convert.ToDouble(d[k]) : def; }
    public int Int(string k) { return (int)Num(k); }
    public int Int(string k, int def) { return Has(k) ? (int)Num(k) : def; }
    public bool Bool(string k, bool def) { return Has(k) ? Convert.ToBoolean(d[k]) : def; }
    public Dictionary<string, object> Obj(string k) { return Raw(k) as Dictionary<string, object>; }
    // A JSON array argument (the serializer gives an ArrayList); a missing one fails with its name.
    public IList List(string k) {
        IList l = Raw(k) as IList;
        if (l == null) throw new BridgeError("argument \"" + k + "\" must be a list");
        return l;
    }
    // Each object of a JSON array argument as Args, so its fields get the same typed lookups.
    public List<Args> Items(string k) {
        List<Args> r = new List<Args>();
        foreach (object o in List(k)) {
            Dictionary<string, object> d = o as Dictionary<string, object>;
            if (d == null) throw new BridgeError("each item of \"" + k + "\" must be an object");
            r.Add(new Args(d));
        }
        return r;
    }
}

public static class Log {
    public static readonly string Path = System.IO.Path.Combine(System.IO.Path.GetTempPath(), "dodstudio_vegas_bridge.log");
    static readonly object gate = new object();
    public static void Write(string s) {
        lock (gate) {
            try {
                FileInfo f = new FileInfo(Path);
                if (f.Exists && f.Length > 4 * 1024 * 1024) f.Delete();
                // Every bridge VEGAS writes here, so each line names its PID.
                File.AppendAllText(Path, DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss.fff") + " [" + Session.Pid + "] " + s + Environment.NewLine);
            } catch { }
        }
    }
}

}
