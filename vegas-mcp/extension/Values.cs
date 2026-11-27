// Small conversions the project-detail commands share: enum names from
// arguments, numbers and points out of JSON lists, and angles. Each area's
// own conversions stay in its file.
using System;
using System.Collections;
using System.Collections.Generic;

namespace DodStudio.VegasBridge {

static class Values {
    // Keyframes and points match an existing one within half a millisecond:
    // a Timecode is 100 ns, and a time sent as seconds rarely lands exactly.
    public const double SameTime = 0.0005;

    // An enum value by name, ignoring case; an unknown name fails listing the known ones.
    public static T Enum<T>(string name, string what) {
        foreach (string n in System.Enum.GetNames(typeof(T))) {
            if (string.Equals(n, name, StringComparison.OrdinalIgnoreCase)) return (T)System.Enum.Parse(typeof(T), n);
        }
        throw new BridgeError(what + " \"" + name + "\" is not one of " + string.Join(", ", System.Enum.GetNames(typeof(T))));
    }

    // A finite number from JSON. A string that is not one, a list or an
    // object fails naming `what`, and so do "NaN" and "Infinity", which
    // Convert.ToDouble accepts and which pass every range check.
    public static double Number(object raw, string what) {
        double v;
        try {
            v = Convert.ToDouble(raw, System.Globalization.CultureInfo.InvariantCulture);
        } catch (Exception e) {
            if (!(e is FormatException || e is InvalidCastException || e is OverflowException)) throw;
            throw new BridgeError(what + " must be a number; got " + Describe(raw));
        }
        if (double.IsNaN(v) || double.IsInfinity(v)) throw new BridgeError(what + " must be a finite number; got " + Describe(raw));
        return v;
    }

    static string Describe(object raw) {
        if (raw == null) return "null";
        if (raw is string) return "\"" + raw + "\"";
        if (raw is IList) return "a list";
        if (raw is IDictionary) return "an object";
        return Convert.ToString(raw, System.Globalization.CultureInfo.InvariantCulture);
    }

    // Exactly `count` numbers from a JSON list, e.g. a point [x, y].
    public static double[] Numbers(object raw, int count, string what) {
        IList l = raw as IList;
        if (l == null || l.Count != count) throw new BridgeError(what + " must be a list of " + count + " numbers");
        double[] r = new double[count];
        for (int i = 0; i < count; i++) r[i] = Number(l[i], what);
        return r;
    }

    // 3 or 4 numbers, 0 to 1: a colour as [r, g, b] or [r, g, b, a].
    public static double[] Colour(object raw, string what) {
        IList l = raw as IList;
        if (l == null || (l.Count != 3 && l.Count != 4)) throw new BridgeError(what + " must be [r, g, b] or [r, g, b, a], each 0 to 1");
        double[] r = new double[] { 0, 0, 0, 1 };
        for (int i = 0; i < l.Count; i++) r[i] = Number(l[i], what);
        return r;
    }

    // Every item's `time`, checked before the items are sorted by it (a
    // missing or bad one would otherwise fail inside List.Sort as "Failed to
    // compare two elements"): present, a finite number, not negative, and
    // not past `max` seconds (the event's length for times from its start;
    // PositiveInfinity where nothing bounds it). `what` names an item, e.g.
    // "keyframe"; `origin` what its times count from.
    public static void CheckTimes(List<Args> items, string what, double max, string origin) {
        for (int i = 0; i < items.Count; i++) {
            Args it = items[i];
            if (!it.Has("time")) throw new BridgeError(what + " " + i + " has no time");
            double t = Number(it.Raw("time"), what + " " + i + "'s time");
            if (t < 0) throw new BridgeError(what + " " + i + "'s time " + t + " s is before the " + origin);
            if (t > max + SameTime) throw new BridgeError(what + " " + i + "'s time " + t + " s is past the end (" + Round(max) + " s from the " + origin + ")");
        }
    }

    // Items in time order; CheckTimes has checked each time.
    public static void SortByTime(List<Args> items) {
        items.Sort(delegate(Args x, Args y) { return x.Num("time").CompareTo(y.Num("time")); });
    }

    // VEGAS keeps angles in radians; the bridge speaks degrees, as VEGAS's own windows do.
    public static double Degrees(double radians) { return Round(radians * 180.0 / Math.PI); }
    public static double Radians(double degrees) { return degrees * Math.PI / 180.0; }

    // VEGAS stores geometry as floats: 0.1 comes back as 0.100000001490116.
    public static double Round(double v) { return Math.Round(v, 6); }

    public static double[] Point(double x, double y) { return new double[] { Round(x), Round(y) }; }
}

}
