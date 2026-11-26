// The bridge's commands: each one is a VEGAS scripting API call or a few,
// run on VEGAS's UI thread by BridgeModule. Times are seconds, track and
// event indices are 0-based, media is named by its file path or, for a
// subclip, the key project_info lists.
//
// Safety: these run only in a VEGAS started for the bridge (Session). Every
// file they write, and every project or script they open, is inside its work
// folder: open_project copies a project from anywhere else into it and opens
// the copy, and import_edl takes only EDL text files, so the user's own .veg
// files are never opened or saved. Nothing
// saves in place: save_as writes only a path that does not exist yet, unless
// this session wrote it; new_project and open_project refuse to drop unsaved
// changes unless told to; render and snapshot never overwrite a file.
using System;
using System.Collections;
using System.Collections.Generic;
using System.IO;
using System.Text.RegularExpressions;
using ScriptPortal.Vegas;

namespace DodStudio.VegasBridge {

// Partial: each project-detail area (keyframes, Pan/Crop, Track Motion,
// fades and transitions, markers, envelopes, event groups), presets and the
// effect catalogue, changes to many events, and comparing projects has its
// own file.
public partial class Commands {
    readonly Vegas vegas;
    // Files this session saved, the only ones save_as may overwrite.
    readonly HashSet<string> savedThisSession = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
    readonly DialogAnswerer dialogs;
    readonly Snapshot snapshot;

    public Commands(Vegas vegas, DialogAnswerer dialogs) {
        this.vegas = vegas;
        this.dialogs = dialogs;
        snapshot = new Snapshot(vegas);
    }

    public object Run(string cmd, Args a) {
        switch (cmd) {
            case "ping": return Ping();
            case "project_info": return Read(delegate { return ProjectInfo(a.Bool("detail", false)); });
            case "get_keyframes": return Read(delegate { return GetKeyframes(a); });
            case "get_pan_crop": return Read(delegate { return GetPanCrop(a); });
            case "get_track_motion": return Read(delegate { return GetTrackMotion(a); });
            case "get_markers": return Read(delegate { return GetMarkers(); });
            case "get_envelopes": return Read(delegate { return GetEnvelopes(a); });
            case "list_transitions": return ListTransitions(a);
            case "new_project": return NewProject(a);
            case "open_project": return OpenProject(a);
            case "import_edl": return ImportEdl(a);
            case "save_as": return SaveAs(a);
            case "list_effects": return ListEffects(a);
            // Not edits: a throwaway copy in an undo block that is always cancelled (Presets.cs).
            case "list_presets": return ListPresets(a);
            case "read_preset": return ReadPreset(a);
            // Opens copies, then reopens the open project (CompareProjects.cs).
            case "compare_projects": return CompareProjects(a);
            case "list_render_templates": return ListRenderTemplates(a);
            case "render": return Render(a);
            case "run_script": return RunScript(a);
            // Not an edit: it leaves the project, cursor and preview as they were (Snapshot.cs).
            case "snapshot": return snapshot.Take(a);
        }
        // An extension's project edits must sit in an undo block: outside
        // one, every change fails with E_UNEXPECTED. One block per command,
        // so Edit > Undo takes back one bridge call. A command that fails
        // part-way is rolled back whole, so an error never leaves half an edit.
        Func<Args, object> edit = EditCommand(cmd);
        UndoBlock block = new UndoBlock(P, "DoD Studio bridge: " + cmd);
        try {
            return edit(a);
        } catch {
            block.Cancel = true;
            throw;
        } finally {
            block.Dispose();
        }
    }

    // Reading can make VEGAS set up what is read (VideoTrack.TrackMotion on a
    // track that never had any), which outside an undo block fails with
    // E_UNEXPECTED. Reads run in a block that is always cancelled, so they
    // leave no undo step and no change.
    object Read(Func<object> read) {
        UndoBlock block = new UndoBlock(P, "DoD Studio bridge: read");
        block.Cancel = true;
        try {
            return read();
        } finally {
            block.Dispose();
        }
    }

    Func<Args, object> EditCommand(string cmd) {
        switch (cmd) {
            case "add_media": return AddMedia;
            case "set_event": return SetEvent;
            case "create_subclip": return CreateSubclip;
            case "add_effect": return AddEffect;
            case "set_effect_params": return SetEffectParams;
            case "blend_by_speed": return BlendBySpeed;
            case "set_keyframes": return SetKeyframes;
            case "set_pan_crop": return SetPanCrop;
            case "set_track_motion": return SetTrackMotion;
            case "set_fade": return SetFade;
            case "crossfade": return Crossfade;
            case "set_markers": return SetMarkers;
            case "set_envelope": return SetEnvelope;
            case "group_events": return GroupEvents;
            case "apply_to_events": return ApplyToEvents;
            default: throw new BridgeError("unknown command \"" + cmd + "\"");
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
        r["pid"] = Session.Pid;
        r["pipe"] = Session.PipeName;
        r["work_folder"] = Session.Root;
        r["project"] = vegas.Project != null ? (object)vegas.Project.FilePath : null;
        r["dialogs_answered"] = dialogs.All();
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

    // A project outside the work folder is never opened itself: it is copied
    // into <work folder>\copies and the copy opened. The .veg keeps its
    // media's full paths, so the copy finds the same files.
    object OpenProject(Args a) {
        string path = Path.GetFullPath(a.Str("path"));
        if (!File.Exists(path)) throw new BridgeError("no project at " + path);
        if (!path.EndsWith(".veg", StringComparison.OrdinalIgnoreCase)) throw new BridgeError("open_project needs a .veg path");
        RefuseUnsaved(a);
        string copiedFrom = null;
        if (!Session.UnderRoot(path)) {
            copiedFrom = path;
            path = Session.FreshCopyPath(path);
            File.Copy(copiedFrom, path);
            Log.Write("open_project: copied " + copiedFrom + " to " + path);
        }
        // Clear the dirty state first so OpenProject never asks to save.
        vegas.NewProject(false, false);
        if (!vegas.OpenProject(path)) throw new BridgeError("VEGAS could not open " + path);
        object info = ProjectInfo();
        if (copiedFrom == null) return info;
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["opened_copy"] = path;
        r["copied_from"] = copiedFrom;
        r["project"] = info;
        return r;
    }

    // An EDL (DoD Studio's Export VEGAS EDL / Export Project to VEGAS, or
    // any EDL Text File) onto the timeline, the way File > Open of an EDL
    // does it (VEGAS's OpenFile, as #110 measured). Measured here (#1303's
    // test): VEGAS opens an EDL as a NEW project, replacing the open one
    // (asking to save it when it has changes, so that is refused first like
    // open_project), and OpenFile returns before the events exist, so this
    // pumps messages until they appear. Then the size and rate like
    // new_project, and Select None, because the nameplate script works on
    // the selection when there is one (docs/vegas_nameplates.md).
    object ImportEdl(Args a) {
        string path = Path.GetFullPath(a.Str("path"));
        if (!File.Exists(path)) throw new BridgeError("no EDL at " + path);
        // OpenFile opens anything VEGAS can, a .veg in place too, so only an
        // EDL text file goes through: the user's project is opened only as a
        // copy, by open_project.
        string ext = Path.GetExtension(path);
        if (!ext.Equals(".txt", StringComparison.OrdinalIgnoreCase) && !ext.Equals(".edl", StringComparison.OrdinalIgnoreCase))
            throw new BridgeError("import_edl takes an EDL text file (.txt or .edl), got " + path + "; open_project opens a .veg");
        RefuseUnsaved(a);
        // Clear the dirty state first so OpenFile never asks to save.
        vegas.NewProject(false, false);
        DateTime until = DateTime.Now.AddSeconds(a.Num("wait_seconds", 60));
        // A file that is not an EDL VEGAS can read fails inside VEGAS's
        // reader with a bare ArgumentOutOfRangeException, which names
        // neither the file nor the reason.
        try {
            vegas.OpenFile(path);
            while (DateTime.Now < until && (vegas.Project == null || vegas.Project.Tracks.Count == 0)) {
                System.Windows.Forms.Application.DoEvents();
                System.Threading.Thread.Sleep(50);
            }
        } catch (Exception e) {
            if (e is BridgeError) throw;
            throw new BridgeError("VEGAS could not read " + path + " as an EDL (" + e.GetType().Name + ": " + e.Message +
                                  "); import_edl takes VEGAS's own EDL text format");
        }
        Project p = P;
        if (p.Tracks.Count == 0) throw new BridgeError("VEGAS put nothing on the timeline from " + path + " within wait_seconds");
        int events = 0, offline = 0;
        using (new UndoBlock(p, "DoD Studio bridge: EDL settings, select none")) {
            if (a.Has("width")) p.Video.Width = a.Int("width");
            if (a.Has("height")) p.Video.Height = a.Int("height");
            if (a.Has("fps")) p.Video.FrameRate = a.Num("fps");
            foreach (Track t in p.Tracks) {
                foreach (TrackEvent ev in t.Events) {
                    events++;
                    if (a.Bool("select_none", true)) ev.Selected = false;
                    if (ev.ActiveTake == null || ev.ActiveTake.Media == null || ev.ActiveTake.Media.IsOffline()) offline++;
                }
            }
        }
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["edl"] = path;
        r["tracks"] = p.Tracks.Count;
        r["events"] = events;
        r["offline_events"] = offline;
        r["project"] = ProjectInfo();
        return r;
    }

    object SaveAs(Args a) {
        string path = Session.RequireUnderRoot(a.Str("path"), "save_as's path");
        if (!path.EndsWith(".veg", StringComparison.OrdinalIgnoreCase)) throw new BridgeError("save_as needs a .veg path");
        if (File.Exists(path) && !(a.Bool("overwrite", false) && savedThisSession.Contains(path)))
            throw new BridgeError(path + " exists; save_as only overwrites a file this bridge session saved, with overwrite=true");
        Directory.CreateDirectory(Path.GetDirectoryName(path));
        if (!vegas.SaveProject(path)) throw new BridgeError("VEGAS could not save " + path);
        savedThisSession.Add(path);
        return ProjectInfo();
    }

    object ProjectInfo() { return ProjectInfo(false); }

    // Compact by default: tracks, events and media. `detail` adds each
    // event's fades, snap offset, envelope and Pan/Crop keyframe counts, each
    // track's FX, envelopes and Track Motion keyframe count, and the project's
    // Video Output FX, markers, regions and event groups. Keyframes and points
    // themselves come from the get_* commands.
    object ProjectInfo(bool detail) {
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
            if (t.Mute) tr["mute"] = true;
            if (t.Solo) tr["solo"] = true;
            if (detail) TrackDetail(t, tr);
            List<object> evs = new List<object>();
            foreach (TrackEvent ev in t.Events) {
                Dictionary<string, object> e = EventInfo(ev);
                if (detail) EventDetail(ev, e);
                evs.Add(e);
            }
            tr["events"] = evs;
            tracks.Add(tr);
        }
        r["tracks"] = tracks;
        if (detail) {
            r["output_effects"] = EffectsInfo(p.VideoBus.Effects);
            Dictionary<string, object> marks = (Dictionary<string, object>)GetMarkers();
            foreach (KeyValuePair<string, object> kv in marks) r[kv.Key] = kv.Value;
            r["groups"] = GroupsInfo();
        }
        List<object> media = new List<object>();
        foreach (Media m in p.MediaPool) {
            Dictionary<string, object> mi = new Dictionary<string, object>();
            mi["key"] = m.KeyString;
            mi["path"] = m.FilePath;
            mi["subclip"] = m.IsSubclip();
            mi["offline"] = m.IsOffline();
            mi["length"] = Sec(m.Length);
            VideoStream vs = m.GetVideoStreamByIndex(0);
            if (vs != null) mi["fps"] = vs.FrameRate;
            mi["effects"] = EffectsInfo(m.Effects);
            media.Add(mi);
        }
        r["media"] = media;
        return r;
    }

    Dictionary<string, object> EventInfo(TrackEvent ev) {
        Dictionary<string, object> e = new Dictionary<string, object>();
        e["index"] = ev.Index;
        e["name"] = ev.Name;
        e["start"] = Sec(ev.Start);
        e["length"] = Sec(ev.Length);
        e["playback_rate"] = ev.PlaybackRate;
        e["selected"] = ev.Selected;
        // Only when not VEGAS's default (loop is on by default), so the usual
        // event stays short.
        if (ev.Mute) e["mute"] = true;
        if (ev.Locked) e["locked"] = true;
        if (!ev.Loop) e["loop"] = false;
        if (ev.IsGrouped) e["group"] = GroupIndex(ev);
        if (ev.ActiveTake != null) {
            Media m = ev.ActiveTake.Media;
            e["media"] = m != null ? m.KeyString : ev.ActiveTake.MediaPath;
            // The file it plays (a subclip's parent), and whether VEGAS
            // found it: an offline event renders as a placeholder.
            e["media_path"] = m == null ? ev.ActiveTake.MediaPath
                            : m.IsSubclip() ? ((Subclip)m).ParentMedia.FilePath : m.FilePath;
            e["offline"] = m == null || m.IsOffline();
            e["offset"] = MediaOffset(ev);
        }
        VideoEvent ve = ev as VideoEvent;
        if (ve != null) {
            e["resample"] = ve.ResampleMode.ToString();
            e["effects"] = EffectsInfo(ve.Effects);
        }
        return e;
    }

    // project_info's `detail` for one event: its fades, snap offset, and how
    // many envelope points and Pan/Crop keyframes it has (get_envelopes and
    // get_pan_crop give them).
    void EventDetail(TrackEvent ev, Dictionary<string, object> e) {
        e["snap_offset"] = Sec(ev.SnapOffset);
        e["fade_in"] = FadeInfo(ev.FadeIn, ev.IsVideo(), false);
        e["fade_out"] = FadeInfo(ev.FadeOut, ev.IsVideo(), false);
        VideoEvent ve = ev as VideoEvent;
        if (ve != null) {
            e["envelopes"] = EnvelopeCounts(ve.Envelopes);
            e["pan_crop_keyframes"] = ve.VideoMotion.Keyframes.Count;
        }
    }

    // project_info's `detail` for one track: its FX, envelopes and, for
    // video, its compositing and Track Motion keyframe count.
    void TrackDetail(Track t, Dictionary<string, object> tr) {
        tr["envelopes"] = EnvelopeCounts(t.Envelopes);
        VideoTrack vt = t as VideoTrack;
        if (vt == null) return;
        tr["effects"] = EffectsInfo(vt.Effects);
        tr["composite_level"] = vt.CompositeLevel;
        tr["composite_mode"] = vt.CompositeMode.ToString();
        tr["track_motion_keyframes"] = vt.TrackMotion.MotionKeyframes.Count;
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
        return EventAt(a.Int("track"), a.Int("event"));
    }

    TrackEvent EventAt(int track, int index) {
        Track t = TrackAt(track);
        foreach (TrackEvent ev in t.Events) if (ev.Index == index) return ev;
        throw new BridgeError("no event " + index + " on track " + t.Index);
    }

    VideoEvent VideoEventAt(Args a) {
        VideoEvent ve = EventAt(a) as VideoEvent;
        if (ve == null) throw new BridgeError("event " + a.Int("event") + " on track " + a.Int("track") + " is not a video event");
        return ve;
    }

    // A take's Offset is in stretched (timeline) time: the media second an
    // event starts at is Offset x its speed (measured on build 315: Offset
    // 1 s at rate 0.5 shows the media's 0.5 s). The bridge's `offset` is
    // media seconds, so it reads the same at any speed.
    static double MediaOffset(TrackEvent ev) {
        return Sec(ev.ActiveTake.Offset) * ev.PlaybackRate;
    }

    // Set after the speed: an Offset set at the old speed is clamped to the
    // media's length at that speed.
    static void SetMediaOffset(TrackEvent ev, double mediaSeconds) {
        if (ev.ActiveTake == null) throw new BridgeError("event " + ev.Index + " has no take, so no offset");
        if (ev.PlaybackRate <= 0) throw new BridgeError("event " + ev.Index + " has speed " + ev.PlaybackRate + "; an offset needs a speed above 0");
        ev.ActiveTake.Offset = Tc(mediaSeconds / ev.PlaybackRate);
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
            // Project.AddVideoTrack inserts at index 0, which moves every
            // existing track down one, so a later track index from an earlier
            // reply would reach the wrong track (measured on build 315;
            // AddAudioTrack appends). The new track goes after all of them
            // instead: no index changes. VEGAS draws track 0 in front, so it
            // is behind the others.
            track = new VideoTrack(P, P.Tracks.Count, null);
            P.Tracks.Add(track);
        }
        VideoEvent ev = track.AddVideoEvent(Tc(start), Tc(length));
        ev.AddTake(vs);
        ev.PlaybackRate = rate;
        ev.Length = Tc(length);
        SetMediaOffset(ev, offset);
        if (a.Has("resample")) ev.ResampleMode = (VideoResampleMode)Enum.Parse(typeof(VideoResampleMode), a.Str("resample"), true);
        if (a.Has("name")) ev.Name = a.Str("name");
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["track"] = track.Index;
        r["new_track"] = !a.Has("track");
        r["event"] = EventInfo(ev);
        if (a.Bool("with_audio", false)) {
            AudioStream aus = m.GetAudioStreamByIndex(0);
            if (aus != null) {
                AudioTrack at = P.AddAudioTrack();
                AudioEvent ae = at.AddAudioEvent(Tc(start), Tc(length));
                ae.AddTake(aus);
                ae.PlaybackRate = rate;
                ae.Length = Tc(length);
                SetMediaOffset(ae, offset);
                if (a.Has("name")) ae.Name = a.Str("name");
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
        if (a.Has("playback_rate")) {
            // A new speed keeps the media second the event starts at, unless
            // `offset` says another.
            double keep = ev.ActiveTake != null ? MediaOffset(ev) : 0;
            ev.PlaybackRate = a.Num("playback_rate");
            if (ev.ActiveTake != null && !a.Has("offset")) SetMediaOffset(ev, keep);
        }
        if (a.Has("start")) ev.Start = Tc(a.Num("start"));
        if (a.Has("length")) ev.Length = Tc(a.Num("length"));
        if (a.Has("offset")) SetMediaOffset(ev, a.Num("offset"));
        if (a.Has("name")) ev.Name = a.Str("name");
        if (a.Has("mute")) ev.Mute = a.Bool("mute", false);
        if (a.Has("locked")) ev.Locked = a.Bool("locked", false);
        if (a.Has("loop")) ev.Loop = a.Bool("loop", false);
        // Where the event snaps, in seconds from its start.
        if (a.Has("snap_offset")) ev.SnapOffset = Tc(a.Num("snap_offset"));
        if (a.Has("resample")) {
            VideoEvent ve = ev as VideoEvent;
            if (ve == null) throw new BridgeError("resample is for video events");
            ve.ResampleMode = (VideoResampleMode)Enum.Parse(typeof(VideoResampleMode), a.Str("resample"), true);
        }
        Dictionary<string, object> r = EventInfo(ev);
        r["snap_offset"] = Sec(ev.SnapOffset);
        return r;
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

    // ---- Frame Blend by event speed (#1051) ----

    // VEGAS never tells an OFX effect an event's speed, and only a Media FX
    // sees the 300 fps frames, so the blend cannot follow a ctrl-drag by
    // itself. This does it at edit time instead: each event gets its own
    // subclip of exactly the media it plays (a subclip has its own Media FX
    // chain), with Frame Blend set to the captured frames one output frame
    // spans at the event's speed: 5 at normal speed, 2 at 2.5x slow, none at
    // 5x slow. The event's old take stays, so switching takes undoes it.
    object BlendBySpeed(Args a) {
        Track track = TrackAt(a.Int("track"));
        string effectId = a.Str("effect", "{Svfx:com.dodstudio.FrameBlend}");
        PlugInNode fx = FindPlugIn(vegas.VideoFX, effectId);
        if (fx == null) throw new BridgeError("Frame Blend (" + effectId + ") is not installed in this VEGAS (OFX_PLUGIN_PATH)");
        double captureFps = a.Num("capture_fps", 300.0);
        double projectFps = P.Video.FrameRate;
        int only = a.Int("event", -1);
        List<object> done = new List<object>();
        List<TrackEvent> events = new List<TrackEvent>();
        foreach (TrackEvent ev in track.Events) if (only < 0 || ev.Index == only) events.Add(ev);
        foreach (TrackEvent ev in events) {
            VideoEvent ve = ev as VideoEvent;
            if (ve == null || ve.ActiveTake == null || ve.ActiveTake.Media == null) continue;
            Media m = ve.ActiveTake.Media;
            string parentPath = m.FilePath;
            double baseStart = 0;
            if (m.IsSubclip()) {
                Subclip sc = (Subclip)m;
                parentPath = sc.ParentMedia.FilePath;
                baseStart = Sec(sc.Start);
            }
            double rate = ve.PlaybackRate;
            int frames = (int)Math.Round(captureFps / projectFps * rate);
            frames = Math.Max(1, Math.Min(16, frames));
            double mediaStart = baseStart + MediaOffset(ve);
            // The blend at the event's last frame reads `frames` captured
            // frames on from it, so the subclip runs that much longer.
            double mediaLength = Sec(ve.Length) * rate + frames / captureFps;
            string name = (ve.Name != null && ve.Name.Length > 0 ? ve.Name : Path.GetFileNameWithoutExtension(parentPath)) +
                          " blend " + frames + " @" + mediaStart.ToString("0.000") + "s";
            Subclip sub = new Subclip(P, parentPath, Tc(mediaStart), Tc(mediaLength), false, name);
            if (frames > 1) {
                Effect e = sub.Effects.AddEffect(fx);
                Dictionary<string, object> ps = new Dictionary<string, object>();
                ps["frames"] = frames;
                ps["captureFps"] = captureFps;
                SetParams(e, ps);
            }
            Take t = ve.AddTake(sub.GetVideoStreamByIndex(0), true, name);
            t.Offset = Tc(0);
            Dictionary<string, object> r = new Dictionary<string, object>();
            r["event"] = ve.Index;
            r["playback_rate"] = rate;
            r["frames"] = frames;
            r["subclip"] = sub.KeyString;
            r["media_start"] = mediaStart;
            r["media_length"] = mediaLength;
            done.Add(r);
        }
        return done;
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

    // An FX chain: an event's (Event FX), a media's (Media FX), a video
    // track's (Track FX) or the project's Video Output FX.
    Effects EffectsOf(Args a) {
        string target = a.Str("target", "event");
        if (target == "media") return MediaOrImport(a.Str("media")).Effects;
        if (target == "event") {
            VideoEvent ve = EventAt(a) as VideoEvent;
            if (ve == null) throw new BridgeError("effects go on video events");
            return ve.Effects;
        }
        if (target == "track") {
            Track t = TrackAt(a.Int("track"));
            if (!t.IsVideo()) throw new BridgeError("track " + t.Index + " is an audio track; video effects go on video tracks");
            return t.Effects;
        }
        if (target == "output") return P.VideoBus.Effects;
        throw new BridgeError("target is \"event\", \"media\", \"track\" or \"output\"");
    }

    object AddEffect(Args a) {
        string want = a.Str("effect");
        PlugInNode fx = FindPlugIn(vegas.VideoFX, want);
        if (fx == null) throw new BridgeError("no video effect named or with id \"" + want + "\" (see list_effects)");
        Effects chain = EffectsOf(a);
        Effect e = chain.AddEffect(fx);
        if (e == null) throw new BridgeError("VEGAS did not add " + fx.Name);
        return WithPresetAndParams(e, a);
    }

    object SetEffectParams(Args a) {
        Effect e = EffectAt(a);
        if (a.Has("bypass")) e.Bypass = a.Bool("bypass", false);
        return WithPresetAndParams(e, a);
    }

    // The preset first (Presets.cs), so `params` change it.
    object WithPresetAndParams(Effect e, Args a) {
        string preset = a.Has("preset") ? ApplyPreset(e, a.Str("preset")) : null;
        if (a.Has("params")) SetParams(e, a.Obj("params"));
        Dictionary<string, object> r = (Dictionary<string, object>)EffectInfo(e);
        if (preset != null) r["preset_applied"] = preset;
        return r;
    }

    static void SetParams(Effect e, Dictionary<string, object> values) {
        if (values == null) return;
        if (!e.IsOFX || e.OFXEffect == null) throw new BridgeError(e.PlugIn.Name + " is not an OFX effect; its parameters are not scriptable");
        RequireOFX(e);
        foreach (KeyValuePair<string, object> kv in values) SetOfxValue(FindParam(e, kv.Key), kv.Value, false, null);
        e.OFXEffect.AllParametersChanged();
    }

    static object ParamValue(OFXParameter p) {
        if (!HoldsValue(p)) return null;
        // A parameter type the API gives no Value shows without one, as before.
        try { return OfxJson(Member(p, "Value")); } catch (BridgeError) { return null; }
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
                // A size or rate of 0 means the template follows the project
                // (VEGAS fills it in only at some moments), so it matches.
                if (a.Has("fps") && t.VideoFrameRate > 0 && Math.Abs(t.VideoFrameRate - a.Num("fps")) > 0.001) continue;
                if (a.Has("width") && t.VideoWidth > 0 && t.VideoWidth != a.Int("width")) continue;
                if (a.Has("height") && t.VideoHeight > 0 && t.VideoHeight != a.Int("height")) continue;
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
        string path = Session.RequireUnderRoot(a.Str("path"), "render's path");
        if (File.Exists(path)) throw new BridgeError(path + " exists; render never overwrites");
        List<string> numbered = NumberedFiles(path);
        if (numbered.Count > 0)
            throw new BridgeError(numbered[0] + " exists, and an Image Sequence render to " + path +
                                  " would write over it; render never overwrites");
        if (!a.Has("template")) throw new BridgeError("missing argument \"template\" (a name or part of one, see list_render_templates)");
        KeyValuePair<Renderer, RenderTemplate> pick = new KeyValuePair<Renderer, RenderTemplate>();
        bool found = false;
        foreach (KeyValuePair<Renderer, RenderTemplate> kv in Templates(a)) {
            bool exact = string.Equals(kv.Value.Name, a.Str("template"), StringComparison.OrdinalIgnoreCase);
            if (!found || exact) { pick = kv; found = true; }
            if (exact) break;
        }
        if (!found) throw new BridgeError("no render template matches (see list_render_templates)");
        // VEGAS builds its "match the project" templates when the renderers
        // are first listed in a session; listed before the timeline had video,
        // they stay audio-only until VEGAS restarts.
        if (pick.Value.VideoStreamCount < 1 && !a.Bool("audio_only", false))
            throw new BridgeError("template \"" + pick.Value.Name + "\" (" + pick.Key.FileTypeName + ") has no video stream. " +
                                  "VEGAS fixes its project-matching templates the first time templates are listed in a session: " +
                                  "list or render only after video is on the timeline, or restart VEGAS. Pass audio_only=true to render it anyway.");
        Directory.CreateDirectory(Path.GetDirectoryName(path));
        RenderArgs args = new RenderArgs(P);
        args.OutputFile = path;
        args.RenderTemplate = pick.Value;
        args.Start = Tc(a.Num("start", 0.0));
        args.Length = a.Has("length") ? Tc(a.Num("length")) : Tc(Sec(P.Length) - a.Num("start", 0.0));
        // The first-render Hint box that holds a render until OK is pressed
        // is answered by the session's DialogAnswerer; the reply's "dialogs"
        // says so.
        DateTime t0 = DateTime.Now;
        RenderStatus st = vegas.Render(args);
        Dictionary<string, object> r = TemplateInfo(pick.Key, pick.Value);
        r["status"] = st.ToString();
        r["path"] = path;
        r["seconds"] = (DateTime.Now - t0).TotalSeconds;
        // What was written: the file itself, or an Image Sequence's numbered
        // files (VEGAS writes name_000000.png and on, never name.png).
        List<string> written = File.Exists(path) ? new List<string>(new string[] { path }) : NumberedFiles(path);
        r["exists"] = written.Count > 0;
        r["file_count"] = written.Count;
        r["files"] = written.Count <= MaxFilesListed ? written : written.GetRange(0, MaxFilesListed);
        return r;
    }

    const int MaxFilesListed = 20;

    // The files an Image Sequence render to `path` writes: the name, an
    // underscore and a frame number (measured: f.png gives f_000000.png,
    // f_000001.png ...), in name order.
    static List<string> NumberedFiles(string path) {
        List<string> found = new List<string>();
        string dir = Path.GetDirectoryName(path);
        if (!Directory.Exists(dir)) return found;
        string stem = Path.GetFileNameWithoutExtension(path);
        string ext = Path.GetExtension(path);
        Regex numbered = new Regex("^" + Regex.Escape(stem) + "_[0-9]+" + Regex.Escape(ext) + "$", RegexOptions.IgnoreCase);
        foreach (string f in Directory.GetFiles(dir, stem + "_*" + ext))
            if (numbered.IsMatch(Path.GetFileName(f))) found.Add(f);
        found.Sort(StringComparer.OrdinalIgnoreCase);
        return found;
    }

    // Escape hatch for anything the commands above do not cover: a C# VEGAS
    // script in the work folder, run like Tools > Scripting > Run Script.
    // A compile error in VEGAS's own compiler leaves a modal error box, so
    // the script is compiled here first, the way VEGAS compiles it (C# 5,
    // no System.Core), and a failure comes back as the reply instead.
    object RunScript(Args a) {
        string path = Session.RequireUnderRoot(a.Str("path"), "run_script's path");
        if (!File.Exists(path)) throw new BridgeError("no script at " + path);
        if (!path.EndsWith(".cs", StringComparison.OrdinalIgnoreCase))
            throw new BridgeError("run_script runs C# (.cs) scripts only: they are compiled first so an error never leaves a box open");
        List<string> errors = ScriptCheck.Compile(path);
        if (errors.Count > 0)
            throw new BridgeError("the script does not compile as VEGAS compiles it (C# 5, no System.Core); nothing ran:\n" +
                                  string.Join("\n", errors.ToArray()));
        vegas.RunScriptFile(path, a.Str("script_args", ""));
        return "ran " + path;
    }
}

}
