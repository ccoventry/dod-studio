// The snapshot command: what the project looks like at a timeline time, so
// Claude can check its own edits (a clip in place, an effect's look, a red
// frame) without rendering a clip and pulling frames from it.
//
// Vegas.SaveSnapshot renders the composited frame (every track and FX) at the
// preview's quality and size: at Draft / Quarter a 720p project gives a
// 160x90 PNG. So each call switches the preview to Best at full size, and
// puts it back afterwards. SaveSnapshot also moves the edit cursor to the
// frame it renders, so the cursor goes back too. Measured on build 315
// (#1050), in a minimised VEGAS:
// - The preview settings are project state, but changing them adds no undo
//   step and leaves the project unmodified. An extension still changes the
//   quality only inside an undo block (E_UNEXPECTED outside one).
// - A new quality applies at once; a new preview size only once VEGAS's
//   message loop has run (Application.DoEvents is not enough), so a call that
//   has to change the size takes its snapshots in a later step (LaterStep).
// - The frame is the one a render of that time gives, pixel for pixel (an
//   Image Sequence PNG render of the same frames).
// FrameImage.cs turns VEGAS's PNG into the file and the inline copy.
using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using Gdi = System.Drawing;
using ScriptPortal.Vegas;

namespace DodStudio.VegasBridge {

public class Snapshot {
    // An image sent to a model stays small: 1280 px wide unless asked for more.
    public const int DefaultWidth = 1280;
    // A width past any frame VEGAS makes; it only stops the cast to int
    // overflowing, since a width at or above the frame's keeps its size.
    public const int MaxWidth = 65535;
    // Each frame takes about 0.2 s; a call holds VEGAS's UI thread meanwhile.
    public const int MaxTimes = 10;
    public const string DefaultFolder = "snapshots";
    // How long a call waits for VEGAS to apply the full preview size, and how
    // often it looks.
    const double SizeWaitSeconds = 3;
    const int SizePollMs = 20;
    // Names tried for one frame before giving up: <name>.png, <name>_2.png...
    const int MaxNameTries = 1000;

    readonly Vegas vegas;

    public Snapshot(Vegas vegas) { this.vegas = vegas; }

    // One call: what it was asked for, and the preview settings to put back.
    sealed class Request {
        public Project Project;
        public List<double> Times;
        public int Width;
        public bool Inline;
        public string Folder;
        public string Stem;
        public double Length;
        public PreviewSettings Before;
        public DateTime SizeDeadline;
    }

    // The reply, or a LaterStep when the preview size has yet to apply.
    public object Take(Args a) {
        Request q = Parse(a);
        q.Before = PreviewSettings.Read(vegas);
        try {
            PreviewSettings.ApplyBestFullSize(vegas);
            q.SizeDeadline = DateTime.Now.AddSeconds(SizeWaitSeconds);
            return ShootWhenFullSize(q);
        } catch (Exception e) {
            throw RestoredAfter(q, e);
        }
    }

    Request Parse(Args a) {
        Project p = vegas.Project;
        if (p == null) throw new BridgeError("no project is open");
        if (vegas.Transport.IsPlaying) throw new BridgeError("VEGAS is playing; a snapshot needs playback stopped");
        Request q = new Request();
        q.Project = p;
        q.Length = p.Length.ToMilliseconds() / 1000.0;
        q.Times = TimesOf(a, q.Length);
        q.Width = WidthOf(a);
        q.Inline = a.Bool("inline", true);
        q.Folder = FolderOf(a);
        // An untitled project's FilePath is null or "", depending on how it
        // was made.
        q.Stem = string.IsNullOrEmpty(p.FilePath) ? "untitled" : Path.GetFileNameWithoutExtension(p.FilePath);
        return q;
    }

    static int WidthOf(Args a) {
        double w = a.Num("width", DefaultWidth);
        if (double.IsNaN(w) || w < 0 || w > MaxWidth)
            throw new BridgeError("width is a pixel width up to " + MaxWidth + ", or 0 for the project's full size (got " +
                                  w.ToString(CultureInfo.InvariantCulture) + ")");
        return (int)w;
    }

    // Inside the work folder, or the work folder itself.
    static string FolderOf(Args a) {
        if (!a.Has("folder")) return Path.Combine(Session.Root, DefaultFolder);
        string full = Path.GetFullPath(a.Str("folder")).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
        if (string.Equals(full, Session.Root, StringComparison.OrdinalIgnoreCase)) return Session.Root;
        return Session.RequireUnderRoot(full, "snapshot's folder");
    }

    object ShootWhenFullSize(Request q) {
        PreviewVideoProperties pv = q.Project.Preview;
        if (pv.Width >= q.Project.Video.Width) return Shoot(q);
        if (DateTime.Now < q.SizeDeadline) {
            LaterStep later = new LaterStep();
            later.AfterMs = SizePollMs;
            later.Next = delegate {
                try { return ShootWhenFullSize(q); } catch (Exception e) { throw RestoredAfter(q, e); }
            };
            return later;
        }
        throw new BridgeError("VEGAS kept the preview at " + pv.Width + "x" + pv.Height + " instead of the project's " +
                              q.Project.Video.Width + "x" + q.Project.Video.Height + " for " + SizeWaitSeconds +
                              " s; no snapshot taken");
    }

    // Every frame or none: a failure part-way deletes the PNGs already
    // written, so a refused call leaves no files behind and the error is the
    // whole answer. The preview goes back either way (the caller's catch puts
    // it back after a failure).
    object Shoot(Request q) {
        List<object> shots = new List<object>();
        List<string> written = new List<string>();
        long inlineLeft = FrameImage.InlineCallMaxBytes;
        try {
            Directory.CreateDirectory(q.Folder);
            foreach (double t in q.Times) {
                string path = ReservePath(q.Folder, q.Stem, t);
                written.Add(path);
                Dictionary<string, object> shot = TakeOne(q, t, path, inlineLeft);
                inlineLeft -= InlineBytes(shot);
                shots.Add(shot);
            }
        } catch {
            foreach (string path in written) TryDelete(path);
            throw;
        }
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["project_width"] = q.Project.Video.Width;
        r["project_height"] = q.Project.Video.Height;
        r["project_length"] = q.Length;
        r["snapshots"] = shots;
        // The frames are taken, so a preview that did not go back is reported
        // beside them rather than thrown away with them.
        Exception notBack = q.Before.Restore(vegas);
        if (notBack != null) r["preview_not_put_back"] = Describe(notBack);
        return r;
    }

    // The error to throw for a call that failed after the preview changed:
    // the original one, once the preview is back. When putting it back fails
    // too, a BridgeError saying both, so neither is lost.
    Exception RestoredAfter(Request q, Exception original) {
        if (!(original is BridgeError)) Log.Write("snapshot failed: " + original);
        Exception notBack = q.Before.Restore(vegas);
        if (notBack == null) return original is BridgeError ? original : new BridgeError(Describe(original));
        Log.Write("snapshot: putting the preview back failed after an error: " + notBack);
        return new BridgeError(Describe(original) + "; putting the preview back also failed: " + Describe(notBack));
    }

    // An error as the bridge reports it (BridgeModule.Handle).
    static string Describe(Exception e) {
        if (e is System.Reflection.TargetInvocationException && e.InnerException != null) e = e.InnerException;
        return e is BridgeError ? e.Message : e.GetType().Name + ": " + e.Message;
    }

    static long InlineBytes(Dictionary<string, object> shot) {
        object copy;
        if (!shot.TryGetValue("inline", out copy)) return 0;
        Dictionary<string, object> c = copy as Dictionary<string, object>;
        object bytes;
        return c != null && c.TryGetValue("bytes", out bytes) ? Convert.ToInt64(bytes) : 0;
    }

    static void TryDelete(string path) {
        try { if (File.Exists(path)) File.Delete(path); } catch (Exception e) { Log.Write("snapshot: could not delete " + path + ": " + e.Message); }
    }

    // `time` or `times`, each inside the project: VEGAS throws
    // ArgumentOutOfRangeException past the end, which says nothing useful.
    static List<double> TimesOf(Args a, double length) {
        List<double> times = new List<double>();
        IEnumerable many = a.Raw("times") as IEnumerable;
        if (many != null && !(many is string)) {
            foreach (object o in many) times.Add(TimeOf(o));
        } else if (a.Has("time")) {
            times.Add(TimeOf(a.Raw("time")));
        } else {
            throw new BridgeError("missing argument \"time\" (seconds) or \"times\" (a list of seconds)");
        }
        if (times.Count == 0) throw new BridgeError("times is empty");
        if (times.Count > MaxTimes) throw new BridgeError("at most " + MaxTimes + " times per snapshot call (got " + times.Count + ")");
        foreach (double t in times) {
            if (double.IsNaN(t) || t < 0) throw new BridgeError("time " + t + " is not a timeline time (seconds from 0)");
            if (t >= length)
                throw new BridgeError("time " + t + " s is past the end of the project (it is " + length + " s long" +
                                      (length > 0 ? "" : ": nothing is on the timeline") + ")");
        }
        return times;
    }

    // A number of seconds; Convert.ToDouble would read null as 0 s.
    static double TimeOf(object o) {
        if (o == null || o is bool || o is string) throw new BridgeError("a time is a number of seconds (got " + (o == null ? "null" : "\"" + o + "\"") + ")");
        try {
            return Convert.ToDouble(o, CultureInfo.InvariantCulture);
        } catch (Exception e) {
            if (e is InvalidCastException || e is FormatException || e is OverflowException)
                throw new BridgeError("a time is a number of seconds (got " + o.GetType().Name + ")");
            throw;
        }
    }

    // A file name in `folder` that nobody else has, claimed by creating the
    // file: <project>_<time>s_<when>_<pid>.png, then _2, _3... The PID keeps
    // two bridge VEGAS sharing a work folder apart, and FileMode.CreateNew
    // fails if any process made the file first, so a snapshot never takes
    // another's name or overwrites a file. VEGAS's own PNG is named after
    // this one (RawPathOf), so it is this call's alone too; a name whose raw
    // file is already there (left by a crash) is skipped.
    static string ReservePath(string folder, string stem, double t) {
        string name = stem + "_" + t.ToString("0.000", CultureInfo.InvariantCulture) + "s_" +
                      DateTime.Now.ToString("yyyyMMdd-HHmmss") + "_" + Session.Pid;
        for (int n = 1; n <= MaxNameTries; n++) {
            string path = Path.Combine(folder, (n == 1 ? name : name + "_" + n) + ".png");
            if (File.Exists(path) || File.Exists(RawPathOf(path))) continue;
            try {
                using (new FileStream(path, FileMode.CreateNew, FileAccess.Write)) { }
                return path;
            } catch (IOException) {
                if (!File.Exists(path)) throw;
            }
        }
        throw new BridgeError("no free snapshot name for " + name + " in " + folder + " after " + MaxNameTries + " tries");
    }

    // VEGAS writes its own PNG beside the final one, which is made from it.
    static string RawPathOf(string path) { return path + ".vegas.png"; }

    // `path` is reserved by ReservePath and empty until the frame is written.
    Dictionary<string, object> TakeOne(Request q, double t, string path, long inlineLeft) {
        DateTime t0 = DateTime.Now;
        string raw = RawPathOf(path);
        try {
            RenderStatus st = vegas.SaveSnapshot(raw, ImageFileFormat.PNG, Timecode.FromSeconds(t));
            if (st != RenderStatus.Complete || !File.Exists(raw))
                throw new BridgeError("VEGAS did not save a snapshot at " + t + " s (" + st + ")");
            Dictionary<string, object> r = new Dictionary<string, object>();
            r["time"] = t;
            r["frame"] = (long)Math.Floor(t * q.Project.Video.FrameRate + 1e-6);
            r["path"] = path;
            using (Gdi.Bitmap frame = FrameImage.Opaque(raw, q.Width)) {
                byte[] png = FrameImage.Png(frame);
                File.WriteAllBytes(path, png);
                r["width"] = frame.Width;
                r["height"] = frame.Height;
                r["bytes"] = png.Length;
                if (q.Inline) r["inline"] = FrameImage.InlineCopy(frame, png, inlineLeft);
            }
            r["seconds"] = (DateTime.Now - t0).TotalSeconds;
            return r;
        } finally {
            TryDelete(raw);
        }
    }

    // The preview settings and edit cursor a snapshot changes, to put back.
    sealed class PreviewSettings {
        Timecode cursor;
        VideoRenderQuality quality;
        VideoPreviewSize size;
        bool fullSize;

        public static PreviewSettings Read(Vegas vegas) {
            PreviewVideoProperties pv = vegas.Project.Preview;
            PreviewSettings s = new PreviewSettings();
            s.cursor = vegas.Transport.CursorPosition;
            s.quality = pv.RenderQuality;
            s.size = pv.PreviewSize;
            s.fullSize = pv.FullSize;
            return s;
        }

        // Best quality at full size. Only settings that differ are touched.
        public static void ApplyBestFullSize(Vegas vegas) {
            PreviewVideoProperties pv = vegas.Project.Preview;
            using (PreviewBlock(vegas)) {
                if (pv.RenderQuality != VideoRenderQuality.Best) pv.RenderQuality = VideoRenderQuality.Best;
                // PreviewSize does not leave Full while FullSize is on, so
                // FullSize is set first, both ways.
                if (!pv.FullSize) pv.FullSize = true;
                if (pv.PreviewSize != VideoPreviewSize.Full) pv.PreviewSize = VideoPreviewSize.Full;
            }
        }

        // Puts back each setting on its own, so one that fails does not stop
        // the rest; returns the first failure, or null.
        public Exception Restore(Vegas vegas) {
            Exception first = null;
            try {
                PreviewVideoProperties pv = vegas.Project.Preview;
                using (PreviewBlock(vegas)) {
                    Attempt(ref first, delegate { if (pv.RenderQuality != quality) pv.RenderQuality = quality; });
                    Attempt(ref first, delegate { if (pv.FullSize != fullSize) pv.FullSize = fullSize; });
                    Attempt(ref first, delegate { if (pv.PreviewSize != size) pv.PreviewSize = size; });
                }
            } catch (Exception e) {
                if (first == null) first = e;
            }
            Attempt(ref first, delegate { vegas.Transport.CursorPosition = cursor; });
            return first;
        }

        static void Attempt(ref Exception first, Action step) {
            try { step(); } catch (Exception e) { if (first == null) first = e; }
        }

        // The undo block an extension needs to change the preview quality,
        // cancelled when disposed: the change stays (it is not an undoable
        // edit), and nothing is added to the undo history.
        static IDisposable PreviewBlock(Vegas vegas) {
            UndoBlock block = new UndoBlock(vegas.Project, "DoD Studio bridge: snapshot preview");
            block.Cancel = true;
            return block;
        }
    }
}

}
