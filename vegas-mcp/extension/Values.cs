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

    // Exactly `count` numbers from a JSON list, e.g. a point [x, y].
    public static double[] Numbers(object raw, int count, string what) {
        IList l = raw as IList;
        if (l == null || l.Count != count) throw new BridgeError(what + " must be a list of " + count + " numbers");
        double[] r = new double[count];
        for (int i = 0; i < count; i++) r[i] = Convert.ToDouble(l[i]);
        return r;
    }

    // 3 or 4 numbers, 0 to 1: a colour as [r, g, b] or [r, g, b, a].
    public static double[] Colour(object raw, string what) {
        IList l = raw as IList;
        if (l == null || (l.Count != 3 && l.Count != 4)) throw new BridgeError(what + " must be [r, g, b] or [r, g, b, a], each 0 to 1");
        double[] r = new double[] { 0, 0, 0, 1 };
        for (int i = 0; i < l.Count; i++) r[i] = Convert.ToDouble(l[i]);
        return r;
    }

    // VEGAS keeps angles in radians; the bridge speaks degrees, as VEGAS's own windows do.
    public static double Degrees(double radians) { return Round(radians * 180.0 / Math.PI); }
    public static double Radians(double degrees) { return degrees * Math.PI / 180.0; }

    // VEGAS stores geometry as floats: 0.1 comes back as 0.100000001490116.
    public static double Round(double v) { return Math.Round(v, 6); }

    public static double[] Point(double x, double y) { return new double[] { Round(x), Round(y) }; }
}

}
