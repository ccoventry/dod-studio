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
        return PP(track, key, null, start, end);
    }

    // A piece with a nameplate picture (its player's).
    static EntryPoint.Piece PP(int track, string key, string plate, double start, double end)
    {
        EntryPoint.Piece p = new EntryPoint.Piece();
        p.Track = track; p.Key = key; p.Plate = plate; p.Start = start; p.End = end;
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
        CheckWith(name, true, expected, pieces);
    }

    static void CheckWith(string name, bool samePlayer, string expected, params EntryPoint.Piece[] pieces)
    {
        string got = Show(EntryPoint.GroupPieces(new List<EntryPoint.Piece>(pieces), 67, samePlayer)); // 4 frames at 60 fps
        if (got == expected) { Console.WriteLine("ok    " + name); return; }
        failures++;
        Console.WriteLine("FAIL  " + name + ": expected " + expected + ", got " + got);
    }

    // All pieces in one group; counts its same-player joins.
    static void CheckCount(string name, int expected, params EntryPoint.Piece[] pieces)
    {
        List<EntryPoint.Piece> list = new List<EntryPoint.Piece>(pieces);
        List<int> group = new List<int>();
        for (int i = 0; i < list.Count; i++) group.Add(i);
        int got = EntryPoint.SamePlayerJoins(list, group);
        if (got == expected) { Console.WriteLine("ok    " + name); return; }
        failures++;
        Console.WriteLine("FAIL  " + name + ": expected " + expected + ", got " + got);
    }

    static void CheckBool(string name, bool expected, bool got)
    {
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

        // Same player (same nameplate picture), different media files (#1440).
        Check("two clips of one player back to back join", "[0,1]",
            PP(1, "a", "x", 0, 1000), PP(1, "b", "x", 1000, 2000));
        Check("same player with a gap don't join", "[0] [1]",
            PP(1, "a", "x", 0, 1000), PP(1, "b", "x", 1100, 2000));
        Check("same player with another player between don't join", "[0] [1] [2]",
            PP(1, "a", "x", 0, 1000), PP(1, "b", "y", 1000, 2000), PP(1, "c", "x", 2000, 3000));
        Check("a split clip then another clip of the same player is one run", "[0,1,2]",
            PP(1, "a", "x", 0, 1000), PP(1, "a", "x", 1000, 3000), PP(1, "b", "x", 3000, 4000));
        Check("a clip with no nameplate never joins by player", "[0] [1]",
            PP(1, "a", null, 0, 1000), PP(1, "b", null, 1000, 2000));
        Check("a clip with no nameplate between breaks the run", "[0] [1] [2]",
            PP(1, "a", "x", 0, 1000), PP(1, "b", null, 1000, 2000), PP(1, "c", "x", 2000, 3000));
        Check("same player on two tracks stays two runs", "[0] [1]",
            PP(1, "a", "x", 0, 1000), PP(2, "b", "x", 1000, 2000));
        Check("same player crossfading joins", "[0,1]",
            PP(1, "a", "x", 0, 1000), PP(1, "b", "x", 900, 2000));
        CheckWith("same player joining off keeps one nameplate per clip", false, "[0] [1]",
            PP(1, "a", "x", 0, 1000), PP(1, "b", "x", 1000, 2000));
        CheckWith("same player joining off still joins pieces of one clip", false, "[0,1]",
            PP(1, "a", "x", 0, 1000), PP(1, "a", "x", 1000, 2000));

        CheckCount("a, a, b: one same-player join", 1, P(1, "a", 0, 1000), P(1, "a", 1000, 2000), P(1, "b", 2000, 3000));
        CheckCount("pieces of one clip: no same-player join", 0, P(1, "a", 0, 1000), P(1, "a", 1000, 2000));
        CheckCount("a, b, c: two same-player joins", 2, P(1, "a", 0, 1000), P(1, "b", 1000, 2000), P(1, "c", 2000, 3000));
        CheckCount("a single piece: none", 0, P(1, "a", 0, 1000));

        CheckBool("a nameplate over a joined run covers its middle clip", true, EntryPoint.Covers(0, 3000, 1000, 2000));
        CheckBool("an exact span is covered", true, EntryPoint.Covers(1000, 2000, 1000, 2000));
        CheckBool("a longer run is not covered by a shorter nameplate", false, EntryPoint.Covers(1000, 2000, 1000, 3000));
        CheckBool("a run starting before the nameplate is not covered", false, EntryPoint.Covers(1000, 3000, 500, 2000));

        Console.WriteLine(failures == 0 ? "all passed" : failures + " failed");
        return failures == 0 ? 0 : 1;
    }
}
