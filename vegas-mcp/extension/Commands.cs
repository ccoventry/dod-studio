// The bridge's commands: each one is a VEGAS scripting API call or a few,
// run on VEGAS's UI thread by BridgeModule. Times are seconds, track and
// event indices are 0-based, media is named by its file path or, for a
// subclip, the key project_info lists.
//
// Safety: VEGAS reopens the user's last project on start, so nothing here
// saves in place. save_as writes only a path that does not exist yet, unless
// this session wrote it; new_project and open_project refuse to drop unsaved
// changes unless told to; render refuses to overwrite a file.
using System;
using System.Collections;
using System.Collections.Generic;
using System.IO;
using ScriptPortal.Vegas;

namespace DodStudio.VegasBridge {

public class Commands {
    readonly Vegas vegas;
    // Files this session saved, the only ones save_as may overwrite.
    readonly HashSet<string> savedThisSession = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

    public Commands(Vegas vegas) { this.vegas = vegas; }

    public object Run(string cmd, Args a) {
        switch (cmd) {
            case "ping": return Ping();
            case "project_info": return ProjectInfo();
            case "new_project": return NewProject(a);
            case "open_project": return OpenProject(a);
            case "save_as": return SaveAs(a);
            case "list_effects": return ListEffects(a);
            case "list_render_templates": return ListRenderTemplates(a);
            case "render": return Render(a);
            case "run_script": return RunScript(a);
        }
        // An extension's project edits must sit in an undo block: outside
        // one, every change fails with E_UNEXPECTED. One block per command,
        // so Edit > Undo takes back one bridge call.
        using (new UndoBlock(P, "DoD Studio bridge: " + cmd)) {
            switch (cmd) {
                case "add_media": return AddMedia(a);
                case "set_event": return SetEvent(a);
                case "create_subclip": return CreateSubclip(a);
                case "add_effect": return AddEffect(a);
                case "set_effect_params": return SetEffectParams(a);
                default: throw new BridgeError("unknown command \"" + cmd + "\"");
            }
        }
    }

    Project P {
        get {
            if (vegas.Project == null) throw new BridgeError("no project is open");
            return vegas.Project;
        }
    }

    static double Sec(Timecode t) { return t.ToMilliseconds() / 1000.0; }
    static Timecode Tc(double seconds) { return Timecode.FromSeconds(seconds); }

    object Ping() {
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["bridge"] = BridgeModule.Version;
        r["vegas"] = vegas.Version;
        r["project"] = vegas.Project != null ? (object)vegas.Project.FilePath : null;
        return r;
    }

    // ---- project ----

    void RefuseUnsaved(Args a) {
        if (vegas.Project != null && vegas.Project.IsModified && !a.Bool("discard_changes", false))
            throw new BridgeError("the open project (" + (vegas.Project.FilePath ?? "untitled") +
                                  ") has unsaved changes; pass discard_changes=true to drop them");
    }

    object NewProject(Args a) {
        RefuseUnsaved(a);
        if (!vegas.NewProject(false, false)) throw new BridgeError("VEGAS refused to create a project");
        Project p = vegas.Project;
        using (new UndoBlock(p, "DoD Studio bridge: project settings")) {
            if (a.Has("width")) p.Video.Width = a.Int("width");
            if (a.Has("height")) p.Video.Height = a.Int("height");
            if (a.Has("fps")) p.Video.FrameRate = a.Num("fps");
            p.Video.FieldOrder = VideoFieldOrder.ProgressiveScan;
            string pf = a.Str("pixel_format", "");
            if (pf.Length > 0) p.Video.PixelFormat = (PixelFormat)Enum.Parse(typeof(PixelFormat), pf, true);
        }
        return ProjectInfo();
    }

    object OpenProject(Args a) {
        string path = a.Str("path");
        if (!File.Exists(path)) throw new BridgeError("no project at " + path);
        RefuseUnsaved(a);
        // Clear the dirty state first so OpenProject never asks to save.
        vegas.NewProject(false, false);
        if (!vegas.OpenProject(path)) throw new BridgeError("VEGAS could not open " + path);
        return ProjectInfo();
    }

    object SaveAs(Args a) {
        string path = Path.GetFullPath(a.Str("path"));
        if (!path.EndsWith(".veg", StringComparison.OrdinalIgnoreCase)) throw new BridgeError("save_as needs a .veg path");
        if (File.Exists(path) && !(a.Bool("overwrite", false) && savedThisSession.Contains(path)))
            throw new BridgeError(path + " exists; save_as only overwrites a file this bridge session saved, with overwrite=true");
        Directory.CreateDirectory(Path.GetDirectoryName(path));
        if (!vegas.SaveProject(path)) throw new BridgeError("VEGAS could not save " + path);
        savedThisSession.Add(path);
        return ProjectInfo();
    }

    object ProjectInfo() {
        Project p = P;
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["path"] = p.FilePath;
        r["modified"] = p.IsModified;
        r["width"] = p.Video.Width;
        r["height"] = p.Video.Height;
        r["fps"] = p.Video.FrameRate;
        r["pixel_format"] = p.Video.PixelFormat.ToString();
        r["length"] = Sec(p.Length);
        List<object> tracks = new List<object>();
        foreach (Track t in p.Tracks) {
            Dictionary<string, object> tr = new Dictionary<string, object>();
            tr["index"] = t.Index;
            tr["type"] = t.IsVideo() ? "video" : "audio";
            tr["name"] = t.Name;
            List<object> evs = new List<object>();
            foreach (TrackEvent ev in t.Events) evs.Add(EventInfo(ev));
            tr["events"] = evs;
            tracks.Add(tr);
        }
        r["tracks"] = tracks;
        List<object> media = new List<object>();
        foreach (Media m in p.MediaPool) {
            Dictionary<string, object> mi = new Dictionary<string, object>();
            mi["key"] = m.KeyString;
            mi["path"] = m.FilePath;
            mi["subclip"] = m.IsSubclip();
            mi["length"] = Sec(m.Length);
            VideoStream vs = m.GetVideoStreamByIndex(0);
            if (vs != null) mi["fps"] = vs.FrameRate;
            mi["effects"] = EffectsInfo(m.Effects);
            media.Add(mi);
        }
        r["media"] = media;
        return r;
    }

    object EventInfo(TrackEvent ev) {
        Dictionary<string, object> e = new Dictionary<string, object>();
        e["index"] = ev.Index;
        e["name"] = ev.Name;
        e["start"] = Sec(ev.Start);
        e["length"] = Sec(ev.Length);
        e["playback_rate"] = ev.PlaybackRate;
        if (ev.ActiveTake != null) {
            e["media"] = ev.ActiveTake.Media != null ? ev.ActiveTake.Media.KeyString : ev.ActiveTake.MediaPath;
            e["offset"] = Sec(ev.ActiveTake.Offset);
        }
        VideoEvent ve = ev as VideoEvent;
        if (ve != null) {
            e["resample"] = ve.ResampleMode.ToString();
            e["effects"] = EffectsInfo(ve.Effects);
        }
        return e;
    }

    // ---- media and events ----

    Media FindMedia(string keyOrPath) {
        foreach (Media m in P.MediaPool) {
            if (string.Equals(m.KeyString, keyOrPath, StringComparison.OrdinalIgnoreCase)) return m;
        }
        foreach (Media m in P.MediaPool) {
            if (!m.IsSubclip() && string.Equals(m.FilePath, keyOrPath, StringComparison.OrdinalIgnoreCase)) return m;
        }
        return null;
    }

    Media MediaOrImport(string keyOrPath) {
        Media m = FindMedia(keyOrPath);
        if (m != null) return m;
        if (!File.Exists(keyOrPath)) throw new BridgeError("no media \"" + keyOrPath + "\" in the project and no such file");
        return Media.CreateInstance(P, keyOrPath);
    }

    Track TrackAt(int index) {
        foreach (Track t in P.Tracks) if (t.Index == index) return t;
        throw new BridgeError("no track " + index);
    }

    TrackEvent EventAt(Args a) {
        Track t = TrackAt(a.Int("track"));
        int i = a.Int("event");
        foreach (TrackEvent ev in t.Events) if (ev.Index == i) return ev;
        throw new BridgeError("no event " + i + " on track " + t.Index);
    }

    object AddMedia(Args a) {
        Media m = MediaOrImport(a.Str("media"));
        VideoStream vs = m.GetVideoStreamByIndex(0);
        if (vs == null) throw new BridgeError("media has no video stream");
        double rate = a.Num("playback_rate", 1.0);
        double offset = a.Num("offset", 0.0);
        double length = a.Has("length") ? a.Num("length") : (Sec(m.Length) - offset) / rate;
        double start = a.Num("start", 0.0);
        VideoTrack track;
        if (a.Has("track")) {
            track = TrackAt(a.Int("track")) as VideoTrack;
            if (track == null) throw new BridgeError("track " + a.Int("track") + " is not a video track");
        } else {
            track = P.AddVideoTrack();
        }
        VideoEvent ev = track.AddVideoEvent(Tc(start), Tc(length));
        Take take = ev.AddTake(vs);
        take.Offset = Tc(offset);
        ev.PlaybackRate = rate;
        if (a.Has("resample")) ev.ResampleMode = (VideoResampleMode)Enum.Parse(typeof(VideoResampleMode), a.Str("resample"), true);
        if (a.Has("name")) ev.Name = a.Str("name");
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["track"] = track.Index;
        r["event"] = EventInfo(ev);
        if (a.Bool("with_audio", false)) {
            AudioStream aus = m.GetAudioStreamByIndex(0);
            if (aus != null) {
                AudioTrack at = P.AddAudioTrack();
                AudioEvent ae = at.AddAudioEvent(Tc(start), Tc(length));
                Take atake = ae.AddTake(aus);
                atake.Offset = Tc(offset);
                ae.PlaybackRate = rate;
                TrackEventGroup g = new TrackEventGroup(P);
                P.TrackEventGroups.Add(g);
                g.Add(ev);
                g.Add(ae);
                r["audio_track"] = at.Index;
            }
        }
        return r;
    }

    object SetEvent(Args a) {
        TrackEvent ev = EventAt(a);
        if (a.Has("playback_rate")) ev.PlaybackRate = a.Num("playback_rate");
        if (a.Has("start")) ev.Start = Tc(a.Num("start"));
        if (a.Has("length")) ev.Length = Tc(a.Num("length"));
        if (a.Has("offset") && ev.ActiveTake != null) ev.ActiveTake.Offset = Tc(a.Num("offset"));
        if (a.Has("name")) ev.Name = a.Str("name");
        if (a.Has("resample")) {
            VideoEvent ve = ev as VideoEvent;
            if (ve == null) throw new BridgeError("resample is for video events");
            ve.ResampleMode = (VideoResampleMode)Enum.Parse(typeof(VideoResampleMode), a.Str("resample"), true);
        }
        return EventInfo(ev);
    }

    object CreateSubclip(Args a) {
        Media parent = MediaOrImport(a.Str("media"));
        string name = a.Str("name", "subclip " + a.Num("start") + "s");
        Subclip s = new Subclip(P, parent.FilePath, Tc(a.Num("start")), Tc(a.Num("length")), a.Bool("reverse", false), name);
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["key"] = s.KeyString;
        r["length"] = Sec(s.Length);
        r["effects"] = EffectsInfo(s.Effects);
        return r;
    }

    // ---- effects ----

    static PlugInNode FindPlugIn(PlugInNode node, string want) {
        foreach (PlugInNode c in node) {
            if (!c.IsContainer && (string.Equals(c.UniqueID, want, StringComparison.OrdinalIgnoreCase) ||
                                   string.Equals(c.Name, want, StringComparison.OrdinalIgnoreCase))) return c;
            PlugInNode r = FindPlugIn(c, want);
            if (r != null) return r;
        }
        return null;
    }

    static void CollectPlugIns(PlugInNode node, string filter, List<object> into) {
        foreach (PlugInNode c in node) {
            if (c.IsContainer) { CollectPlugIns(c, filter, into); continue; }
            if (filter.Length > 0 && c.Name.IndexOf(filter, StringComparison.OrdinalIgnoreCase) < 0 &&
                (c.UniqueID ?? "").IndexOf(filter, StringComparison.OrdinalIgnoreCase) < 0) continue;
            Dictionary<string, object> d = new Dictionary<string, object>();
            d["name"] = c.Name;
            d["id"] = c.UniqueID;
            d["ofx"] = c.IsOFX;
            into.Add(d);
        }
    }

    object ListEffects(Args a) {
        List<object> r = new List<object>();
        CollectPlugIns(vegas.VideoFX, a.Str("filter", ""), r);
        return r;
    }

    Effects EffectsOf(Args a) {
        string target = a.Str("target", "event");
        if (target == "media") return MediaOrImport(a.Str("media")).Effects;
        if (target == "event") {
            VideoEvent ve = EventAt(a) as VideoEvent;
            if (ve == null) throw new BridgeError("effects go on video events");
            return ve.Effects;
        }
        throw new BridgeError("target is \"event\" or \"media\"");
    }

    object AddEffect(Args a) {
        string want = a.Str("effect");
        PlugInNode fx = FindPlugIn(vegas.VideoFX, want);
        if (fx == null) throw new BridgeError("no video effect named or with id \"" + want + "\" (see list_effects)");
        Effects chain = EffectsOf(a);
        Effect e = chain.AddEffect(fx);
        if (e == null) throw new BridgeError("VEGAS did not add " + fx.Name);
        if (a.Has("params")) SetParams(e, a.Obj("params"));
        return EffectInfo(e);
    }

    object SetEffectParams(Args a) {
        Effects chain = EffectsOf(a);
        int i = a.Int("effect_index");
        if (i < 0 || i >= chain.Count) throw new BridgeError("no effect " + i + " in that chain (" + chain.Count + ")");
        Effect e = chain[i];
        if (a.Has("bypass")) e.Bypass = a.Bool("bypass", false);
        if (a.Has("params")) SetParams(e, a.Obj("params"));
        return EffectInfo(e);
    }

    static void SetParams(Effect e, Dictionary<string, object> values) {
        if (values == null) return;
        if (!e.IsOFX || e.OFXEffect == null) throw new BridgeError(e.PlugIn.Name + " is not an OFX effect; its parameters are not scriptable");
        foreach (KeyValuePair<string, object> kv in values) {
            OFXParameter p = null;
            foreach (OFXParameter q in e.OFXEffect.Parameters) {
                if (string.Equals(q.Name, kv.Key, StringComparison.OrdinalIgnoreCase) ||
                    string.Equals(q.Label, kv.Key, StringComparison.OrdinalIgnoreCase)) { p = q; break; }
            }
            if (p == null) throw new BridgeError(e.PlugIn.Name + " has no parameter \"" + kv.Key + "\"");
            if (p is OFXIntegerParameter) ((OFXIntegerParameter)p).Value = Convert.ToInt32(kv.Value);
            else if (p is OFXDoubleParameter) ((OFXDoubleParameter)p).Value = Convert.ToDouble(kv.Value);
            else if (p is OFXBooleanParameter) ((OFXBooleanParameter)p).Value = Convert.ToBoolean(kv.Value);
            else if (p is OFXStringParameter) ((OFXStringParameter)p).Value = Convert.ToString(kv.Value);
            else if (p is OFXChoiceParameter) {
                OFXChoiceParameter c = (OFXChoiceParameter)p;
                OFXChoice pick = null;
                foreach (OFXChoice ch in c.Choices) if (string.Equals(ch.Name, Convert.ToString(kv.Value), StringComparison.OrdinalIgnoreCase)) pick = ch;
                if (pick == null) throw new BridgeError("\"" + kv.Value + "\" is not a choice of " + p.Name);
                c.Value = pick;
            } else throw new BridgeError("parameter " + p.Name + " is a " + p.ParameterType + ", which the bridge cannot set");
        }
        e.OFXEffect.AllParametersChanged();
    }

    static object ParamValue(OFXParameter p) {
        if (p is OFXIntegerParameter) return ((OFXIntegerParameter)p).Value;
        if (p is OFXDoubleParameter) return ((OFXDoubleParameter)p).Value;
        if (p is OFXBooleanParameter) return ((OFXBooleanParameter)p).Value;
        if (p is OFXStringParameter) return ((OFXStringParameter)p).Value;
        if (p is OFXChoiceParameter) { OFXChoice c = ((OFXChoiceParameter)p).Value; return c != null ? c.Name : null; }
        return null;
    }

    static object EffectInfo(Effect e) {
        Dictionary<string, object> d = new Dictionary<string, object>();
        d["index"] = e.Index;
        d["name"] = e.PlugIn != null ? e.PlugIn.Name : null;
        d["id"] = e.PlugIn != null ? e.PlugIn.UniqueID : null;
        d["bypass"] = e.Bypass;
        if (e.IsOFX && e.OFXEffect != null) {
            List<object> ps = new List<object>();
            foreach (OFXParameter p in e.OFXEffect.Parameters) {
                Dictionary<string, object> pd = new Dictionary<string, object>();
                pd["name"] = p.Name;
                pd["label"] = p.Label;
                pd["type"] = p.ParameterType.ToString();
                object v = ParamValue(p);
                if (v != null) pd["value"] = v;
                ps.Add(pd);
            }
            d["params"] = ps;
        }
        return d;
    }

    static List<object> EffectsInfo(Effects chain) {
        List<object> r = new List<object>();
        foreach (Effect e in chain) r.Add(EffectInfo(e));
        return r;
    }

    // ---- render ----

    IEnumerable<KeyValuePair<Renderer, RenderTemplate>> Templates(Args a) {
        string filter = a.Str("template", "");
        string renderer = a.Str("renderer", "");
        foreach (Renderer r in vegas.Renderers) {
            if (renderer.Length > 0 && r.FileTypeName.IndexOf(renderer, StringComparison.OrdinalIgnoreCase) < 0) continue;
            foreach (RenderTemplate t in r.Templates) {
                if (!t.IsValid()) continue;
                if (filter.Length > 0 && t.Name.IndexOf(filter, StringComparison.OrdinalIgnoreCase) < 0) continue;
                if (a.Has("fps") && Math.Abs(t.VideoFrameRate - a.Num("fps")) > 0.001) continue;
                if (a.Has("width") && t.VideoWidth != a.Int("width")) continue;
                if (a.Has("height") && t.VideoHeight != a.Int("height")) continue;
                yield return new KeyValuePair<Renderer, RenderTemplate>(r, t);
            }
        }
    }

    static Dictionary<string, object> TemplateInfo(Renderer r, RenderTemplate t) {
        Dictionary<string, object> d = new Dictionary<string, object>();
        d["renderer"] = r.FileTypeName;
        d["template"] = t.Name;
        d["width"] = t.VideoWidth;
        d["height"] = t.VideoHeight;
        d["fps"] = t.VideoFrameRate;
        d["video_streams"] = t.VideoStreamCount;
        d["extension"] = t.FileExtensions.Length > 0 ? t.FileExtensions[0].TrimStart('*') : "";
        return d;
    }

    object ListRenderTemplates(Args a) {
        List<object> r = new List<object>();
        int limit = a.Int("limit", 200);
        foreach (KeyValuePair<Renderer, RenderTemplate> kv in Templates(a)) {
            if (r.Count >= limit) break;
            r.Add(TemplateInfo(kv.Key, kv.Value));
        }
        return r;
    }

    object Render(Args a) {
        string path = Path.GetFullPath(a.Str("path"));
        if (File.Exists(path)) throw new BridgeError(path + " exists; render never overwrites");
        if (!a.Has("template")) throw new BridgeError("missing argument \"template\" (a name or part of one, see list_render_templates)");
        KeyValuePair<Renderer, RenderTemplate> pick = new KeyValuePair<Renderer, RenderTemplate>();
        bool found = false;
        foreach (KeyValuePair<Renderer, RenderTemplate> kv in Templates(a)) {
            bool exact = string.Equals(kv.Value.Name, a.Str("template"), StringComparison.OrdinalIgnoreCase);
            if (!found || exact) { pick = kv; found = true; }
            if (exact) break;
        }
        if (!found) throw new BridgeError("no render template matches (see list_render_templates)");
        Directory.CreateDirectory(Path.GetDirectoryName(path));
        RenderArgs args = new RenderArgs(P);
        args.OutputFile = path;
        args.RenderTemplate = pick.Value;
        args.Start = Tc(a.Num("start", 0.0));
        args.Length = a.Has("length") ? Tc(a.Num("length")) : Tc(Sec(P.Length) - a.Num("start", 0.0));
        DateTime t0 = DateTime.Now;
        RenderStatus st;
        int hints;
        using (HintDismisser hd = new HintDismisser()) {
            st = vegas.Render(args);
            hints = hd.Dismissed;
        }
        Dictionary<string, object> r = TemplateInfo(pick.Key, pick.Value);
        r["status"] = st.ToString();
        r["hints_dismissed"] = hints;
        r["path"] = path;
        r["seconds"] = (DateTime.Now - t0).TotalSeconds;
        r["exists"] = File.Exists(path);
        return r;
    }

    // Escape hatch for anything the commands above do not cover: a VEGAS
    // script file (.cs/.js/.vb), run like Tools > Scripting > Run Script.
    object RunScript(Args a) {
        string path = a.Str("path");
        if (!File.Exists(path)) throw new BridgeError("no script at " + path);
        vegas.RunScriptFile(path, a.Str("script_args", ""));
        return "ran " + path;
    }
}

}
