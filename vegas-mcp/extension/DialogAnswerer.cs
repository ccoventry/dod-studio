// Answers the dialogs this VEGAS raises, so none waits on the user's screen
// and no bridge command hangs on one. Runs for the whole session in a VEGAS
// started for the bridge, and looks only at windows of this VEGAS process:
// the user's own VEGAS beside it is never touched.
//
//   Missing media ("... could not be found"): "Ignore all missing files and
//     leave them offline", then OK.
//   The first-render "Hint" box (OK only): OK.
//   "Save changes?": No. The bridge never saves in place.
//   Any other modal box still open after a few seconds: Cancel, else No,
//     else OK, else Close. Its text goes to the log and to the reply of the
//     command that raised it, so the next run can answer it properly.
//
// Left alone: a dialog with a progress bar (a render's: closing it would
// cancel the work it shows), and VEGAS's offer to restore an autosaved
// project, which may be the user's crashed session (launch refuses to start
// a bridge VEGAS while such an autosave exists, so it should never appear).
// Only a modal dialog that is active counts, so neither the lower box of two
// nested ones, a hidden box VEGAS keeps for reuse, nor a modeless box is
// pressed:
//   a Windows dialog box (#32770) that is enabled, with its owner disabled
//     and the box the owner's enabled popup (a modal box);
//   one with no owner, enabled and visible, while every other shown window
//     of its thread is disabled (a modal loop's doing; a hidden one waits on
//     nobody, and may be kept for reuse);
//   a WinForms form, enabled, inside its ShowDialog. One of VEGAS's own
//     (this AppDomain) says so itself (Form.Modal). A script's runs in
//     VEGAS's "Script Domain", where Control.FromHandle cannot reach
//     (measured), so it counts when every other shown window of its thread
//     is disabled, as ShowDialog leaves them.
// WPF windows are left alone: VEGAS 21's render progress window is one
// (unowned, and visible even in a minimised VEGAS), and nothing outside a
// WPF window tells a modal box from it. A call held up by one runs into
// its time limit on the MCP side, and `end` ends that VEGAS.
// A box that ignores its button is given up on after a few presses. English
// VEGAS only: on another language the rules miss and the fallback answers.
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Windows.Forms;

namespace DodStudio.VegasBridge {

public sealed class DialogAnswerer {
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumThreadWindows(uint thread, EnumProc f, IntPtr l);
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
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
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
    const uint GW_ENABLEDPOPUP = 6;
    const uint SMTO_ABORTIFHUNG = 0x0002;
    const uint SEND_TIMEOUT_MS = 2000;
    const string DialogClass = "#32770";
    const string FormClassPrefix = "WindowsForms10.Window";
    const string FormButtonClassPrefix = "WindowsForms10.BUTTON";
    // How long an unknown modal box stays before it is closed: long enough
    // that a box VEGAS closes on its own is not touched.
    static readonly TimeSpan UnknownGrace = TimeSpan.FromSeconds(4);
    // A box just answered takes a moment to close; it is not pressed again sooner.
    static readonly TimeSpan AnswerHoldOff = TimeSpan.FromSeconds(3);
    const int ScanIntervalMs = 300;
    // Presses before a box that ignores them is given up on and reported.
    const int MaxPresses = 3;

    readonly uint pid = (uint)Session.Pid;
    readonly object gate = new object();
    readonly List<string> answered = new List<string>();
    // When each still-open unknown box was first seen, boxes already answered
    // and how often, and boxes left alone (reported once). Clock-change safe:
    // times come from a Stopwatch, not the wall clock.
    readonly Dictionary<IntPtr, TimeSpan> firstSeen = new Dictionary<IntPtr, TimeSpan>();
    readonly Dictionary<IntPtr, TimeSpan> done = new Dictionary<IntPtr, TimeSpan>();
    readonly Dictionary<IntPtr, int> presses = new Dictionary<IntPtr, int>();
    readonly HashSet<IntPtr> leftAlone = new HashSet<IntPtr>();
    readonly System.Diagnostics.Stopwatch clock = System.Diagnostics.Stopwatch.StartNew();

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
    // button ignores BM_CLICK in a dialog that is not active (Windows says
    // so; measured on the missing-media box of a minimised VEGAS, whose
    // dialogs are hidden), and a bridge VEGAS's never is, so the dialog gets
    // the WM_COMMAND the button would send. A WinForms button always does:
    // it clicks on a mouse-up only with the mouse over it, so BM_CLICK
    // misses in a hidden form, but its form hands a WM_COMMAND back to it
    // as a click, from any AppDomain.
    static void Click(IntPtr button) {
        IntPtr unused;
        int kind = GetWindowLong(button, GWL_STYLE) & BS_TYPEMASK;
        bool push = kind == BS_PUSHBUTTON || kind == BS_DEFPUSHBUTTON;
        bool winForms = Cls(button).StartsWith(FormButtonClassPrefix, StringComparison.Ordinal);
        IntPtr dialog = GetParent(button);
        if (push || winForms) {
            if (dialog != IntPtr.Zero && (winForms || dialog != GetForegroundWindow())) {
                IntPtr wParam = (IntPtr)((GetDlgCtrlID(button) & 0xFFFF) | (BN_CLICKED << 16));
                SendMessageTimeout(dialog, WM_COMMAND, wParam, button, SMTO_ABORTIFHUNG, SEND_TIMEOUT_MS, out unused);
                // EndDialog only marks the box done; its message loop leaves
                // on the next posted message, and a hidden box in a minimised
                // VEGAS gets none (a sent message does not count), so the
                // command that opened it would wait forever. Post one.
                PostMessage(dialog, WM_NULL, IntPtr.Zero, IntPtr.Zero);
                return;
            }
        }
        SendMessageTimeout(button, BM_CLICK, IntPtr.Zero, IntPtr.Zero, SMTO_ABORTIFHUNG, SEND_TIMEOUT_MS, out unused);
    }

    // The box a user would be looking at: enabled, its owner disabled (it is
    // modal), and the owner's enabled popup. Hidden or not: a minimised
    // VEGAS's dialogs are hidden, since Windows hides what a minimised window owns.
    static bool IsActiveModal(IntPtr h) {
        if (!IsWindowEnabled(h)) return false;
        IntPtr owner = GetWindow(h, GW_OWNER);
        if (owner == IntPtr.Zero || IsWindowEnabled(owner)) return false;
        return GetWindow(owner, GW_ENABLEDPOPUP) == h;
    }

    // A box with no owner that is modal all the same: enabled and visible,
    // with every other shown window of its thread disabled. A modal loop does
    // that (WinForms' MessageBox.Show and ShowDialog disable every enabled
    // top-level window of the thread, MB_TASKMODAL too), and a box alone on a
    // thread of its own has nothing to disable. A modeless unowned box leaves
    // VEGAS's main window enabled, so it is left alone, and so is a bare
    // MessageBox(NULL, ...) on VEGAS's UI thread, which disables nothing: a
    // call waiting on one runs into its time limit, and `end` ends that VEGAS.
    static bool IsUnownedModalBox(IntPtr h) {
        return GetWindow(h, GW_OWNER) == IntPtr.Zero && IsWindowEnabled(h) && IsWindowVisible(h) && OnlyEnabledOfThread(h);
    }

    // A WinForms form inside its ShowDialog, owned or not, and not under a
    // box of its own (that disables it). One of VEGAS's own (this AppDomain)
    // says so itself: Form.Modal, a plain flag, safe to read from this
    // thread; any other control of this AppDomain is not a dialog. A script's
    // form (Control.FromHandle cannot reach its AppDomain) counts when every
    // other shown window of its thread is disabled, as ShowDialog leaves them.
    static bool IsModalForm(IntPtr h) {
        if (!IsWindowEnabled(h)) return false;
        Control mine = Control.FromHandle(h);
        if (mine != null) {
            Form form = mine as Form;
            return form != null && form.Modal;
        }
        return IsWindowVisible(h) && OnlyEnabledOfThread(h);
    }

    // Whether h is the only enabled, visible top-level window of its thread.
    static bool OnlyEnabledOfThread(IntPtr h) {
        uint unused;
        uint thread = GetWindowThreadProcessId(h, out unused);
        bool alone = true;
        EnumThreadWindows(thread, delegate(IntPtr w, IntPtr l) {
            if (w != h && IsWindowVisible(w) && IsWindowEnabled(w)) { alone = false; return false; }
            return true;
        }, IntPtr.Zero);
        return alone;
    }

    static bool IsAnswerable(IntPtr h) {
        string cls = Cls(h);
        if (cls == DialogClass) return IsActiveModal(h) || IsUnownedModalBox(h);
        if (cls.StartsWith(FormClassPrefix, StringComparison.Ordinal)) return IsModalForm(h);
        return false;
    }

    static bool IsButton(string cls) {
        return cls == "Button" || cls.StartsWith(FormButtonClassPrefix, StringComparison.Ordinal);
    }

    void Record(string what) {
        lock (gate) answered.Add(what);
        Log.Write("dialog: " + what);
    }

    void Watch() {
        while (true) {
            try { Scan(); } catch (Exception e) { Log.Write("dialog watcher: " + e.Message); }
            Thread.Sleep(ScanIntervalMs);
        }
    }

    void Scan() {
        TimeSpan now = clock.Elapsed;
        HashSet<IntPtr> open = new HashSet<IntPtr>();
        EnumWindows(delegate(IntPtr h, IntPtr l) {
            uint owner;
            GetWindowThreadProcessId(h, out owner);
            if (owner != pid || !IsAnswerable(h)) return true;
            open.Add(h);
            if (leftAlone.Contains(h)) return true;
            TimeSpan when;
            if (done.TryGetValue(h, out when) && now - when < AnswerHoldOff) return true;
            int count;
            presses.TryGetValue(h, out count);
            if (count >= MaxPresses) {
                leftAlone.Add(h);
                Record("box '" + Text(h) + "' stayed open after " + count + " presses: left it");
                return true;
            }
            if (Answer(h, now)) { done[h] = now; presses[h] = count + 1; }
            return true;
        }, IntPtr.Zero);
        // Forget windows that have closed (handles are reused).
        foreach (IntPtr h in new List<IntPtr>(firstSeen.Keys)) if (!open.Contains(h)) firstSeen.Remove(h);
        foreach (IntPtr h in new List<IntPtr>(done.Keys)) if (!open.Contains(h)) done.Remove(h);
        foreach (IntPtr h in new List<IntPtr>(presses.Keys)) if (!open.Contains(h)) presses.Remove(h);
        leftAlone.IntersectWith(open);
    }

    // True when it pressed something.
    bool Answer(IntPtr h, TimeSpan now) {
        string title = Text(h);
        StringBuilder body = new StringBuilder();
        Dictionary<string, IntPtr> buttons = new Dictionary<string, IntPtr>();
        IntPtr ignoreAll = IntPtr.Zero;
        bool progress = false;
        EnumChildWindows(h, delegate(IntPtr c, IntPtr l) {
            string cls = Cls(c);
            string t = Text(c);
            if (cls.IndexOf("progress", StringComparison.OrdinalIgnoreCase) >= 0) progress = true;
            if (IsButton(cls)) {
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

        // Only the prompt's own sentences: "restore" alone also matches a
        // plug-in's "default settings have been restored" box, a path, or
        // restored.veg, which then would hang unanswered.
        if (lower.Contains("restore the autosaved project") || lower.Contains("did not complete properly")) {
            leftAlone.Add(h);
            Record("left VEGAS's restore prompt for the user ('" + Short(text) + "')");
            return false;
        }
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
        // Anything else only once it has stayed. Scan passes only a modal box
        // (IsAnswerable), so a modeless box, form or tool window is never
        // pressed or closed.
        TimeSpan seen;
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
