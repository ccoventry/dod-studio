// Pan/Crop: a video event's VideoMotion keyframes, get_pan_crop and
// set_pan_crop. Each keyframe is the part of the source picture that fills
// the frame: its four corners (bounds) in the source media's own pixels,
// y down from the top-left, plus a centre, a rotation and how it moves to
// the next keyframe. Times are seconds from the event's start.
//
// "zoom" is how much larger the picture shows than at the event's default
// frame (the whole media, widened or heightened to the project's aspect as
// VEGAS does): zoom 2 shows a box half as wide and high.
using System;
using System.Collections.Generic;
using ScriptPortal.Vegas;

namespace DodStudio.VegasBridge {

public partial class Commands {

    // The box VEGAS shows a new event's media in: the whole media, padded to
    // the project's aspect so it is not stretched.
    struct PanFrame {
        public double Width, Height, CenterX, CenterY;
    }

    PanFrame DefaultPanFrame(VideoEvent ve) {
        if (ve.ActiveTake == null) throw new BridgeError("event " + ve.Index + " has no take, so no Pan/Crop frame");
        VideoStream vs = ve.ActiveTake.MediaStream as VideoStream;
        if (vs == null) throw new BridgeError("event " + ve.Index + "'s take has no video stream");
        double w = vs.Width, h = vs.Height;
        double project = (double)P.Video.Width / P.Video.Height * P.Video.PixelAspectRatio;
        double media = w / h * vs.PixelAspectRatio;
        PanFrame f = new PanFrame();
        f.CenterX = w / 2;
        f.CenterY = h / 2;
        if (media > project) { f.Width = w; f.Height = w * vs.PixelAspectRatio / project; }
        else { f.Height = h; f.Width = h * project / vs.PixelAspectRatio; }
        return f;
    }

    object GetPanCrop(Args a) {
        VideoEvent ve = VideoEventAt(a);
        PanFrame f = DefaultPanFrame(ve);
        VideoStream vs = (VideoStream)ve.ActiveTake.MediaStream;
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["track"] = ve.Track.Index;
        r["event"] = ve.Index;
        r["event_length"] = Sec(ve.Length);
        r["media_width"] = vs.Width;
        r["media_height"] = vs.Height;
        r["default_frame"] = new double[] { Values.Round(f.Width), Values.Round(f.Height) };
        r["maintain_aspect"] = ve.MaintainAspectRatio;
        List<object> keys = new List<object>();
        foreach (VideoMotionKeyframe k in ve.VideoMotion.Keyframes) keys.Add(PanKeyInfo(k, f));
        r["keyframes"] = keys;
        return r;
    }

    static object PanKeyInfo(VideoMotionKeyframe k, PanFrame f) {
        VideoMotionBounds b = k.Bounds;
        double w = Distance(b.TopLeft, b.TopRight), h = Distance(b.TopLeft, b.BottomLeft);
        Dictionary<string, object> d = new Dictionary<string, object>();
        d["time"] = Sec(k.Position);
        d["center"] = Values.Point(k.Center.X, k.Center.Y);
        d["bounds"] = new object[] {
            Values.Point(b.TopLeft.X, b.TopLeft.Y), Values.Point(b.TopRight.X, b.TopRight.Y),
            Values.Point(b.BottomRight.X, b.BottomRight.Y), Values.Point(b.BottomLeft.X, b.BottomLeft.Y) };
        d["width"] = Values.Round(w);
        d["height"] = Values.Round(h);
        d["zoom"] = w > 0 ? Values.Round(f.Width / w) : 0;
        d["rotation"] = Values.Degrees(k.Rotation);
        d["smoothness"] = Values.Round(k.Smoothness);
        d["interpolation"] = k.Type.ToString();
        return d;
    }

    static double Distance(VideoMotionVertex a, VideoMotionVertex b) {
        double dx = b.X - a.X, dy = b.Y - a.Y;
        return Math.Sqrt(dx * dx + dy * dy);
    }

    // mode "replace" (default): the keyframes become exactly these (VEGAS
    // keeps one at time 0: given none there, it takes the first keyframe's
    // frame). "add": added, replacing any at the same time. reset: back to the
    // default frame, one keyframe. `zoom` and `pan` add the keyframes of a
    // whole move to `keyframes`.
    object SetPanCrop(Args a) {
        VideoEvent ve = VideoEventAt(a);
        PanFrame f = DefaultPanFrame(ve);
        string mode = a.Str("mode", "replace");
        if (mode != "replace" && mode != "add") throw new BridgeError("mode is \"replace\" or \"add\"");
        List<Args> specs = a.Has("keyframes") ? a.Items("keyframes") : new List<Args>();
        if (a.Has("zoom")) specs.AddRange(ZoomMove(new Args(a.Obj("zoom")), ve));
        if (a.Has("pan")) specs.AddRange(PanMove(new Args(a.Obj("pan")), ve));
        bool reset = a.Bool("reset", false);
        if (specs.Count == 0 && !reset) throw new BridgeError("nothing to set: give keyframes, zoom, pan or reset");
        specs.Sort(delegate(Args x, Args y) { return x.Num("time").CompareTo(y.Num("time")); });
        VideoMotionKeyframes keys = ve.VideoMotion.Keyframes;
        if (reset || mode == "replace") {
            keys.Clear();
            VideoMotionKeyframe first = keys[0];
            first.Position = Tc(0);
            SetPanBox(first, f, f.CenterX, f.CenterY, 1.0);
            first.Rotation = 0;
            first.Smoothness = 0;
            first.Type = VideoKeyframeType.Linear;
        }
        foreach (Args s in specs) {
            double t = s.Num("time");
            if (t < 0) throw new BridgeError("keyframe time " + t + " is before the event's start");
            ApplyPanKey(PanKeyAt(ve, t), s, f);
        }
        if (mode == "replace" && specs.Count > 0 && specs[0].Num("time") > Values.SameTime) ApplyPanKey(keys[0], specs[0], f);
        return GetPanCrop(a);
    }

    VideoMotionKeyframe PanKeyAt(VideoEvent ve, double t) {
        foreach (VideoMotionKeyframe k in ve.VideoMotion.Keyframes)
            if (Math.Abs(Sec(k.Position) - t) < Values.SameTime) return k;
        VideoMotionKeyframe made = new VideoMotionKeyframe(P, Tc(t));
        ve.VideoMotion.Keyframes.Add(made);
        return made;
    }

    // A keyframe's frame from one of: bounds (four corners), rect (x, y,
    // width, height), or zoom and/or center. What it does not give stays.
    void ApplyPanKey(VideoMotionKeyframe k, Args s, PanFrame f) {
        if (s.Has("bounds")) {
            System.Collections.IList c = s.List("bounds");
            if (c.Count != 4) throw new BridgeError("bounds is four [x, y] corners: top-left, top-right, bottom-right, bottom-left");
            VideoMotionVertex[] v = new VideoMotionVertex[4];
            for (int i = 0; i < 4; i++) {
                double[] p = Values.Numbers(c[i], 2, "a bounds corner");
                v[i] = new VideoMotionVertex((float)p[0], (float)p[1]);
            }
            k.Bounds = new VideoMotionBounds(v[0], v[1], v[2], v[3]);
            k.Center = new VideoMotionVertex((v[0].X + v[2].X) / 2, (v[0].Y + v[2].Y) / 2);
        } else if (s.Has("rect")) {
            Args r = new Args(s.Obj("rect"));
            double w = r.Num("width"), h = r.Num("height");
            if (w <= 0 || h <= 0) throw new BridgeError("rect's width and height must be above 0");
            SetPanBoxSize(k, r.Num("x") + w / 2, r.Num("y") + h / 2, w, h);
        } else if (s.Has("zoom") || s.Has("center")) {
            double zoom = s.Num("zoom", 1.0);
            if (zoom <= 0) throw new BridgeError("zoom must be above 0");
            double cx = f.CenterX, cy = f.CenterY;
            if (s.Has("center")) { double[] c = Values.Numbers(s.Raw("center"), 2, "center"); cx = c[0]; cy = c[1]; }
            SetPanBox(k, f, cx, cy, zoom);
        }
        if (s.Has("rotation")) k.Rotation = Values.Radians(s.Num("rotation"));
        if (s.Has("smoothness")) k.Smoothness = (float)s.Num("smoothness");
        if (s.Has("interpolation")) k.Type = Values.Enum<VideoKeyframeType>(s.Str("interpolation"), "interpolation");
    }

    static void SetPanBox(VideoMotionKeyframe k, PanFrame f, double cx, double cy, double zoom) {
        SetPanBoxSize(k, cx, cy, f.Width / zoom, f.Height / zoom);
    }

    static void SetPanBoxSize(VideoMotionKeyframe k, double cx, double cy, double w, double h) {
        float x0 = (float)(cx - w / 2), y0 = (float)(cy - h / 2), x1 = (float)(cx + w / 2), y1 = (float)(cy + h / 2);
        k.Bounds = new VideoMotionBounds(new VideoMotionVertex(x0, y0), new VideoMotionVertex(x1, y0),
                                         new VideoMotionVertex(x1, y1), new VideoMotionVertex(x0, y1));
        k.Center = new VideoMotionVertex((float)cx, (float)cy);
    }

    // zoom: {scale, center, start, end, from_scale, from_center, interpolation}.
    // Without `end` the event holds `scale` at `center` from `start`; with it,
    // the picture moves from from_scale at from_center (default 1, the middle)
    // at `start` to `scale` at `center` at `end`.
    static List<Args> ZoomMove(Args z, VideoEvent ve) {
        List<Args> r = new List<Args>();
        double start = z.Num("start", 0.0);
        Dictionary<string, object> to = new Dictionary<string, object>();
        to["zoom"] = z.Num("scale");
        if (z.Has("center")) to["center"] = z.Raw("center");
        if (!z.Has("end")) {
            to["time"] = start;
            if (z.Has("interpolation")) to["interpolation"] = z.Str("interpolation");
            r.Add(new Args(to));
            return r;
        }
        double end = z.Num("end");
        if (end <= start) throw new BridgeError("zoom's end must be after its start");
        Dictionary<string, object> from = new Dictionary<string, object>();
        from["time"] = start;
        from["zoom"] = z.Num("from_scale", 1.0);
        if (z.Has("from_center")) from["center"] = z.Raw("from_center");
        if (z.Has("interpolation")) from["interpolation"] = z.Str("interpolation");
        to["time"] = end;
        r.Add(new Args(from));
        r.Add(new Args(to));
        return r;
    }

    // pan: {from, to, start, end, scale, interpolation}: the centre moves from
    // `from` at `start` (default 0) to `to` at `end` (default the event's end)
    // at one zoom (`scale`, default 1).
    static List<Args> PanMove(Args p, VideoEvent ve) {
        double start = p.Num("start", 0.0), end = p.Num("end", Sec(ve.Length));
        if (end <= start) throw new BridgeError("pan's end must be after its start");
        List<Args> r = new List<Args>();
        foreach (string side in new string[] { "from", "to" }) {
            Dictionary<string, object> k = new Dictionary<string, object>();
            k["time"] = side == "from" ? start : end;
            k["center"] = p.Raw(side) ?? Missing(side, "pan");
            k["zoom"] = p.Num("scale", 1.0);
            if (side == "from" && p.Has("interpolation")) k["interpolation"] = p.Str("interpolation");
            r.Add(new Args(k));
        }
        return r;
    }

    static object Missing(string name, string what) {
        throw new BridgeError(what + " needs \"" + name + "\"");
    }
}

}
