// Answers the dialogs this VEGAS raises, so none waits on the user's screen
// and no bridge command hangs on one. Runs for the whole session in a VEGAS
// started for the bridge, and looks only at windows of this VEGAS process:
// the user's own VEGAS beside it is never touched.
//
//   Missing media ("... could not be found"): "Ignore all missing files and
//     leave them offline", then OK (as frag-editor's vegas/dump_veg.cs does).
//   The first-render "Hint" box (OK only): OK.
//   "Save changes?": No. The bridge never saves in place.
//   Any other modal box still open after a few seconds: Cancel, else No,
//     else OK, else Close. Its text goes to the log and to the reply of the
//     command that raised it, so the next run can answer it properly.
//
// A dialog with a progress bar (a render's) is left alone: closing it would
// cancel the work it shows.
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

namespace DodStudio.VegasBridge {

public sealed class DialogAnswerer {
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern bool IsWindowEnabled(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern IntPtr SendMessageTimeout(IntPtr h, uint m, IntPtr w, IntPtr l, uint flags, uint ms, out IntPtr result);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern int GetWindowLong(IntPtr h, int index);
    [DllImport("user32.dll")] static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] static extern int GetDlgCtrlID(IntPtr h);
    const int GWL_STYLE = -16;
    const int BS_TYPEMASK = 0x0F;
    const int BS_PUSHBUTTON = 0x00;
    const int BS_DEFPUSHBUTTON = 0x01;
    const int BN_CLICKED = 0;
    const uint WM_COMMAND = 0x0111;
    const uint WM_NULL = 0x0000;
    const uint BM_CLICK = 0x00F5;
    const uint WM_CLOSE = 0x0010;
    const uint GW_OWNER = 4;
    const uint SMTO_ABORTIFHUNG = 0x0002;
    // How long an unknown modal box stays before it is closed: long enough
    // that a box VEGAS closes on its own is not touched.
    static readonly TimeSpan UnknownGrace = TimeSpan.FromSeconds(4);

    readonly uint pid = (uint)Session.Pid;
    readonly object gate = new object();
    readonly List<string> answered = new List<string>();
    // When each still-open unknown box was first seen, and boxes already
    // answered (a window takes a moment to close after its button).
    readonly Dictionary<IntPtr, DateTime> firstSeen = new Dictionary<IntPtr, DateTime>();
    readonly Dictionary<IntPtr, DateTime> done = new Dictionary<IntPtr, DateTime>();

    public DialogAnswerer() {
        Thread t = new Thread(Watch);
        t.IsBackground = true;
        t.Name = "dodstudio-vegas dialogs";
        t.Start();
    }

    public int Count { get { lock (gate) return answered.Count; } }

    public List<string> Since(int n) {
        lock (gate) return answered.GetRange(Math.Min(n, answered.Count), answered.Count - Math.Min(n, answered.Count));
    }

    public List<string> All() { lock (gate) return new List<string>(answered); }

    static string Text(IntPtr h) { StringBuilder s = new StringBuilder(1024); GetWindowText(h, s, 1024); return s.ToString(); }
    static string Cls(IntPtr h) { StringBuilder s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }

    // "&OK" and the Hint box's "DialogButtonOK" both read "ok".
    static string ButtonWord(string t) {
        string b = t.Replace("&", "").Trim().ToLowerInvariant();
        return b.StartsWith("dialogbutton") ? b.Substring("dialogbutton".Length) : b;
    }

    // A radio button or check box takes BM_CLICK, hidden or not. A push
    // button in a hidden (inactive) dialog ignores BM_CLICK, so the dialog
    // gets the WM_COMMAND the button would send (measured on the missing-media
    // box of a minimised VEGAS).
    static void Click(IntPtr button) {
        IntPtr unused;
        int kind = GetWindowLong(button, GWL_STYLE) & BS_TYPEMASK;
        IntPtr dialog = GetParent(button);
        if (kind == BS_PUSHBUTTON || kind == BS_DEFPUSHBUTTON) {
            if (dialog != IntPtr.Zero && !IsWindowVisible(dialog)) {
                IntPtr wParam = (IntPtr)((GetDlgCtrlID(button) & 0xFFFF) | (BN_CLICKED << 16));
                SendMessageTimeout(dialog, WM_COMMAND, wParam, button, SMTO_ABORTIFHUNG, 2000, out unused);
                // EndDialog only marks the box done; its message loop leaves
                // on the next posted message, and a hidden box in a minimised
                // VEGAS gets none (a sent message does not count), so the
                // command that opened it would wait forever. Post one.
                PostMessage(dialog, WM_NULL, IntPtr.Zero, IntPtr.Zero);
                return;
            }
        }
        SendMessageTimeout(button, BM_CLICK, IntPtr.Zero, IntPtr.Zero, SMTO_ABORTIFHUNG, 2000, out unused);
    }

    void Record(string what) {
        lock (gate) answered.Add(what);
        Log.Write("dialog: " + what);
    }

    void Watch() {
        while (true) {
            try { Scan(); } catch (Exception e) { Log.Write("dialog watcher: " + e.Message); }
            Thread.Sleep(300);
        }
    }

    void Scan() {
        DateTime now = DateTime.Now;
        HashSet<IntPtr> open = new HashSet<IntPtr>();
        EnumWindows(delegate(IntPtr h, IntPtr l) {
            uint owner;
            GetWindowThreadProcessId(h, out owner);
            if (owner != pid || Cls(h) != "#32770") return true;
            // The bridge VEGAS runs minimised, and Windows hides the windows a
            // minimised window owns, so its dialogs are hidden. A hidden one
            // counts while it is modal (its owner disabled); a hidden dialog
            // whose owner is enabled has ended or is kept for reuse.
            if (!IsWindowVisible(h)) {
                IntPtr o = GetWindow(h, GW_OWNER);
                if (o == IntPtr.Zero || IsWindowEnabled(o)) return true;
            }
            open.Add(h);
            DateTime when;
            if (done.TryGetValue(h, out when) && now - when < TimeSpan.FromSeconds(3)) return true;
            if (Answer(h, now)) done[h] = now;
            return true;
        }, IntPtr.Zero);
        // Forget windows that have closed (handles are reused).
        foreach (IntPtr h in new List<IntPtr>(firstSeen.Keys)) if (!open.Contains(h)) firstSeen.Remove(h);
        foreach (IntPtr h in new List<IntPtr>(done.Keys)) if (!open.Contains(h)) done.Remove(h);
    }

    // True when it pressed something.
    bool Answer(IntPtr h, DateTime now) {
        string title = Text(h);
        StringBuilder body = new StringBuilder();
        Dictionary<string, IntPtr> buttons = new Dictionary<string, IntPtr>();
        IntPtr ignoreAll = IntPtr.Zero;
        bool progress = false;
        EnumChildWindows(h, delegate(IntPtr c, IntPtr l) {
            string cls = Cls(c);
            string t = Text(c);
            if (cls.IndexOf("progress", StringComparison.OrdinalIgnoreCase) >= 0) progress = true;
            if (cls == "Button") {
                string word = ButtonWord(t);
                if (word.StartsWith("ignore all missing files")) ignoreAll = c;
                else if (!buttons.ContainsKey(word)) buttons[word] = c;
            } else if (t.Length > 0) {
                body.Append(body.Length > 0 ? " " : "").Append(t.Replace("\r", " ").Replace("\n", " "));
            }
            return true;
        }, IntPtr.Zero);
        string text = body.ToString();
        string lower = text.ToLowerInvariant();
        IntPtr b;

        if (lower.Contains("could not be found") && ignoreAll != IntPtr.Zero && buttons.TryGetValue("ok", out b)) {
            Click(ignoreAll);
            Click(b);
            Record("missing media: ignored all missing files, left offline ('" + Short(text) + "')");
            return true;
        }
        if (title == "Hint" && buttons.TryGetValue("ok", out b)) {
            Click(b);
            Record("Hint box: OK ('" + Short(text) + "')");
            return true;
        }
        if (lower.Contains("save") && lower.Contains("change") && buttons.TryGetValue("no", out b)) {
            Click(b);
            Record("save changes? No ('" + Short(text) + "')");
            return true;
        }
        if (progress) return false;
        // Anything else only when it is modal (its owner is disabled), so a
        // floating tool window is never closed, and only once it has stayed.
        IntPtr ownerWindow = GetWindow(h, GW_OWNER);
        if (ownerWindow == IntPtr.Zero || IsWindowEnabled(ownerWindow)) return false;
        DateTime seen;
        if (!firstSeen.TryGetValue(h, out seen)) { firstSeen[h] = now; return false; }
        if (now - seen < UnknownGrace) return false;
        foreach (string word in new string[] { "cancel", "no", "ok", "close" }) {
            if (buttons.TryGetValue(word, out b)) {
                Click(b);
                Record("unknown box '" + title + "': pressed " + word + " ('" + Short(text) + "')");
                return true;
            }
        }
        PostMessage(h, WM_CLOSE, IntPtr.Zero, IntPtr.Zero);
        Record("unknown box '" + title + "': closed it ('" + Short(text) + "')");
        return true;
    }

    static string Short(string s) { return s.Length <= 300 ? s : s.Substring(0, 300) + "..."; }
}

}
