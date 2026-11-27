// apply_to_events: one change to every event a filter picks, in one call
// and one undo step (a failure on any event rolls the whole call back), the
// way a look is put on a whole edit. Each change runs the single-event
// command of the same kind (set_event, add_effect, set_effect_params,
// set_pan_crop, set_fade) with that event's track and index, so it means
// exactly what that command means.
using System;
using System.Collections.Generic;
using ScriptPortal.Vegas;

namespace DodStudio.VegasBridge {

public partial class Commands {

    // The changes apply_to_events knows, in the order it makes them.
    static readonly string[] BatchChanges = { "set_event", "add_effect", "set_effect", "remove_effect", "pan_crop", "fade_in", "fade_out" };

    static readonly string[] FilterKeys = { "all", "track", "tracks", "start", "end", "media", "name", "selected", "type" };

    // What each change takes. A key it does not know is refused rather than
    // dropped: a typo would otherwise make the change to every event the
    // filter picks without the part that was meant.
    static readonly Dictionary<string, string[]> BatchChangeKeys = new Dictionary<string, string[]> {
        { "set_event", new string[] { "playback_rate", "start", "shift", "length", "offset", "resample", "name", "mute", "locked", "loop", "snap_offset" } },
        { "add_effect", new string[] { "effect", "preset", "params", "target" } },
        { "set_effect", new string[] { "effect", "preset", "params", "bypass", "target" } },
        { "remove_effect", new string[] { "effect", "target" } },
        { "pan_crop", new string[] { "keyframes", "zoom", "pan", "mode", "reset" } },
        { "fade_in", new string[] { "length", "curve", "reciprocal_curve", "transition", "preset", "params" } },
        { "fade_out", new string[] { "length", "curve", "reciprocal_curve", "transition", "preset", "params" } },
    };

    static void RefuseUnknownKeys(IEnumerable<string> keys, string[] known, string what) {
        List<string> unknown = new List<string>();
        foreach (string k in keys) if (Array.IndexOf(known, k) < 0) unknown.Add(k);
        if (unknown.Count > 0)
            throw new BridgeError(what + " has no \"" + string.Join("\", \"", unknown.ToArray()) + "\"; it takes " + string.Join(", ", known));
    }

    object ApplyToEvents(Args a) {
        List<string> known = new List<string>(BatchChanges);
        known.Add("filter");
        RefuseUnknownKeys(a.Keys, known.ToArray(), "apply_to_events");
        if (!a.Has("filter")) throw new BridgeError("missing argument \"filter\" ({\"all\": true} picks every event)");
        if (a.Obj("filter") == null) throw new BridgeError("filter must be an object");
        Args filter = new Args(a.Obj("filter"));
        bool any = false;
        foreach (string c in BatchChanges) {
            if (!a.Has(c)) continue;
            any = true;
            if (a.Obj(c) == null) throw new BridgeError(c + " must be an object");
            RefuseUnknownKeys(a.Obj(c).Keys, BatchChangeKeys[c], c);
        }
        if (!any) throw new BridgeError("nothing to change: give one or more of " + string.Join(", ", BatchChanges));
        List<TrackEvent> picked = PickEvents(filter);
        if (picked.Count == 0) throw new BridgeError("no event matches the filter (project_info lists the events)");
        List<object> changed = new List<object>();
        List<object> skipped = new List<object>();
        // A media's Media FX gets each change once, however many events play it.
        Dictionary<string, bool> mediaDone = new Dictionary<string, bool>(StringComparer.OrdinalIgnoreCase);
        foreach (TrackEvent ev in picked) {
            List<string> done = new List<string>();
            foreach (string change in BatchChanges) {
                if (!a.Has(change)) continue;
                string why = ApplyOne(change, ev, a.Obj(change), mediaDone, done);
                if (why != null) skipped.Add(Skipped(ev, change, why));
            }
            if (done.Count > 0) {
                Dictionary<string, object> r = EventRefInfo(ev);
                r["done"] = done;
                changed.Add(r);
            }
        }
        Dictionary<string, object> reply = new Dictionary<string, object>();
        reply["matched"] = picked.Count;
        reply["changed"] = changed;
        if (skipped.Count > 0) reply["skipped"] = skipped;
        if (a.Has("set_event")) {
            List<object> overlaps = OverlapsWithUnpicked(picked);
            if (overlaps.Count > 0) reply["warnings"] = overlaps;
        }
        return reply;
    }

    // Each picked event that now overlaps an event on its track the filter
    // did not pick: a shift or a longer event can land on a neighbour, which
    // VEGAS allows (the two then play as a crossfade).
    static List<object> OverlapsWithUnpicked(List<TrackEvent> picked) {
        List<object> r = new List<object>();
        foreach (TrackEvent ev in picked) {
            foreach (TrackEvent other in ev.Track.Events) {
                if (picked.Contains(other)) continue;
                if (other.Start >= ev.End || ev.Start >= other.End) continue;
                Dictionary<string, object> w = EventRefInfo(ev);
                w["overlaps"] = EventRefInfo(other);
                w["why"] = "now overlaps an event the filter did not pick";
                r.Add(w);
            }
        }
        return r;
    }

    // One change to one event. Returns why it was skipped, or null when made
    // (what it did goes into `done`).
    string ApplyOne(string change, TrackEvent ev, Dictionary<string, object> spec, Dictionary<string, bool> mediaDone, List<string> done) {
        if (spec == null) throw new BridgeError(change + " must be an object");
        VideoEvent ve = ev as VideoEvent;
        if (change == "set_event") {
            Dictionary<string, object> d = new Dictionary<string, object>(spec);
            // shift: move by seconds, each event from its own start.
            if (d.ContainsKey("shift")) {
                if (d.ContainsKey("start")) throw new BridgeError("set_event takes start or shift, not both");
                d["start"] = Sec(ev.Start) + Values.Number(d["shift"], "set_event's shift");
                d.Remove("shift");
            }
            SetEvent(ForEvent(ev, d));
            done.Add("set_event");
            return null;
        }
        if (change == "fade_in" || change == "fade_out") {
            Dictionary<string, object> d = new Dictionary<string, object>(spec);
            d["edge"] = change == "fade_in" ? "in" : "out";
            if (!ev.IsVideo() && d.ContainsKey("transition")) return "a transition is for video events";
            SetFade(ForEvent(ev, d));
            done.Add(change);
            return null;
        }
        if (ve == null) return change + " is for video events";
        if (change == "pan_crop") {
            SetPanCrop(ForEvent(ev, spec));
            done.Add("pan_crop");
            return null;
        }
        bool media = Convert.ToString(Get(spec, "target", "event")) == "media";
        if (Convert.ToString(Get(spec, "target", "event")) != "event" && !media)
            throw new BridgeError(change + "'s target is \"event\" or \"media\"");
        Dictionary<string, object> args = new Dictionary<string, object>(spec);
        if (media) {
            if (ev.ActiveTake == null || ev.ActiveTake.Media == null) return "no media to put a Media FX on";
            args["media"] = ev.ActiveTake.Media.KeyString;
        }
        if (media) {
            string once = change + " " + ev.ActiveTake.Media.KeyString;
            // The chain is the media's, so an event playing the same media again would repeat it.
            if (mediaDone.ContainsKey(once)) return "its media's Media FX had this " + change + " for an earlier event";
            mediaDone[once] = true;
        }
        if (change == "add_effect") {
            Dictionary<string, object> r = (Dictionary<string, object>)AddEffect(ForEvent(ev, args));
            done.Add("add_effect " + r["name"] + " at " + (media ? "Media FX " : "Event FX ") + r["index"]);
            return null;
        }
        // set_effect and remove_effect act on every effect of that plug-in in the chain.
        Effects chain = EffectsOf(ForEvent(ev, args));
        string want = Convert.ToString(Get(spec, "effect", null));
        if (want == null || want.Length == 0) throw new BridgeError(change + " needs \"effect\" (a name or id)");
        List<Effect> hits = new List<Effect>();
        foreach (Effect e in chain) {
            if (e.PlugIn != null && (string.Equals(e.PlugIn.Name, want, StringComparison.OrdinalIgnoreCase) ||
                                     string.Equals(e.PlugIn.UniqueID, want, StringComparison.OrdinalIgnoreCase))) hits.Add(e);
        }
        if (hits.Count == 0) return "no " + want + " in its " + (media ? "Media FX" : "Event FX");
        if (change == "remove_effect") {
            foreach (Effect e in hits) chain.Remove(e);
            done.Add("remove_effect " + want + " x" + hits.Count);
            return null;
        }
        Args set = new Args(args);
        foreach (Effect e in hits) {
            if (set.Has("bypass")) e.Bypass = set.Bool("bypass", false);
            WithPresetAndParams(e, set);
        }
        done.Add("set_effect " + want + " x" + hits.Count);
        return null;
    }

    static object Get(Dictionary<string, object> d, string key, object def) {
        object v;
        return d.TryGetValue(key, out v) && v != null ? v : def;
    }

    // A single-event command's arguments for this event: its track and its
    // index now (a set_event earlier in the call may have moved it).
    static Args ForEvent(TrackEvent ev, Dictionary<string, object> spec) {
        Dictionary<string, object> d = new Dictionary<string, object>(spec);
        d["track"] = ev.Track.Index;
        d["event"] = ev.Index;
        return new Args(d);
    }

    // filter: track or tracks, start/end (events overlapping that stretch of
    // the timeline), media (part of the file path or subclip key), name (part
    // of the event's name), selected, type (video or audio). All given must
    // hold. Every event takes {"all": true}, alone: an empty filter is refused,
    // since a filter whose keys were all mistyped would otherwise be one.
    List<TrackEvent> PickEvents(Args f) {
        RefuseUnknownKeys(f.Keys, FilterKeys, "filter");
        if (f.Keys.Count == 0) throw new BridgeError("filter is empty: give {\"all\": true} to change every event");
        if (f.Has("all")) {
            if (!f.Bool("all", false)) throw new BridgeError("filter all is true or left out");
            if (f.Keys.Count > 1) throw new BridgeError("filter all picks every event, so it goes alone");
        }
        List<int> tracks = null;
        if (f.Has("track")) tracks = new List<int>(new int[] { f.Int("track") });
        if (f.Has("tracks")) {
            if (tracks != null) throw new BridgeError("filter takes track or tracks, not both");
            tracks = new List<int>();
            foreach (object o in f.List("tracks")) tracks.Add((int)Values.Number(o, "filter tracks"));
        }
        if (tracks != null) foreach (int i in tracks) TrackAt(i);  // an unknown track is an error, not "no match"
        string type = f.Str("type", "");
        if (type.Length > 0 && type != "video" && type != "audio") throw new BridgeError("filter type is \"video\" or \"audio\"");
        double from = f.Num("start", double.NegativeInfinity), to = f.Num("end", double.PositiveInfinity);
        if (to <= from) throw new BridgeError("filter end must be after its start");
        string media = f.Str("media", ""), name = f.Str("name", "");
        List<TrackEvent> r = new List<TrackEvent>();
        foreach (Track t in P.Tracks) {
            if (tracks != null && !tracks.Contains(t.Index)) continue;
            if ((type == "video" && !t.IsVideo()) || (type == "audio" && t.IsVideo())) continue;
            foreach (TrackEvent ev in t.Events) {
                if (Sec(ev.End) <= from + Values.SameTime || Sec(ev.Start) >= to - Values.SameTime) continue;
                if (f.Has("selected") && ev.Selected != f.Bool("selected", false)) continue;
                if (name.Length > 0 && (ev.Name ?? "").IndexOf(name, StringComparison.OrdinalIgnoreCase) < 0) continue;
                if (media.Length > 0 && !MediaMatches(ev, media)) continue;
                r.Add(ev);
            }
        }
        return r;
    }

    static bool MediaMatches(TrackEvent ev, string part) {
        if (ev.ActiveTake == null) return false;
        Media m = ev.ActiveTake.Media;
        List<string> names = new List<string>();
        names.Add(ev.ActiveTake.MediaPath);
        if (m != null) {
            names.Add(m.KeyString);
            if (m.IsSubclip()) names.Add(((Subclip)m).ParentMedia.FilePath);
        }
        foreach (string n in names) if (n != null && n.IndexOf(part, StringComparison.OrdinalIgnoreCase) >= 0) return true;
        return false;
    }

    static Dictionary<string, object> EventRefInfo(TrackEvent ev) {
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["track"] = ev.Track.Index;
        r["event"] = ev.Index;
        r["start"] = Sec(ev.Start);
        r["length"] = Sec(ev.Length);
        if (!string.IsNullOrEmpty(ev.Name)) r["name"] = ev.Name;
        if (ev.ActiveTake != null) r["media"] = ev.ActiveTake.MediaPath;
        return r;
    }

    static object Skipped(TrackEvent ev, string change, string why) {
        Dictionary<string, object> r = EventRefInfo(ev);
        r["change"] = change;
        r["why"] = why;
        return r;
    }
}

}
