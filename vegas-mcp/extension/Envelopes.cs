// Envelopes: get_envelopes and set_envelope, on a track (audio: Volume,
// Pan, Mute; video: Composite level, FadeToColor, Mute) or a video event
// (Velocity: its speed over time). The scripting API gives audio events no
// envelopes. A point's time is timeline seconds on a track, seconds from the
// event's start on an event; its value is in the envelope's own range
// (min, max and neutral come back with it: Volume 0 to 2 with 1 = 0 dB,
// Pan -1 left to 1 right, Composite 0 to 1, Velocity a speed).
using System;
using System.Collections.Generic;
using ScriptPortal.Vegas;

namespace DodStudio.VegasBridge {

public partial class Commands {

    Envelopes EnvelopesOf(Args a, out string timesFrom, out string owner) {
        if (a.Has("event")) {
            VideoEvent ve = EventAt(a) as VideoEvent;
            if (ve == null) throw new BridgeError("the scripting API gives audio events no envelopes; use the audio track's");
            timesFrom = "event start";
            owner = "event " + ve.Index + " on track " + ve.Track.Index;
            return ve.Envelopes;
        }
        Track t = TrackAt(a.Int("track"));
        timesFrom = "project start";
        owner = "track " + t.Index;
        return t.Envelopes;
    }

    bool CanAddEnvelope(Args a, EnvelopeType type) {
        if (a.Has("event")) return ((VideoEvent)EventAt(a)).CanAddEnvelope(type);
        return TrackAt(a.Int("track")).CanAddEnvelope(type);
    }

    object GetEnvelopes(Args a) {
        string timesFrom, owner;
        Envelopes envs = EnvelopesOf(a, out timesFrom, out owner);
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["times_from"] = timesFrom;
        List<object> list = new List<object>();
        foreach (Envelope e in envs) list.Add(EnvelopeInfo(e));
        r["envelopes"] = list;
        List<string> can = new List<string>();
        foreach (EnvelopeType t in Enum.GetValues(typeof(EnvelopeType))) if (CanAddEnvelope(a, t)) can.Add(t.ToString());
        r["can_add"] = can;
        return r;
    }

    static object EnvelopeInfo(Envelope e) {
        Dictionary<string, object> d = new Dictionary<string, object>();
        d["type"] = e.Type.ToString();
        d["name"] = e.Name;
        d["min"] = e.Min;
        d["max"] = e.Max;
        d["neutral"] = e.Neutral;
        List<object> pts = new List<object>();
        foreach (EnvelopePoint p in e.Points) {
            Dictionary<string, object> pd = new Dictionary<string, object>();
            pd["time"] = Sec(p.X);
            pd["value"] = Values.Round(p.Y);
            pd["curve"] = CurveName(p.Curve);
            pts.Add(pd);
        }
        d["points"] = pts;
        return d;
    }

    // project_info's `detail`: each envelope's type and how many points it has.
    static Dictionary<string, object> EnvelopeCounts(Envelopes envs) {
        Dictionary<string, object> d = new Dictionary<string, object>();
        foreach (Envelope e in envs) d[e.Type.ToString()] = e.Points.Count;
        return d;
    }

    // type: the envelope (added when the track or event has none);
    // points: [{time, value, curve}]; mode "replace" (default: exactly these;
    // VEGAS keeps a point at time 0, which, given none there, takes the first
    // point's value), "add" (changing any point at the same time) or "remove"
    // (the whole envelope).
    object SetEnvelope(Args a) {
        string timesFrom, owner;
        Envelopes envs = EnvelopesOf(a, out timesFrom, out owner);
        EnvelopeType type = Values.Enum<EnvelopeType>(a.Str("type"), "envelope type");
        string mode = a.Str("mode", "replace");
        if (mode != "replace" && mode != "add" && mode != "remove") throw new BridgeError("mode is \"replace\", \"add\" or \"remove\"");
        Envelope env = envs.FindByType(type);
        if (mode == "remove") {
            if (env == null) throw new BridgeError(owner + " has no " + type + " envelope");
            envs.Remove(env);
            return GetEnvelopes(a);
        }
        List<Args> pts = a.Items("points");
        if (pts.Count == 0) throw new BridgeError("points is empty; mode \"remove\" removes the envelope");
        Values.CheckTimes(pts, "point", a.Has("event") ? Sec(EventAt(a).Length) : double.PositiveInfinity, timesFrom);
        Values.SortByTime(pts);
        if (env == null) {
            if (!CanAddEnvelope(a, type)) throw new BridgeError(owner + " cannot have a " + type + " envelope (get_envelopes lists can_add)");
            env = new Envelope(type);
            envs.Add(env);
        } else if (mode == "replace") {
            // Clear leaves the point at time 0.
            env.Points.Clear();
        }
        foreach (Args p in pts) SetEnvelopePoint(env, p);
        if (mode == "replace" && pts[0].Num("time") > Values.SameTime) {
            EnvelopePoint zero = env.Points.GetPointAtX(Tc(0));
            if (zero != null) {
                zero.Y = pts[0].Num("value");
                if (pts[0].Has("curve")) zero.Curve = Curve(pts[0].Str("curve"));
            }
        }
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["times_from"] = timesFrom;
        r["envelope"] = EnvelopeInfo(env);
        return r;
    }

    static void SetEnvelopePoint(Envelope env, Args p) {
        double t = p.Num("time"), y = p.Num("value");
        if (y < env.Min || y > env.Max) throw new BridgeError(env.Type + " values run " + env.Min + " to " + env.Max + "; got " + y);
        CurveType curve = p.Has("curve") ? Curve(p.Str("curve")) : CurveType.Linear;
        EnvelopePoint at = null;
        foreach (EnvelopePoint q in env.Points) if (Math.Abs(Sec(q.X) - t) < Values.SameTime) at = q;
        if (at != null) {
            at.Y = y;
            at.Curve = curve;
        } else {
            env.Points.Add(new EnvelopePoint(Tc(t), y, curve));
        }
    }
}

}
