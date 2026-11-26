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

    object ApplyToEvents(Args a) {
        if (!a.Has("filter")) throw new BridgeError("missing argument \"filter\" ({} picks every event)");
        Args filter = new Args(a.Obj("filter"));
        bool any = false;
        foreach (string c in BatchChanges) any |= a.Has(c);
        if (!any) throw new BridgeError("nothing to change: give one or more of " + string.Join(", ", BatchChanges));
        List<TrackEvent> picked = PickEvents(filter);
        if (picked.Count == 0) throw new BridgeError("no event matches the filter (project_info lists the events)");
        List<object> changed = new List<object>();
        List<object> skipped = new List<object>();
        // A media's Media FX gets an add_effect once, however many events play it.
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
        return reply;
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
                d["start"] = Sec(ev.Start) + Convert.ToDouble(d["shift"]);
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
        if (change == "add_effect") {
            if (media && mediaDone.ContainsKey(ev.ActiveTake.Media.KeyString)) return "its media's Media FX already got it from an earlier event";
            if (media) mediaDone[ev.ActiveTake.Media.KeyString] = true;
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
    // of the event's name), selected, type (video or audio); {} picks every
    // event. All given must hold.
    List<TrackEvent> PickEvents(Args f) {
        List<int> tracks = null;
        if (f.Has("track")) tracks = new List<int>(new int[] { f.Int("track") });
        if (f.Has("tracks")) {
            if (tracks != null) throw new BridgeError("filter takes track or tracks, not both");
            tracks = new List<int>();
            foreach (object o in f.List("tracks")) tracks.Add(Convert.ToInt32(o));
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
