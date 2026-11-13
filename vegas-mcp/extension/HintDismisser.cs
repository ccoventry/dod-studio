// While the bridge renders, VEGAS can pop a "Hint" box (an OK-only notice)
// that holds the render until someone clicks OK; with nobody at the PC the
// call never returns. This presses OK on Hint boxes of this VEGAS process
// only, for as long as it is running, and counts them so the reply says so.
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

namespace DodStudio.VegasBridge {

public sealed class HintDismisser : IDisposable {
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    const uint BM_CLICK = 0x00F5;

    readonly uint pid = (uint)System.Diagnostics.Process.GetCurrentProcess().Id;
    readonly Thread thread;
    volatile bool stop;
    public int Dismissed;

    public HintDismisser() {
        thread = new Thread(Watch);
        thread.IsBackground = true;
        thread.Start();
    }

    static string Text(IntPtr h) { StringBuilder s = new StringBuilder(256); GetWindowText(h, s, 256); return s.ToString(); }
    static string Cls(IntPtr h) { StringBuilder s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }

    void Watch() {
        while (!stop) {
            Thread.Sleep(500);
            EnumWindows(delegate(IntPtr h, IntPtr l) {
                uint owner;
                GetWindowThreadProcessId(h, out owner);
                if (owner != pid || !IsWindowVisible(h) || Cls(h) != "#32770" || Text(h) != "Hint") return true;
                EnumChildWindows(h, delegate(IntPtr c, IntPtr l2) {
                    string t = Text(c);
                    if (Cls(c) == "Button" && (t == "DialogButtonOK" || t == "OK")) {
                        PostMessage(c, BM_CLICK, IntPtr.Zero, IntPtr.Zero);
                        Dismissed++;
                        Log.Write("pressed OK on a VEGAS Hint box during render");
                        return false;
                    }
                    return true;
                }, IntPtr.Zero);
                return true;
            }, IntPtr.Zero);
        }
    }

    public void Dispose() { stop = true; }
}

}
