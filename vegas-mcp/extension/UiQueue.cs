// Runs bridge commands on VEGAS's UI thread, handed over from the pipe
// thread. A WinForms timer created on the UI thread picks each one up from
// its own WM_TIMER, not through Control.Invoke: VEGAS's OpenProject never
// returns after its missing-media box is answered when it runs inside an
// Invoke callback (run_script too), but does from a script started with
// -SCRIPT (measured on build 315, #1050).
using System;
using System.Collections.Generic;
using System.Threading;
using System.Windows.Forms;

namespace DodStudio.VegasBridge {

public sealed class UiQueue {
    sealed class Item {
        public MethodInvoker Work;
        public readonly ManualResetEvent Done = new ManualResetEvent(false);
    }

    readonly Queue<Item> items = new Queue<Item>();
    readonly System.Windows.Forms.Timer timer = new System.Windows.Forms.Timer();
    // A command pumps messages while VEGAS works (opening a project, a
    // render), so the timer ticks again inside it; one runs at a time.
    bool running;

    // Call on the UI thread.
    public UiQueue() {
        timer.Interval = 15;
        timer.Tick += delegate { RunNext(); };
        timer.Start();
    }

    // Call from any other thread: waits until the work has run on the UI thread.
    public void Run(MethodInvoker work) {
        Item item = new Item();
        item.Work = work;
        lock (items) items.Enqueue(item);
        item.Done.WaitOne();
    }

    void RunNext() {
        if (running) return;
        Item item;
        lock (items) {
            if (items.Count == 0) return;
            item = items.Dequeue();
        }
        running = true;
        try { item.Work(); } finally { running = false; item.Done.Set(); }
    }
}

}
