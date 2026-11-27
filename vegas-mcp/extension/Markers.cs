// Markers, regions and command markers: get_markers and set_markers. Times
// are timeline seconds. VEGAS keeps each list in time order, so a marker's
// index changes when one is added before it or it moves past another;
// set_markers therefore reads every index from the lists as they were
// before the call (as get_markers gave them), whatever its earlier ops did.
using System;
using System.Collections.Generic;
using System.Reflection;
using ScriptPortal.Vegas;

namespace DodStudio.VegasBridge {

public partial class Commands {

    object GetMarkers() {
        Project p = P;
        Dictionary<string, object> r = new Dictionary<string, object>();
        List<object> markers = new List<object>();
        foreach (Marker m in p.Markers) markers.Add(MarkerInfo(m));
        r["markers"] = markers;
        List<object> regions = new List<object>();
        foreach (Region m in p.Regions) regions.Add(MarkerInfo(m));
        r["regions"] = regions;
        List<object> commands = new List<object>();
        foreach (CommandMarker m in p.CommandMarkers) commands.Add(MarkerInfo(m));
        r["command_markers"] = commands;
        return r;
    }

    static object MarkerInfo(Marker m) {
        Dictionary<string, object> d = new Dictionary<string, object>();
        d["index"] = m.Index;
        d["time"] = Sec(m.Position);
        Region r = m as Region;
        if (r != null) { d["length"] = Sec(r.Length); d["end"] = Sec(r.End); }
        CommandMarker c = m as CommandMarker;
        if (c != null) {
            d["type"] = c.CommandType.ToString();
            d["parameter"] = c.CommandParameter;
            d["comment"] = c.Comment;
        } else {
            d["label"] = m.Label;
        }
        return d;
    }

    // ops, applied in order: {op: add|move|rename|delete, kind:
    // marker|region|command (default marker), index (for move, rename and
    // delete: from the lists before this call), time, length (regions),
    // label, and for command markers type (e.g. TEXT, URL), parameter, comment}.
    object SetMarkers(Args a) {
        Project p = P;
        Dictionary<string, List<Marker>> before = new Dictionary<string, List<Marker>>();
        before["marker"] = new List<Marker>();
        foreach (Marker m in p.Markers) before["marker"].Add(m);
        before["region"] = new List<Marker>();
        foreach (Region m in p.Regions) before["region"].Add(m);
        before["command"] = new List<Marker>();
        foreach (CommandMarker m in p.CommandMarkers) before["command"].Add(m);
        List<Marker> deleted = new List<Marker>();
        List<Args> ops = a.Items("ops");
        if (ops.Count == 0) throw new BridgeError("ops is empty");
        for (int i = 0; i < ops.Count; i++) {
            Args op = ops[i];
            string kind = op.Str("kind", "marker");
            if (!before.ContainsKey(kind)) throw new BridgeError("op " + i + ": kind is \"marker\", \"region\" or \"command\"");
            string verb = op.Str("op");
            if (verb == "add") { AddMarker(p, kind, op); continue; }
            int index = op.Int("index");
            if (index < 0 || index >= before[kind].Count) throw new BridgeError("op " + i + ": no " + kind + " " + index + " (" + before[kind].Count + " before this call)");
            Marker target = before[kind][index];
            if (deleted.Contains(target)) throw new BridgeError("op " + i + ": " + kind + " " + index + " was deleted by an earlier op");
            if (verb == "delete") {
                RemoveMarker(p, kind, target);
                deleted.Add(target);
            } else if (verb == "move") {
                if (!op.Has("time") && !op.Has("length")) throw new BridgeError("op " + i + ": move needs time or length");
                if (op.Has("time")) target.Position = Tc(NonNegative(op.Num("time"), "time"));
                if (op.Has("length")) {
                    Region r = target as Region;
                    if (r == null) throw new BridgeError("op " + i + ": only a region has a length");
                    r.Length = Tc(NonNegative(op.Num("length"), "length"));
                }
            } else if (verb == "rename") {
                SetMarkerText(target, op);
            } else {
                throw new BridgeError("op " + i + ": op is \"add\", \"move\", \"rename\" or \"delete\"");
            }
        }
        return GetMarkers();
    }

    static double NonNegative(double v, string what) {
        if (v < 0) throw new BridgeError(what + " " + v + " is before the start");
        return v;
    }

    static void AddMarker(Project p, string kind, Args op) {
        Timecode at = Tc(NonNegative(op.Num("time"), "time"));
        if (kind == "region") {
            Region r = new Region(at, Tc(NonNegative(op.Num("length"), "length")), op.Str("label", ""));
            p.Regions.Add(r);
        } else if (kind == "command") {
            CommandMarker c = new CommandMarker(at, CommandType(op.Str("type", "TEXT")), op.Str("parameter", ""), op.Str("comment", ""));
            p.CommandMarkers.Add(c);
        } else {
            p.Markers.Add(new Marker(at, op.Str("label", "")));
        }
    }

    static void RemoveMarker(Project p, string kind, Marker m) {
        if (kind == "region") p.Regions.Remove((Region)m);
        else if (kind == "command") p.CommandMarkers.Remove((CommandMarker)m);
        else p.Markers.Remove(m);
    }

    static void SetMarkerText(Marker m, Args op) {
        CommandMarker c = m as CommandMarker;
        if (c == null) {
            m.Label = op.Str("label");
            return;
        }
        // A label given to a command marker would otherwise be dropped silently.
        if (op.Has("label")) throw new BridgeError("a command marker has no label; rename it with type, parameter or comment");
        if (!op.Has("type") && !op.Has("parameter") && !op.Has("comment"))
            throw new BridgeError("renaming a command marker needs type, parameter or comment");
        if (op.Has("type") || op.Has("parameter"))
            c.SetCommand(op.Has("type") ? CommandType(op.Str("type")) : c.CommandType, op.Str("parameter", c.CommandParameter));
        if (op.Has("comment")) c.Comment = op.Str("comment");
    }

    // A command marker's type by the name of one of VEGAS's own (TEXT, URL,
    // TITLE, ...), else as written.
    static MarkerCommandType CommandType(string name) {
        foreach (FieldInfo fi in typeof(MarkerCommandType).GetFields(BindingFlags.Public | BindingFlags.Static)) {
            MarkerCommandType t = fi.GetValue(null) as MarkerCommandType;
            if (t == null) continue;
            if (string.Equals(fi.Name, name, StringComparison.OrdinalIgnoreCase) ||
                string.Equals(t.ToString(), name, StringComparison.OrdinalIgnoreCase)) return t;
        }
        return new MarkerCommandType(name);
    }
}

}
