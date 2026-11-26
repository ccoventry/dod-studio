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
    // Each frame takes about 0.2 s; a call holds VEGAS's UI thread meanwhile.
    public const int MaxTimes = 10;
    public const string DefaultFolder = "snapshots";
    // How long a call waits for VEGAS to apply the full preview size, and how
    // often it looks.
    const double SizeWaitSeconds = 3;
    const int SizePollMs = 20;

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
        } catch {
            q.Before.Restore(vegas);
            throw;
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
        q.Width = a.Int("width", DefaultWidth);
        if (q.Width < 0) throw new BridgeError("width is a pixel width, or 0 for the project's full size (got " + q.Width + ")");
        q.Inline = a.Bool("inline", true);
        q.Folder = a.Has("folder") ? Session.RequireUnderRoot(a.Str("folder"), "snapshot's folder")
                                   : Path.Combine(Session.Root, DefaultFolder);
        q.Stem = p.FilePath != null ? Path.GetFileNameWithoutExtension(p.FilePath) : "untitled";
        return q;
    }

    object ShootWhenFullSize(Request q) {
        PreviewVideoProperties pv = q.Project.Preview;
        if (pv.Width >= q.Project.Video.Width) return Shoot(q);
        if (DateTime.Now < q.SizeDeadline) {
            LaterStep later = new LaterStep();
            later.AfterMs = SizePollMs;
            later.Next = delegate {
                try { return ShootWhenFullSize(q); } catch { q.Before.Restore(vegas); throw; }
            };
            return later;
        }
        q.Before.Restore(vegas);
        throw new BridgeError("VEGAS kept the preview at " + pv.Width + "x" + pv.Height + " instead of the project's " +
                              q.Project.Video.Width + "x" + q.Project.Video.Height + " for " + SizeWaitSeconds +
                              " s; no snapshot taken");
    }

    object Shoot(Request q) {
        List<object> shots = new List<object>();
        try {
            Directory.CreateDirectory(q.Folder);
            foreach (double t in q.Times) shots.Add(TakeOne(q, t, FreshPath(q.Folder, q.Stem, t)));
        } finally {
            q.Before.Restore(vegas);
        }
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["project_width"] = q.Project.Video.Width;
        r["project_height"] = q.Project.Video.Height;
        r["project_length"] = q.Length;
        r["snapshots"] = shots;
        return r;
    }

    // `time` or `times`, each inside the project: VEGAS throws
    // ArgumentOutOfRangeException past the end, which says nothing useful.
    static List<double> TimesOf(Args a, double length) {
        List<double> times = new List<double>();
        IEnumerable many = a.Raw("times") as IEnumerable;
        if (many != null && !(many is string)) {
            foreach (object o in many) times.Add(Convert.ToDouble(o, CultureInfo.InvariantCulture));
        } else if (a.Has("time")) {
            times.Add(a.Num("time"));
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

    // A file name in `folder` that does not exist yet, so a snapshot never
    // overwrites one: <project>_<time>s_<when>.png, then _2, _3...
    static string FreshPath(string folder, string stem, double t) {
        string name = stem + "_" + t.ToString("0.000", CultureInfo.InvariantCulture) + "s_" + DateTime.Now.ToString("yyyyMMdd-HHmmss");
        string path = Path.Combine(folder, name + ".png");
        for (int n = 2; File.Exists(path); n++) path = Path.Combine(folder, name + "_" + n + ".png");
        return path;
    }

    object TakeOne(Request q, double t, string path) {
        DateTime t0 = DateTime.Now;
        // VEGAS writes its own PNG beside the final one, which is made from it.
        string raw = path + ".vegas.png";
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
                if (q.Inline) r["inline"] = FrameImage.InlineCopy(frame, png);
            }
            r["seconds"] = (DateTime.Now - t0).TotalSeconds;
            return r;
        } finally {
            if (File.Exists(raw)) File.Delete(raw);
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

        public void Restore(Vegas vegas) {
            try {
                PreviewVideoProperties pv = vegas.Project.Preview;
                using (PreviewBlock(vegas)) {
                    if (pv.RenderQuality != quality) pv.RenderQuality = quality;
                    if (pv.FullSize != fullSize) pv.FullSize = fullSize;
                    if (pv.PreviewSize != size) pv.PreviewSize = size;
                }
            } finally {
                vegas.Transport.CursorPosition = cursor;
            }
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
