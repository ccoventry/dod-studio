// Effect presets and the effect catalogue: list_effects (each effect once,
// with the folders VEGAS lists it under, and with `details` its OFX
// parameters and presets), list_presets, read_preset, and the `preset`
// add_effect and set_effect_params take.
//
// A preset is applied by name (Effect.Preset). PlugInNode.Presets lists a
// plug-in's presets, VEGAS's own and the user's saved ones in one list,
// with nothing that tells them apart. An effect's parameters can be read
// only once it sits in a chain (a detached `new Effect(node)` throws on
// every property), so the catalogue and read_preset add a throwaway copy
// in an undo block that is always cancelled, and remove it before the
// block ends: the project is left as it was, unmodified.
//
// Saving a preset is not in VEGAS's scripting API (no member of Effect,
// PlugInNode or EffectPresets writes one), and the store it would go to is
// the one the user's own VEGAS reads, so the bridge never writes presets.
using System;
using System.Collections.Generic;
using ScriptPortal.Vegas;

namespace DodStudio.VegasBridge {

public partial class Commands {

    // Opening every plug-in to read its parameters is slow and some show
    // windows of their own (licence and model-download boxes), so details
    // are for a filtered few.
    const int MaxDetailedEffects = 30;

    PlugInNode PlugInRoot(Args a) {
        string kind = a.Str("kind", "effect");
        if (kind == "effect") return vegas.VideoFX;
        if (kind == "transition") return vegas.Transitions;
        throw new BridgeError("kind is \"effect\" or \"transition\"");
    }

    PlugInNode RequirePlugIn(Args a) {
        string want = a.Str("effect");
        PlugInNode node = FindPlugIn(PlugInRoot(a), want);
        if (node == null)
            throw new BridgeError("no video " + a.Str("kind", "effect") + " named or with id \"" + want + "\" (see list_effects or list_transitions)");
        return node;
    }

    // ---- the catalogue ----

    // VEGAS lists an effect under several folders (Frame Blend under five:
    // its group, OFX, 32-bit floating point, Third Party and the top), each
    // as its own node. One entry per id, with every folder it is under.
    static void CollectPlugIns(PlugInNode node, string folder, string filter,
                               Dictionary<string, Dictionary<string, object>> byId, List<Dictionary<string, object>> into) {
        foreach (PlugInNode c in node) {
            if (c.IsContainer) {
                CollectPlugIns(c, folder.Length > 0 ? folder + "/" + c.Name : c.Name, filter, byId, into);
                continue;
            }
            string id = c.UniqueID ?? c.Name;
            if (filter.Length > 0 && c.Name.IndexOf(filter, StringComparison.OrdinalIgnoreCase) < 0 &&
                id.IndexOf(filter, StringComparison.OrdinalIgnoreCase) < 0) continue;
            Dictionary<string, object> d;
            if (!byId.TryGetValue(id, out d)) {
                d = new Dictionary<string, object>();
                d["name"] = c.Name;
                d["id"] = c.UniqueID;
                d["ofx"] = c.IsOFX;
                if (c.IsPackage) d["package"] = true;
                d["categories"] = new List<string>();
                byId[id] = d;
                into.Add(d);
            }
            // The top of the tree lists every plug-in, so it is no category.
            List<string> categories = (List<string>)d["categories"];
            if (folder.Length > 0 && !categories.Contains(folder)) categories.Add(folder);
        }
    }

    object ListEffects(Args a) {
        string filter = a.Str("filter", "");
        List<Dictionary<string, object>> found = new List<Dictionary<string, object>>();
        CollectPlugIns(vegas.VideoFX, "", filter, new Dictionary<string, Dictionary<string, object>>(StringComparer.OrdinalIgnoreCase), found);
        if (!a.Bool("details", false)) return found;
        if (filter.Length == 0) throw new BridgeError("details needs a filter (part of an effect's name or id)");
        if (found.Count > MaxDetailedEffects)
            throw new BridgeError(found.Count + " effects match \"" + filter + "\"; details lists at most " + MaxDetailedEffects + ": narrow the filter");
        foreach (Dictionary<string, object> d in found) AddDetails(d, FindPlugIn(vegas.VideoFX, (string)d["id"] ?? (string)d["name"]), false);
        return found;
    }

    // An effect's presets and, for OFX, its parameters with their defaults
    // and ranges, read from a throwaway copy.
    void AddDetails(Dictionary<string, object> d, PlugInNode node, bool transition) {
        d["presets"] = PresetNames(node);
        if (!node.IsOFX) {
            d["params_note"] = "not an OFX plug-in: its parameters are not scriptable, only its presets";
            return;
        }
        try {
            d["params"] = WithThrowaway(node, transition, delegate(Effect e) {
                List<object> ps = new List<object>();
                foreach (OFXParameter p in e.OFXEffect.Parameters) if (HoldsValue(p)) ps.Add(ParamCatalogue(p));
                return ps;
            });
        } catch (Exception ex) {
            if (ex is BridgeError) throw;
            d["params_error"] = ex.GetType().Name + ": " + ex.Message;
        }
    }

    static List<string> PresetNames(PlugInNode node) {
        List<string> names = new List<string>();
        foreach (EffectPreset p in node.Presets) names.Add(p.Name);
        return names;
    }

    // A parameter as a fresh effect has it: its value there is the default.
    static Dictionary<string, object> ParamCatalogue(OFXParameter p) {
        Dictionary<string, object> d = new Dictionary<string, object>();
        d["name"] = p.Name;
        d["label"] = p.Label;
        d["type"] = p.ParameterType.ToString();
        object v = ParamValue(p);
        if (v != null) d["default"] = v;
        // OFXRangeParameter (numbers, points) has the range; the others none.
        foreach (string range in new string[] { "Min", "Max", "DisplayMin", "DisplayMax" }) {
            try { d[RangeKey(range)] = OfxJson(Member(p, range)); } catch (BridgeError) { }
        }
        OFXChoiceParameter c = p as OFXChoiceParameter;
        if (c != null) {
            List<string> choices = new List<string>();
            foreach (OFXChoice ch in c.Choices) choices.Add(ch.Name);
            d["choices"] = choices;
        }
        d["can_animate"] = p.CanAnimate;
        if (!string.IsNullOrEmpty(p.Hint)) d["hint"] = p.Hint;
        return d;
    }

    static string RangeKey(string member) {
        if (member == "DisplayMin") return "display_min";
        if (member == "DisplayMax") return "display_max";
        return member.ToLowerInvariant();
    }

    // ---- presets ----

    object ListPresets(Args a) {
        PlugInNode node = RequirePlugIn(a);
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["name"] = node.Name;
        r["id"] = node.UniqueID;
        r["ofx"] = node.IsOFX;
        r["presets"] = PresetNames(node);
        return r;
    }

    // A preset's parameter values, from a throwaway copy of the effect with
    // the preset applied, beside the defaults; `changed` marks what the
    // preset sets away from them.
    object ReadPreset(Args a) {
        PlugInNode node = RequirePlugIn(a);
        string preset = a.Str("preset");
        bool transition = a.Str("kind", "effect") == "transition";
        if (!node.IsOFX)
            throw new BridgeError(node.Name + " is not an OFX plug-in: its presets apply by name, but their values cannot be read");
        return WithThrowaway(node, transition, delegate(Effect e) {
            Dictionary<string, object> defaults = new Dictionary<string, object>();
            foreach (OFXParameter p in e.OFXEffect.Parameters) if (HoldsValue(p)) defaults[p.Name] = ParamValue(p);
            string applied = ApplyPreset(e, preset);
            List<object> ps = new List<object>();
            int changed = 0;
            foreach (OFXParameter p in e.OFXEffect.Parameters) {
                if (!HoldsValue(p)) continue;
                Dictionary<string, object> pd = new Dictionary<string, object>();
                pd["name"] = p.Name;
                pd["label"] = p.Label;
                pd["type"] = p.ParameterType.ToString();
                object v = ParamValue(p);
                pd["value"] = v;
                object def;
                defaults.TryGetValue(p.Name, out def);
                pd["default"] = def;
                bool differs = !SameJson(v, def);
                pd["changed"] = differs;
                if (differs) changed++;
                ps.Add(pd);
            }
            Dictionary<string, object> r = new Dictionary<string, object>();
            r["name"] = node.Name;
            r["id"] = node.UniqueID;
            r["preset"] = applied;
            r["changed_count"] = changed;
            r["params"] = ps;
            return r;
        });
    }

    // Applies a preset by name (any case) and returns its name as VEGAS has
    // it. An unknown name fails listing the presets there are: VEGAS itself
    // takes one silently.
    static string ApplyPreset(Effect e, string preset) {
        foreach (EffectPreset p in e.Presets) {
            if (string.Equals(p.Name, preset, StringComparison.OrdinalIgnoreCase)) {
                e.Preset = p.Name;
                return p.Name;
            }
        }
        List<string> names = new List<string>();
        foreach (EffectPreset p in e.Presets) names.Add(p.Name);
        // Preset names hold commas ("Circle, Center"), so they are listed one per quote.
        throw new BridgeError(e.PlugIn.Name + " has no preset \"" + preset + "\"; its presets: \"" + string.Join("\", \"", names.ToArray()) + "\"");
    }

    // Two OFX values as JSON (numbers, strings, lists of numbers) alike.
    static bool SameJson(object x, object y) {
        if (x == null || y == null) return x == y;
        System.Collections.IList lx = x as System.Collections.IList, ly = y as System.Collections.IList;
        if (lx != null && ly != null) {
            if (lx.Count != ly.Count) return false;
            for (int i = 0; i < lx.Count; i++) if (!SameJson(lx[i], ly[i])) return false;
            return true;
        }
        if (x is string || y is string || x is bool || y is bool) return x.Equals(y);
        return Math.Abs(Convert.ToDouble(x) - Convert.ToDouble(y)) < 1e-9;
    }

    // Runs `read` on a copy of the plug-in that the project never keeps: an
    // effect on the Video Output FX chain, or a transition on the fade-in of
    // an empty event on a track of its own, inside an undo block that is
    // always cancelled, and removed again before the block ends.
    object WithThrowaway(PlugInNode node, bool transition, Func<Effect, object> read) {
        Project p = P;
        UndoBlock block = new UndoBlock(p, "DoD Studio bridge: read a plug-in");
        block.Cancel = true;
        try {
            if (!transition) {
                Effect e = p.VideoBus.Effects.AddEffect(node);
                if (e == null) throw new BridgeError("VEGAS did not add " + node.Name);
                try { return read(e); } finally { p.VideoBus.Effects.Remove(e); }
            }
            VideoTrack t = new VideoTrack(p, p.Tracks.Count, "throwaway");
            p.Tracks.Add(t);
            try {
                VideoEvent ev = t.AddVideoEvent(Tc(0), Tc(1));
                ev.FadeIn.Length = Tc(0.5);
                ev.FadeIn.Transition = new Effect(node);
                Effect live = ev.FadeIn.Transition as Effect;
                if (live == null) throw new BridgeError("VEGAS did not put " + node.Name + " on a fade");
                return read(live);
            } finally {
                p.Tracks.Remove(t);
            }
        } finally {
            block.Dispose();
        }
    }
}

}
