// Fades and transitions: list_transitions, set_fade (an event's fade in or
// out: length, curve, and on video the transition it plays) and crossfade
// (two events on one track overlapping, the later one's fade-in carrying the
// transition). Lengths are timeline seconds.
using System;
using System.Collections.Generic;
using ScriptPortal.Vegas;

namespace DodStudio.VegasBridge {

public partial class Commands {

    // VEGAS lists a transition under several folders; each comes once.
    static void CollectTransitions(PlugInNode node, string filter, Dictionary<string, bool> seen, List<object> into) {
        foreach (PlugInNode c in node) {
            if (c.IsContainer) { CollectTransitions(c, filter, seen, into); continue; }
            string id = c.UniqueID ?? c.Name;
            if (seen.ContainsKey(id)) continue;
            if (filter.Length > 0 && c.Name.IndexOf(filter, StringComparison.OrdinalIgnoreCase) < 0 &&
                id.IndexOf(filter, StringComparison.OrdinalIgnoreCase) < 0) continue;
            seen[id] = true;
            Dictionary<string, object> d = new Dictionary<string, object>();
            d["name"] = c.Name;
            d["id"] = c.UniqueID;
            d["ofx"] = c.IsOFX;
            into.Add(d);
        }
    }

    object ListTransitions(Args a) {
        List<object> r = new List<object>();
        CollectTransitions(vegas.Transitions, a.Str("filter", ""), new Dictionary<string, bool>(), r);
        return r;
    }

    // A fade as JSON. `full` adds the transition's parameters.
    static object FadeInfo(Fade f, bool video, bool full) {
        Dictionary<string, object> d = new Dictionary<string, object>();
        d["length"] = Sec(f.Length);
        d["curve"] = f.Curve.ToString();
        d["reciprocal_curve"] = f.ReciprocalCurve.ToString();
        Effect t = video ? f.Transition as Effect : null;
        if (t != null) {
            if (full) d["transition"] = EffectInfo(t);
            else d["transition"] = t.PlugIn != null ? t.PlugIn.Name : "?";
        }
        return d;
    }

    // set_fade: edge "in" or "out"; length; curve and reciprocal_curve (the
    // curve the other event of a crossfade follows); transition (a name or
    // id from list_transitions, or "none"), its preset and OFX params.
    object SetFade(Args a) {
        TrackEvent ev = EventAt(a);
        string edge = a.Str("edge");
        if (edge != "in" && edge != "out") throw new BridgeError("edge is \"in\" or \"out\"");
        Fade f = edge == "in" ? ev.FadeIn : ev.FadeOut;
        if (a.Has("length")) {
            double len = a.Num("length");
            if (len < 0 || len > Sec(ev.Length)) throw new BridgeError("fade length " + len + " s must be 0 to the event's " + Sec(ev.Length) + " s");
            f.Length = Tc(len);
        }
        SetFadeLook(f, ev, a, "curve", "reciprocal_curve");
        return FadeReply(ev);
    }

    // The curves and the transition of one fade, from the named arguments.
    void SetFadeLook(Fade f, TrackEvent ev, Args a, string curve, string reciprocal) {
        if (a.Has(curve)) f.Curve = Curve(a.Str(curve));
        if (reciprocal != null && a.Has(reciprocal)) f.ReciprocalCurve = Curve(a.Str(reciprocal));
        if (!a.Has("transition")) {
            if (a.Has("preset") || a.Has("params")) throw new BridgeError("preset and params need a transition");
            return;
        }
        if (!ev.IsVideo()) throw new BridgeError("transitions are for video events; an audio fade has only a curve");
        string want = a.Str("transition");
        if (want.Length == 0 || string.Equals(want, "none", StringComparison.OrdinalIgnoreCase)) {
            f.RemoveTransition();
            return;
        }
        PlugInNode node = FindPlugIn(vegas.Transitions, want);
        if (node == null) throw new BridgeError("no transition named or with id \"" + want + "\" (see list_transitions)");
        f.Transition = new Effect(node);
        // Set on the effect the fade holds, in case assigning copied it.
        Effect live = f.Transition as Effect;
        if (live == null) throw new BridgeError("VEGAS did not put " + node.Name + " on the fade");
        if (a.Has("preset")) {
            string preset = a.Str("preset");
            bool found = false;
            foreach (EffectPreset p in live.Presets) if (string.Equals(p.Name, preset, StringComparison.OrdinalIgnoreCase)) { preset = p.Name; found = true; }
            if (!found) throw new BridgeError(node.Name + " has no preset \"" + preset + "\"");
            live.Preset = preset;
        }
        if (a.Has("params")) SetParams(live, a.Obj("params"));
    }

    static CurveType Curve(string name) {
        // "hold" is what VEGAS's own windows call the curve its API names None.
        if (string.Equals(name, "hold", StringComparison.OrdinalIgnoreCase)) return CurveType.None;
        CurveType c = Values.Enum<CurveType>(name, "curve");
        if (c == CurveType.Invalid) throw new BridgeError("curve \"Invalid\" is not a curve");
        return c;
    }

    object FadeReply(TrackEvent ev) {
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["track"] = ev.Track.Index;
        r["event"] = ev.Index;
        r["start"] = Sec(ev.Start);
        r["length"] = Sec(ev.Length);
        if (ev.ActiveTake != null) r["offset"] = MediaOffset(ev);
        r["fade_in"] = FadeInfo(ev.FadeIn, ev.IsVideo(), true);
        r["fade_out"] = FadeInfo(ev.FadeOut, ev.IsVideo(), true);
        return r;
    }

    // crossfade: the event and the one before it on its track overlap by
    // `length`, the earlier fading out as the later fades in. The later
    // event's start moves to make that overlap, its picture staying where it
    // was on the timeline (VEGAS moves the take's offset with it), so it needs
    // that much media before its first frame. Its fade-in carries the
    // transition (curve_in), the earlier's fade-out curve_out.
    object Crossfade(Args a) {
        TrackEvent later = EventAt(a);
        TrackEvent earlier = null;
        foreach (TrackEvent ev in later.Track.Events) {
            if (ev.Equals(later) || ev.Start >= later.Start) continue;
            if (earlier == null || ev.Start > earlier.Start) earlier = ev;
        }
        if (earlier == null) throw new BridgeError("event " + later.Index + " has no event before it on track " + later.Track.Index);
        double length = a.Num("length");
        if (length <= 0) throw new BridgeError("a crossfade's length must be above 0");
        double newStart = Sec(earlier.End) - length;
        if (newStart <= Sec(earlier.Start)) throw new BridgeError("a " + length + " s crossfade is longer than event " + earlier.Index + " (" + Sec(earlier.Length) + " s)");
        double move = Sec(later.Start) - newStart;
        if (move > 0 && later.ActiveTake != null && Sec(later.ActiveTake.Offset) < move - Values.SameTime)
            throw new BridgeError("event " + later.Index + " starts " + Values.Round(MediaOffset(later)) + " s into its media, too little to start " +
                                  Values.Round(move) + " s earlier for this crossfade");
        double newLength = Sec(later.Length) + move;
        if (newLength <= length) throw new BridgeError("a " + length + " s crossfade is longer than event " + later.Index);
        later.AdjustStartLength(Tc(newStart), Tc(newLength), true);
        later.FadeIn.Length = Tc(length);
        earlier.FadeOut.Length = Tc(length);
        SetFadeLook(later.FadeIn, later, a, "curve_in", null);
        if (a.Has("curve_out")) earlier.FadeOut.Curve = Curve(a.Str("curve_out"));
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["earlier"] = FadeReply(earlier);
        r["later"] = FadeReply(later);
        return r;
    }
}

}
