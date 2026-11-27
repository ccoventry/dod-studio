// Effect parameter keyframes: get_keyframes and set_keyframes, on any FX
// chain (Event FX, Media FX, Track FX, Video Output FX), OFX effects only,
// and the OFX value conversions set_effect_params shares.
//
// A keyframe's time is in its chain's own time: an Event FX's from the
// event's start on the timeline, whatever the event's speed or take offset;
// a Media FX's from the media's start; a Track FX's and a Video Output FX's
// from the project's start.
using System;
using System.Collections;
using System.Collections.Generic;
using System.Reflection;
using ScriptPortal.Vegas;

namespace DodStudio.VegasBridge {

public partial class Commands {

    Effect EffectAt(Args a) {
        Effects chain = EffectsOf(a);
        int i = a.Int("effect_index");
        if (i < 0 || i >= chain.Count) throw new BridgeError("no effect " + i + " in that chain (" + chain.Count + ")");
        return chain[i];
    }

    static string ChainTimeOrigin(Args a) {
        string target = a.Str("target", "event");
        if (target == "event") return "event start";
        if (target == "media") return "media start";
        return "project start";
    }

    // An Event FX keyframe past the event's end never plays; other chains'
    // times are not bounded.
    double ChainTimeLimit(Args a) {
        if (a.Str("target", "event") != "event") return double.PositiveInfinity;
        return Sec(EventAt(a).Length);
    }

    // Legacy DirectX plug-ins hide their parameters from scripts.
    static void RequireOFX(Effect e) {
        if (!e.IsOFX || e.OFXEffect == null)
            throw new BridgeError(e.PlugIn.Name + " is not an OFX effect; its parameters are not scriptable");
    }

    static bool ParamIs(OFXParameter p, string want) {
        return string.Equals(p.Name, want, StringComparison.OrdinalIgnoreCase) ||
               string.Equals(p.Label, want, StringComparison.OrdinalIgnoreCase);
    }

    static OFXParameter FindParam(Effect e, string want) {
        foreach (OFXParameter q in e.OFXEffect.Parameters) if (ParamIs(q, want)) return q;
        throw new BridgeError(e.PlugIn.Name + " has no parameter \"" + want + "\"");
    }

    // Pages, groups and buttons arrange an effect's window; they hold no value.
    static bool HoldsValue(OFXParameter p) {
        return p.ParameterType != OFXParameterType.Page && p.ParameterType != OFXParameterType.Group &&
               p.ParameterType != OFXParameterType.PushButton;
    }

    object GetKeyframes(Args a) {
        Effect e = EffectAt(a);
        RequireOFX(e);
        string want = a.Str("param", "");
        bool all = a.Bool("all", false);
        List<object> ps = new List<object>();
        foreach (OFXParameter p in e.OFXEffect.Parameters) {
            if (!HoldsValue(p)) continue;
            bool pick = want.Length > 0 ? ParamIs(p, want) : p.IsAnimated || (all && p.CanAnimate);
            if (pick) ps.Add(ParamKeyframes(p));
        }
        if (want.Length > 0 && ps.Count == 0) throw new BridgeError(e.PlugIn.Name + " has no parameter \"" + want + "\"");
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["effect_index"] = e.Index;
        r["name"] = e.PlugIn.Name;
        r["id"] = e.PlugIn.UniqueID;
        r["times_from"] = ChainTimeOrigin(a);
        r["params"] = ps;
        return r;
    }

    // Each OFX parameter type keeps its keyframes in a list of its own
    // generic type; all of them derive from OFXKeyframe (time, interpolation).
    static IList KeyframeList(OFXParameter p) {
        return (IList)Member(p, "Keyframes");
    }

    // A property by name, nearest declaration first: some OFX types declare
    // Value again over their generic base's, which makes a plain lookup ambiguous.
    static object Member(object o, string name) {
        for (Type t = o.GetType(); t != null; t = t.BaseType) {
            PropertyInfo pi = t.GetProperty(name, BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly);
            if (pi != null) return pi.GetValue(o, null);
        }
        throw new BridgeError(o.GetType().Name + " has no " + name);
    }

    static object ParamKeyframes(OFXParameter p) {
        Dictionary<string, object> d = new Dictionary<string, object>();
        d["name"] = p.Name;
        d["label"] = p.Label;
        d["type"] = p.ParameterType.ToString();
        d["can_animate"] = p.CanAnimate;
        d["animated"] = p.IsAnimated;
        d["value"] = OfxJson(Member(p, "Value"));
        List<object> keys = new List<object>();
        if (p.IsAnimated) {
            foreach (OFXKeyframe k in KeyframeList(p)) {
                Dictionary<string, object> kd = new Dictionary<string, object>();
                kd["time"] = Sec(k.Time);
                kd["value"] = OfxJson(Member(k, "Value"));
                kd["interpolation"] = k.Interpolation.ToString();
                keys.Add(kd);
            }
        }
        d["keyframes"] = keys;
        return d;
    }

    // An OFX value as JSON: a number, bool, string or choice name, or a list
    // for points ([x, y], [x, y, z]) and colours ([r, g, b, a], 0 to 1).
    static object OfxJson(object v) {
        if (v is OFXDouble2D) { OFXDouble2D p = (OFXDouble2D)v; return new double[] { p.X, p.Y }; }
        if (v is OFXDouble3D) { OFXDouble3D p = (OFXDouble3D)v; return new double[] { p.X, p.Y, p.Z }; }
        if (v is OFXInteger2D) { OFXInteger2D p = (OFXInteger2D)v; return new int[] { p.X, p.Y }; }
        if (v is OFXColor) { OFXColor c = (OFXColor)v; return new double[] { c.R, c.G, c.B, c.A }; }
        if (v is OFXChoice) return ((OFXChoice)v).Name;
        return v;
    }

    // Sets an OFX parameter from a JSON value: its static value, or a
    // keyframe at `at` when `keyed` (VEGAS adds one, or replaces the one there).
    static void SetOfxValue(OFXParameter p, object raw, bool keyed, Timecode at) {
        try {
            SetOfxValueAs(p, raw, keyed, at);
        } catch (FormatException) {
            throw new BridgeError("parameter " + p.Name + " is a " + p.ParameterType + "; \"" + raw + "\" is not one");
        } catch (InvalidCastException) {
            throw new BridgeError("parameter " + p.Name + " is a " + p.ParameterType + "; " + (raw == null ? "null" : raw.GetType().Name) + " is not one");
        }
    }

    static void SetOfxValueAs(OFXParameter p, object raw, bool keyed, Timecode at) {
        string what = "parameter " + p.Name;
        if (p is OFXDoubleParameter) {
            OFXDoubleParameter q = (OFXDoubleParameter)p;
            double v = Convert.ToDouble(raw);
            if (keyed) q.SetValueAtTime(at, v); else q.Value = v;
        } else if (p is OFXIntegerParameter) {
            OFXIntegerParameter q = (OFXIntegerParameter)p;
            int v = Convert.ToInt32(raw);
            if (keyed) q.SetValueAtTime(at, v); else q.Value = v;
        } else if (p is OFXBooleanParameter) {
            OFXBooleanParameter q = (OFXBooleanParameter)p;
            bool v = Convert.ToBoolean(raw);
            if (keyed) q.SetValueAtTime(at, v); else q.Value = v;
        } else if (p is OFXStringParameter) {
            OFXStringParameter q = (OFXStringParameter)p;
            string v = Convert.ToString(raw);
            if (keyed) q.SetValueAtTime(at, v); else q.Value = v;
        } else if (p is OFXChoiceParameter) {
            OFXChoiceParameter q = (OFXChoiceParameter)p;
            OFXChoice v = Choice(q, raw);
            if (keyed) q.SetValueAtTime(at, v); else q.Value = v;
        } else if (p is OFXDouble2DParameter) {
            OFXDouble2DParameter q = (OFXDouble2DParameter)p;
            double[] n = Values.Numbers(raw, 2, what);
            OFXDouble2D v = new OFXDouble2D();
            v.X = n[0]; v.Y = n[1];
            if (keyed) q.SetValueAtTime(at, v); else q.Value = v;
        } else if (p is OFXDouble3DParameter) {
            OFXDouble3DParameter q = (OFXDouble3DParameter)p;
            double[] n = Values.Numbers(raw, 3, what);
            OFXDouble3D v = new OFXDouble3D();
            v.X = n[0]; v.Y = n[1]; v.Z = n[2];
            if (keyed) q.SetValueAtTime(at, v); else q.Value = v;
        } else if (p is OFXInteger2DParameter) {
            OFXInteger2DParameter q = (OFXInteger2DParameter)p;
            double[] n = Values.Numbers(raw, 2, what);
            OFXInteger2D v = new OFXInteger2D();
            v.X = (int)n[0]; v.Y = (int)n[1];
            if (keyed) q.SetValueAtTime(at, v); else q.Value = v;
        } else if (p is OFXRGBParameter) {
            OFXRGBParameter q = (OFXRGBParameter)p;
            double[] c = Values.Colour(raw, what);
            OFXColor v = new OFXColor(c[0], c[1], c[2]);
            if (keyed) q.SetValueAtTime(at, v); else q.Value = v;
        } else if (p is OFXRGBAParameter) {
            OFXRGBAParameter q = (OFXRGBAParameter)p;
            double[] c = Values.Colour(raw, what);
            OFXColor v = new OFXColor(c[0], c[1], c[2], c[3]);
            if (keyed) q.SetValueAtTime(at, v); else q.Value = v;
        } else {
            throw new BridgeError("parameter " + p.Name + " is a " + p.ParameterType + ", which the bridge cannot set");
        }
    }

    // A choice by its name, or by its 0-based position when a number.
    static OFXChoice Choice(OFXChoiceParameter c, object raw) {
        if (!(raw is string)) {
            int i = Convert.ToInt32(raw);
            if (i < 0 || i >= c.Choices.Length) throw new BridgeError(raw + " is not a choice of " + c.Name + " (0 to " + (c.Choices.Length - 1) + ")");
            return c.Choices[i];
        }
        foreach (OFXChoice ch in c.Choices) if (string.Equals(ch.Name, (string)raw, StringComparison.OrdinalIgnoreCase)) return ch;
        throw new BridgeError("\"" + raw + "\" is not a choice of " + c.Name);
    }

    static OFXKeyframe KeyframeAt(OFXParameter p, double seconds) {
        foreach (OFXKeyframe k in KeyframeList(p)) if (Math.Abs(Sec(k.Time) - seconds) < Values.SameTime) return k;
        return null;
    }

    // mode "replace" (default): the parameter's keyframes become exactly
    // these (VEGAS keeps one at time 0: given none there, it takes the first
    // keyframe's value, so the value holds until then). "add": these are added,
    // replacing any at the same time; on a parameter not animated yet, the
    // keyframe at 0 holds its static value. "clear": no keyframes, the static
    // `value` (or the value it had at time 0).
    object SetKeyframes(Args a) {
        Effect e = EffectAt(a);
        RequireOFX(e);
        OFXParameter p = FindParam(e, a.Str("param"));
        string mode = a.Str("mode", "replace");
        if (mode != "replace" && mode != "add" && mode != "clear") throw new BridgeError("mode is \"replace\", \"add\" or \"clear\"");
        if (!p.CanAnimate) throw new BridgeError(e.PlugIn.Name + "'s " + p.Name + " cannot be keyframed");
        if (mode == "clear") {
            p.IsAnimated = false;
            if (a.Has("value")) SetOfxValue(p, a.Raw("value"), false, null);
        } else {
            List<Args> keys = a.Items("keyframes");
            if (keys.Count == 0) throw new BridgeError("keyframes is empty; mode \"clear\" removes them all");
            Values.CheckTimes(keys, "keyframe", ChainTimeLimit(a), ChainTimeOrigin(a));
            Values.SortByTime(keys);
            if (mode == "replace") {
                // Animating starts from one keyframe at 0 holding the static
                // value, so that value becomes the first keyframe's.
                p.IsAnimated = false;
                SetOfxValue(p, keys[0].Raw("value"), false, null);
            }
            // In "add" a parameter not animated yet keeps its static value at 0.
            p.IsAnimated = true;
            foreach (Args k in keys) {
                double t = k.Num("time");
                if (!k.Has("value")) throw new BridgeError("the keyframe at " + t + " s has no value");
                SetOfxValue(p, k.Raw("value"), true, Tc(t));
                if (k.Has("interpolation")) {
                    OFXKeyframe made = KeyframeAt(p, t);
                    if (made == null) throw new BridgeError("VEGAS made no keyframe at " + t + " s");
                    made.Interpolation = Values.Enum<OFXInterpolationType>(k.Str("interpolation"), "interpolation");
                }
            }
        }
        e.OFXEffect.AllParametersChanged();
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["effect_index"] = e.Index;
        r["name"] = e.PlugIn.Name;
        r["times_from"] = ChainTimeOrigin(a);
        r["param"] = ParamKeyframes(p);
        return r;
    }
}

}
