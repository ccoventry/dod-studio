// compare_projects' half in VEGAS: every detail of two projects, the open
// one and/or .veg files, for the MCP server to compare (mcp/project_compare.py).
// A file is never opened itself: it is copied into <work folder>\copies and
// the copy opened, read and dropped. The open project is reopened from its
// file afterwards, so it must have no unsaved changes; an untitled one with
// none is put back as a new empty project of the same size and rate.
//
// The detail is project_info's `detail` with the counts replaced by what
// they count: every FX parameter keyframe, Pan/Crop keyframe, Track Motion
// keyframe and envelope point, and each fade's transition with its values.
using System;
using System.Collections.Generic;
using System.IO;
using ScriptPortal.Vegas;

namespace DodStudio.VegasBridge {

public partial class Commands {

    const string OpenProjectSide = "open";

    object CompareProjects(Args a) {
        string[] sides = { a.Str("a"), a.Str("b") };
        if (sides[0] == OpenProjectSide && sides[1] == OpenProjectSide) throw new BridgeError("a and b are both the open project");
        Project open = P;
        if (open.IsModified)
            throw new BridgeError("the open project (" + (open.FilePath ?? "untitled") + ") has unsaved changes, and compare_projects " +
                                  "reopens it afterwards: save_as it first");
        string openPath = open.FilePath;
        if (openPath != null && !Session.UnderRoot(openPath))
            throw new BridgeError("the open project " + openPath + " is outside the work folder, so it cannot be reopened in place afterwards");
        if (openPath == null && open.Tracks.Count > 0)
            throw new BridgeError("the open project is untitled: save_as it first, so it can be reopened afterwards");
        VideoSettings settings = new VideoSettings(open);
        // Checked before anything is opened, so a bad path changes nothing.
        foreach (string s in sides) {
            if (s == OpenProjectSide) continue;
            string full = Path.GetFullPath(s);
            if (!File.Exists(full)) throw new BridgeError("no project at " + full);
            if (!full.EndsWith(".veg", StringComparison.OrdinalIgnoreCase)) throw new BridgeError("compare_projects compares .veg files, got " + full);
        }
        Dictionary<string, object> r = new Dictionary<string, object>();
        object[] dumps = new object[2];
        string[] read = new string[2];
        // The open project first, before any file replaces it.
        for (int i = 0; i < 2; i++) {
            if (sides[i] != OpenProjectSide) continue;
            dumps[i] = Read(delegate { return ProjectDump(); });
            read[i] = openPath ?? "the open untitled project";
        }
        bool replaced = false;
        try {
            for (int i = 0; i < 2; i++) {
                if (sides[i] == OpenProjectSide) continue;
                string source = Path.GetFullPath(sides[i]);
                string copy = Session.FreshCopyPath(source);
                File.Copy(source, copy);
                Log.Write("compare_projects: copied " + source + " to " + copy);
                replaced = true;
                vegas.NewProject(false, false);
                if (!vegas.OpenProject(copy)) throw new BridgeError("VEGAS could not open " + copy + " (a copy of " + source + ")");
                dumps[i] = Read(delegate { return ProjectDump(); });
                read[i] = copy;
            }
        } finally {
            if (replaced) r["restored"] = Restore(openPath, settings);
        }
        r["a"] = dumps[0];
        r["b"] = dumps[1];
        r["read_a"] = read[0];
        r["read_b"] = read[1];
        return r;
    }

    // The size and rate of an untitled project, to put an empty one back.
    class VideoSettings {
        public readonly int Width, Height;
        public readonly double Fps;
        public readonly PixelFormat Format;

        public VideoSettings(Project p) {
            Width = p.Video.Width;
            Height = p.Video.Height;
            Fps = p.Video.FrameRate;
            Format = p.Video.PixelFormat;
        }
    }

    string Restore(string openPath, VideoSettings s) {
        vegas.NewProject(false, false);
        if (openPath != null) {
            if (!vegas.OpenProject(openPath)) throw new BridgeError("VEGAS could not reopen " + openPath + " after the compare");
            return "reopened " + openPath;
        }
        Project p = vegas.Project;
        if (p.Video.Width == s.Width && p.Video.Height == s.Height && Math.Abs(p.Video.FrameRate - s.Fps) < 1e-9 && p.Video.PixelFormat == s.Format)
            return "a new empty untitled project, as before";
        using (new UndoBlock(p, "DoD Studio bridge: project settings")) {
            p.Video.Width = s.Width;
            p.Video.Height = s.Height;
            p.Video.FrameRate = s.Fps;
            p.Video.PixelFormat = s.Format;
        }
        return "a new empty untitled project of the same size and rate (it now shows as modified)";
    }

    // project_info with detail, and every keyframe and point behind its counts.
    object ProjectDump() {
        Dictionary<string, object> d = (Dictionary<string, object>)ProjectInfo(true);
        List<object> tracks = (List<object>)d["tracks"];
        foreach (Track t in P.Tracks) {
            Dictionary<string, object> tr = (Dictionary<string, object>)tracks[t.Index];
            tr["envelopes"] = EnvelopeList(t.Envelopes);
            VideoTrack vt = t as VideoTrack;
            if (vt != null) {
                tr["effects"] = EffectsDump(vt.Effects);
                Dictionary<string, object> motion = (Dictionary<string, object>)GetTrackMotion(TrackArgs(t.Index));
                motion.Remove("track");
                motion.Remove("project_width");
                motion.Remove("project_height");
                tr["track_motion"] = motion;
                tr.Remove("track_motion_keyframes");
            }
            List<object> evs = (List<object>)tr["events"];
            foreach (TrackEvent ev in t.Events) EventDump(ev, (Dictionary<string, object>)evs[ev.Index]);
        }
        d["output_effects"] = EffectsDump(P.VideoBus.Effects);
        foreach (object o in (List<object>)d["media"]) {
            Dictionary<string, object> mi = (Dictionary<string, object>)o;
            Media m = FindMedia((string)mi["key"]);
            if (m != null) mi["effects"] = EffectsDump(m.Effects);
        }
        return d;
    }

    void EventDump(TrackEvent ev, Dictionary<string, object> e) {
        e["fade_in"] = FadeInfo(ev.FadeIn, ev.IsVideo(), true);
        e["fade_out"] = FadeInfo(ev.FadeOut, ev.IsVideo(), true);
        VideoEvent ve = ev as VideoEvent;
        if (ve == null) return;
        e["effects"] = EffectsDump(ve.Effects);
        e["envelopes"] = EnvelopeList(ve.Envelopes);
        e.Remove("pan_crop_keyframes");
        if (ve.ActiveTake != null && ve.ActiveTake.MediaStream is VideoStream) {
            Dictionary<string, object> d = new Dictionary<string, object>();
            d["track"] = ev.Track.Index;
            d["event"] = ev.Index;
            e["pan_crop"] = ((Dictionary<string, object>)GetPanCrop(new Args(d)))["keyframes"];
        }
    }

    static Args TrackArgs(int track) {
        Dictionary<string, object> d = new Dictionary<string, object>();
        d["track"] = track;
        return new Args(d);
    }

    static List<object> EnvelopeList(Envelopes envs) {
        List<object> r = new List<object>();
        foreach (Envelope e in envs) r.Add(EnvelopeInfo(e));
        return r;
    }

    // Each effect as project_info lists it, plus the keyframes of each
    // animated parameter, by parameter name.
    static List<object> EffectsDump(Effects chain) {
        List<object> r = new List<object>();
        foreach (Effect e in chain) {
            Dictionary<string, object> d = (Dictionary<string, object>)EffectInfo(e);
            if (e.IsOFX && e.OFXEffect != null) {
                Dictionary<string, object> keys = new Dictionary<string, object>();
                foreach (OFXParameter p in e.OFXEffect.Parameters) {
                    if (HoldsValue(p) && p.IsAnimated) keys[p.Name] = ((Dictionary<string, object>)ParamKeyframes(p))["keyframes"];
                }
                if (keys.Count > 0) d["keyframes"] = keys;
            }
            r.Add(d);
        }
        return r;
    }
}

}
