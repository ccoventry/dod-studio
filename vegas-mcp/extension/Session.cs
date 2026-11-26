// Whether this VEGAS was started for the bridge, and the one folder it may
// write in.
//
// VEGAS loads the extension into every VEGAS, the user's own editing
// sessions too, so the bridge needs a mark only its own launches carry: the
// launcher (vegas_mcp.py's launch) sets DODSTUDIO_VEGAS_BRIDGE=1 and
// DODSTUDIO_VEGAS_ROOT in the environment of the VEGAS it starts. A VEGAS
// started from the Start menu, a shortcut or a .veg never has them, so the
// bridge opens no pipe there and nothing can reach that project.
//
// Every file the bridge writes (save_as, render) and every project or script
// it opens lives under DODSTUDIO_VEGAS_ROOT. A project outside it is copied
// in and the copy opened, so the user's own .veg files are never opened or
// saved.
using System;
using System.IO;

namespace DodStudio.VegasBridge {

public static class Session {
    public const string FlagVariable = "DODSTUDIO_VEGAS_BRIDGE";
    public const string RootVariable = "DODSTUDIO_VEGAS_ROOT";

    public static readonly int Pid = System.Diagnostics.Process.GetCurrentProcess().Id;
    // One pipe per VEGAS process, so several bridge VEGAS can run at once;
    // the MCP side finds them by this prefix.
    public static readonly string PipeName = "dodstudio-vegas-" + Pid;
    public static readonly string Root;
    public static readonly string WhyOff;

    static Session() {
        string flag = Environment.GetEnvironmentVariable(FlagVariable);
        string root = Environment.GetEnvironmentVariable(RootVariable);
        if (flag != "1") { WhyOff = FlagVariable + " is not 1"; return; }
        if (string.IsNullOrEmpty(root) || !Path.IsPathRooted(root)) { WhyOff = RootVariable + " is not a full path"; return; }
        string full = Path.GetFullPath(root).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
        if (!Directory.Exists(full)) { WhyOff = RootVariable + " " + full + " does not exist"; return; }
        Root = full;
    }

    public static bool ForBridge { get { return Root != null; } }

    public static bool UnderRoot(string path) {
        string full = Path.GetFullPath(path);
        return full.StartsWith(Root + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase);
    }

    // The full path, or a refusal naming the work folder.
    public static string RequireUnderRoot(string path, string what) {
        string full = Path.GetFullPath(path);
        if (!UnderRoot(full))
            throw new BridgeError(what + " must be inside the work folder " + Root + " (got " + full + ")");
        return full;
    }

    // A path under Root\copies for a copy of `source` that does not exist
    // yet: "name.veg", then "name (2).veg" and so on.
    public static string FreshCopyPath(string source) {
        string dir = Path.Combine(Root, "copies");
        Directory.CreateDirectory(dir);
        string stem = Path.GetFileNameWithoutExtension(source);
        string ext = Path.GetExtension(source);
        string path = Path.Combine(dir, stem + ext);
        for (int n = 2; File.Exists(path); n++) path = Path.Combine(dir, stem + " (" + n + ")" + ext);
        return path;
    }
}

}
