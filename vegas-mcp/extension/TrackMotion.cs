// Track Motion: a video track's position, size and rotation keyframes, and
// its 2D shadow and glow, get_track_motion and set_track_motion. Positions
// are project pixels from the frame's centre, x right and y up; width and
// height are project pixels (the whole frame at first); rotation in degrees;
// times are seconds from the project's start.
using System;
using System.Collections;
using System.Collections.Generic;
using ScriptPortal.Vegas;

namespace DodStudio.VegasBridge {

public partial class Commands {

    VideoTrack VideoTrackAt(int index) {
        VideoTrack vt = TrackAt(index) as VideoTrack;
        if (vt == null) throw new BridgeError("track " + index + " is not a video track");
        return vt;
    }

    object GetTrackMotion(Args a) {
        VideoTrack vt = VideoTrackAt(a.Int("track"));
        TrackMotion tm = vt.TrackMotion;
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["track"] = vt.Index;
        r["project_width"] = P.Video.Width;
        r["project_height"] = P.Video.Height;
        r["keyframes"] = MotionKeys(tm.MotionKeyframes);
        Dictionary<string, object> shadow = new Dictionary<string, object>();
        shadow["enabled"] = tm.ShadowEnabled;
        shadow["keyframes"] = MotionKeys(tm.ShadowKeyframes);
        r["shadow"] = shadow;
        Dictionary<string, object> glow = new Dictionary<string, object>();
        glow["enabled"] = tm.GlowEnabled;
        glow["keyframes"] = MotionKeys(tm.GlowKeyframes);
        r["glow"] = glow;
        return r;
    }

    static List<object> MotionKeys(IEnumerable keys) {
        List<object> r = new List<object>();
        foreach (BaseTrackMotionKeyframe k in keys) r.Add(MotionKeyInfo(k));
        return r;
    }

    static object MotionKeyInfo(BaseTrackMotionKeyframe k) {
        Dictionary<string, object> d = new Dictionary<string, object>();
        d["time"] = Sec(k.Position);
        d["x"] = Values.Round(k.PositionX);
        d["y"] = Values.Round(k.PositionY);
        d["width"] = Values.Round(k.Width);
        d["height"] = Values.Round(k.Height);
        d["rotation"] = Values.Degrees(k.RotationZ);
        d["smoothness"] = Values.Round(k.Smoothness);
        d["interpolation"] = k.Type.ToString();
        TrackShadowKeyframe s = k as TrackShadowKeyframe;
        TrackGlowKeyframe g = k as TrackGlowKeyframe;
        if (s != null) { d["blur"] = Values.Round(s.Blur); d["intensity"] = Values.Round(s.Intensity); d["color"] = ColourJson(s.Color); }
        if (g != null) { d["blur"] = Values.Round(g.Blur); d["intensity"] = Values.Round(g.Intensity); d["color"] = ColourJson(g.Color); }
        return d;
    }

    static double[] ColourJson(VideoColor c) {
        return new double[] { Values.Round(c.Red), Values.Round(c.Green), Values.Round(c.Blue), Values.Round(c.Alpha) };
    }

    // keyframes / mode / reset as set_pan_crop: "replace" (default) makes the
    // keyframes exactly these, the one VEGAS keeps at time 0 back at the full
    // frame first (and, given no keyframe at 0, taking the first one's
    // values); "add" adds them, changing any at the same time. A field a
    // keyframe leaves out keeps the value the track has there. shadow and
    // glow take {enabled, keyframes, mode} the same way (their reset keeps
    // VEGAS's own values).
    object SetTrackMotion(Args a) {
        VideoTrack vt = VideoTrackAt(a.Int("track"));
        TrackMotion tm = vt.TrackMotion;
        bool any = false;
        if (a.Has("keyframes") || a.Bool("reset", false)) {
            SetMotionKeys(a, tm.MotionKeyframes, delegate { tm.MotionKeyframes.Clear(); }, delegate(Timecode t) { return tm.InsertMotionKeyframe(t); }, true);
            any = true;
        }
        if (a.Has("shadow")) {
            Args s = new Args(a.Obj("shadow"));
            if (s.Has("enabled")) tm.ShadowEnabled = s.Bool("enabled", false);
            if (s.Has("keyframes") || s.Bool("reset", false))
                SetMotionKeys(s, tm.ShadowKeyframes, delegate { tm.ShadowKeyframes.Clear(); }, delegate(Timecode t) { return tm.InsertShadowKeyframe(t); }, false);
            any = true;
        }
        if (a.Has("glow")) {
            Args g = new Args(a.Obj("glow"));
            if (g.Has("enabled")) tm.GlowEnabled = g.Bool("enabled", false);
            if (g.Has("keyframes") || g.Bool("reset", false))
                SetMotionKeys(g, tm.GlowKeyframes, delegate { tm.GlowKeyframes.Clear(); }, delegate(Timecode t) { return tm.InsertGlowKeyframe(t); }, false);
            any = true;
        }
        if (!any) throw new BridgeError("nothing to set: give keyframes, reset, shadow or glow");
        return GetTrackMotion(a);
    }

    delegate BaseTrackMotionKeyframe InsertKey(Timecode at);
    // Each keyframe list's own Clear, which leaves the one keyframe VEGAS
    // needs; IList.Clear would reach the base list's instead.
    delegate void ClearKeys();

    void SetMotionKeys(Args a, IList keys, ClearKeys clear, InsertKey insert, bool motion) {
        string mode = a.Str("mode", "replace");
        if (mode != "replace" && mode != "add") throw new BridgeError("mode is \"replace\" or \"add\"");
        List<Args> specs = a.Has("keyframes") ? a.Items("keyframes") : new List<Args>();
        Values.CheckTimes(specs, "keyframe", double.PositiveInfinity, "project start");
        Values.SortByTime(specs);
        if (mode == "replace" || a.Bool("reset", false)) {
            // Clear leaves the list with one keyframe, as VEGAS needs one.
            clear();
            BaseTrackMotionKeyframe first = (BaseTrackMotionKeyframe)keys[0];
            first.Position = Tc(0);
            if (motion) {
                first.PositionX = 0;
                first.PositionY = 0;
                first.Width = P.Video.Width;
                first.Height = P.Video.Height;
                first.RotationZ = 0;
                first.Smoothness = 0;
                first.Type = VideoKeyframeType.Linear;
            }
        }
        foreach (Args s in specs) ApplyMotionKey(MotionKeyAt(keys, s.Num("time"), insert), s);
        if (mode == "replace" && specs.Count > 0 && specs[0].Num("time") > Values.SameTime)
            ApplyMotionKey((BaseTrackMotionKeyframe)keys[0], specs[0]);
    }

    static BaseTrackMotionKeyframe MotionKeyAt(IList keys, double t, InsertKey insert) {
        foreach (BaseTrackMotionKeyframe k in keys) if (Math.Abs(Sec(k.Position) - t) < Values.SameTime) return k;
        return insert(Tc(t));
    }

    void ApplyMotionKey(BaseTrackMotionKeyframe k, Args s) {
        if (s.Has("scale")) {
            double scale = s.Num("scale");
            if (scale <= 0) throw new BridgeError("scale must be above 0");
            k.Width = P.Video.Width * scale;
            k.Height = P.Video.Height * scale;
        }
        if (s.Has("x")) k.PositionX = s.Num("x");
        if (s.Has("y")) k.PositionY = s.Num("y");
        if (s.Has("width")) k.Width = s.Num("width");
        if (s.Has("height")) k.Height = s.Num("height");
        if (s.Has("rotation")) k.RotationZ = Values.Radians(s.Num("rotation"));
        if (s.Has("smoothness")) k.Smoothness = s.Num("smoothness");
        if (s.Has("interpolation")) k.Type = Values.Enum<VideoKeyframeType>(s.Str("interpolation"), "interpolation");
        TrackShadowKeyframe sh = k as TrackShadowKeyframe;
        TrackGlowKeyframe gl = k as TrackGlowKeyframe;
        if (sh == null && gl == null) {
            if (s.Has("blur") || s.Has("intensity") || s.Has("color"))
                throw new BridgeError("blur, intensity and color are for shadow and glow keyframes");
            return;
        }
        if (s.Has("blur")) { if (sh != null) sh.Blur = s.Num("blur"); else gl.Blur = s.Num("blur"); }
        if (s.Has("intensity")) { if (sh != null) sh.Intensity = s.Num("intensity"); else gl.Intensity = s.Num("intensity"); }
        if (s.Has("color")) {
            double[] c = Values.Colour(s.Raw("color"), "color");
            VideoColor v = new VideoColor((float)c[0], (float)c[1], (float)c[2], (float)c[3]);
            if (sh != null) sh.Color = v; else gl.Color = v;
        }
    }
}

}
