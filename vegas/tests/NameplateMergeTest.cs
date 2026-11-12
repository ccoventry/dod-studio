/**
 * Tests for the nameplate script's joining rule (EntryPoint.GroupPieces),
 * which needs no VEGAS to run. Build and run (from the repo root):
 *
 *   powershell -ExecutionPolicy Bypass -File vegas\tests\run.ps1
 *
 * It compiles this file together with the script against VEGAS's own
 * ScriptPortal.Vegas.dll (the script's other code needs it to compile) and
 * exits non-zero when a case fails.
 */

using System;
using System.Collections.Generic;

public static class NameplateMergeTest
{
    static int failures;

    static EntryPoint.Piece P(int track, string key, double start, double end)
    {
        EntryPoint.Piece p = new EntryPoint.Piece();
        p.Track = track; p.Key = key; p.Start = start; p.End = end;
        return p;
    }

    static string Show(List<List<int>> groups)
    {
        List<string> parts = new List<string>();
        foreach (List<int> g in groups) parts.Add("[" + string.Join(",", g.ConvertAll<string>(delegate(int i) { return i.ToString(); }).ToArray()) + "]");
        return string.Join(" ", parts.ToArray());
    }

    static void Check(string name, string expected, params EntryPoint.Piece[] pieces)
    {
        string got = Show(EntryPoint.GroupPieces(new List<EntryPoint.Piece>(pieces), 67)); // 4 frames at 60 fps
        if (got == expected) { Console.WriteLine("ok    " + name); return; }
        failures++;
        Console.WriteLine("FAIL  " + name + ": expected " + expected + ", got " + got);
    }

    public static int Main()
    {
        Check("one clip stays one", "[0]", P(1, "a", 0, 3000));
        Check("three touching pieces of one clip join", "[0,1,2]",
            P(1, "a", 0, 1000), P(1, "a", 1000, 4000), P(1, "a", 4000, 4500));
        // Speed changes change length and offset only: the key is the media.
        Check("a slowed piece (longer) still joins", "[0,1]", P(1, "a", 0, 1000), P(1, "a", 1000, 9000));
        Check("a gap of a few frames joins", "[0,1]", P(1, "a", 0, 1000), P(1, "a", 1050, 2000));
        Check("a gap past the tolerance splits", "[0] [1]", P(1, "a", 0, 1000), P(1, "a", 1100, 2000));
        Check("a crossfade overlap joins", "[0,1]", P(1, "a", 0, 1000), P(1, "a", 900, 2000));
        Check("another clip in between splits the run", "[0] [1] [2]",
            P(1, "a", 0, 1000), P(1, "b", 1000, 2000), P(1, "a", 2000, 3000));
        Check("back-to-back different clips stay apart", "[0] [1]", P(1, "a", 0, 1000), P(1, "b", 1000, 2000));
        Check("same clip on two tracks is two runs", "[0] [1]", P(1, "a", 0, 1000), P(2, "a", 1000, 2000));
        Check("events out of order are sorted by start", "[1,0]", P(1, "a", 1000, 2000), P(1, "a", 0, 1000));
        Check("a piece inside the earlier one's end joins", "[0,1,2]",
            P(1, "a", 0, 5000), P(1, "a", 1000, 2000), P(1, "a", 5000, 6000));
        Check("nothing", "");

        Console.WriteLine(failures == 0 ? "all passed" : failures + " failed");
        return failures == 0 ? 0 : 1;
    }
}
