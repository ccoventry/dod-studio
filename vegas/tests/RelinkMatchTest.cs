/**
 * Tests for the relink script's matching (EntryPoint.PlanFor and the list
 * and clip-list handling around it, #1514), which need no VEGAS to run. Built
 * and run by vegas\tests\run.ps1 with the script, against VEGAS's own
 * ScriptPortal.Vegas.dll (the script's other code needs it to compile); exits
 * non-zero when a case fails.
 */

using System;
using System.Collections.Generic;

public static class RelinkMatchTest
{
    static int failures;

    static void Check(string name, object expected, object got)
    {
        if (Equals(expected, got)) { Console.WriteLine("ok    " + name); return; }
        failures++;
        Console.WriteLine("FAIL  " + name + ": expected " + expected + ", got " + got);
    }

    static EntryPoint.RelinkList ListOf(string json)
    {
        EntryPoint.RelinkList list = new EntryPoint.RelinkList();
        EntryPoint.AddMoves(EntryPoint.ParseMoves(json), list);
        return list;
    }

    static readonly Predicate<string> AllThere = delegate(string p) { return true; };
    static readonly Predicate<string> NoneThere = delegate(string p) { return false; };

    const string MAP = @"{
  ""format"": ""dodstudio-relink-map"",
  ""version"": 1,
  ""moves"": [
    { ""from"": ""E:\\Unsorted\\dyelife_row11.mov"", ""to"": ""H:\\Armory\\dyelife.mov"", ""why"": ""moved"" },
    { ""from"": ""D:/clips/old_take.mov"", ""to"": ""D:\\clips\\old_take_2.mov"", ""why"": ""re-rendered"", ""seconds_from"": 4.2, ""seconds_to"": 5.0 },
    { ""from"": ""C:\\a.mov"", ""to"": ""C:\\b.mov"", ""why"": ""moved"" },
    { ""from"": ""C:\\b.mov"", ""to"": ""C:\\c.mov"", ""why"": ""moved"" },
    { ""from"": ""C:\\loop1.mov"", ""to"": ""C:\\loop2.mov"" },
    { ""from"": ""C:\\loop2.mov"", ""to"": ""C:\\loop1.mov"" },
    { ""from"": """", ""to"": ""C:\\nothing.mov"" },
    { ""note"": ""a line from a newer Studio with no from or to"" }
  ],
  ""added_later"": true
}";

    public static int Main()
    {
        EntryPoint.RelinkList list = ListOf(MAP);
        Check("lines without a from and a to are skipped", 6, list.ByFrom.Count);

        EntryPoint.Plan plan = EntryPoint.PlanFor(@"E:\Unsorted\dyelife_row11.mov", list, AllThere);
        Check("a listed file is swapped", EntryPoint.Outcome.Swap, plan.Outcome);
        Check("for its new file", @"H:\Armory\dyelife.mov", plan.To);

        plan = EntryPoint.PlanFor(@"e:/UNSORTED/Dyelife_Row11.MOV", list, AllThere);
        Check("case and slashes don't matter", EntryPoint.Outcome.Swap, plan.Outcome);

        plan = EntryPoint.PlanFor(@"D:\clips\old_take.mov", list, AllThere);
        Check("a forward-slash from matches a backslash path", EntryPoint.Outcome.Swap, plan.Outcome);
        Check("a re-render keeps its lengths", 4.2, plan.Move.SecondsFrom);
        Check("re-render lengths 4.2 and 5.0 differ", true, EntryPoint.LengthDiffers(plan.Move.SecondsFrom, plan.Move.SecondsTo));

        plan = EntryPoint.PlanFor(@"E:\Unsorted\dyelife_row11.mov", list, NoneThere);
        Check("a new file that isn't there is reported, not swapped", EntryPoint.Outcome.TargetMissing, plan.Outcome);

        plan = EntryPoint.PlanFor(@"H:\Armory\dyelife.mov", list, AllThere);
        Check("a media already at a new file is already right", EntryPoint.Outcome.AlreadyRight, plan.Outcome);

        plan = EntryPoint.PlanFor(@"F:\music\song.wav", list, AllThere);
        Check("a file not on the list is left alone", EntryPoint.Outcome.NotListed, plan.Outcome);

        plan = EntryPoint.PlanFor(@"C:\a.mov", list, AllThere);
        Check("a file moved twice ends at the last place", @"C:\c.mov", plan.To);

        plan = EntryPoint.PlanFor(@"C:\loop1.mov", list, AllThere);
        Check("a loop of moves stops instead of spinning", EntryPoint.Outcome.Swap, plan.Outcome);
        Check("one step round the loop", @"C:\loop2.mov", plan.To);

        Check("equal lengths don't differ", false, EntryPoint.LengthDiffers(4.2, 4.22));
        Check("an unknown length doesn't differ", false, EntryPoint.LengthDiffers(double.NaN, 5.0));

        bool threw = false;
        try { EntryPoint.ParseMoves(@"{ ""format"": ""dodstudio-clip-manifest"", ""clips"": [] }"); }
        catch (FormatException) { threw = true; }
        Check("a clip list is not a relink list", true, threw);

        // A later list wins for the same from.
        EntryPoint.AddMoves(EntryPoint.ParseMoves(@"{ ""format"": ""dodstudio-relink-map"", ""moves"": [ { ""from"": ""C:\\a.mov"", ""to"": ""C:\\z.mov"" } ] }"), list);
        Check("a later list wins", @"C:\z.mov", EntryPoint.PlanFor(@"C:\a.mov", list, AllThere).To);

        // The clip list for the nameplate script follows the moves.
        string clipList = @"{
  ""format"": ""dodstudio-clip-manifest"",
  ""version"": 1,
  ""edl"": ""dod_highlights.txt"",
  ""clips"": [
    { ""name"": ""row11"", ""path"": ""E:\\Unsorted\\dyelife_row11.mov"", ""seconds"": 4.5, ""player"": { ""steam_id"": ""1"", ""name"": ""dye \""life\"""" }, ""nameplate"": null },
    { ""name"": ""other"", ""path"": ""F:\\kept.mov"", ""seconds"": 3, ""hud"": true, ""player"": null, ""nameplate"": ""D:\\plates\\x.png"" }
  ]
}";
        Dictionary<string, object> root = EntryPoint.Json.Parse(clipList) as Dictionary<string, object>;
        Check("one clip of the clip list moved", 1, EntryPoint.RelinkClipList(root, ListOf(MAP)));
        string written = EntryPoint.Json.Write(root);
        Dictionary<string, object> back = EntryPoint.Json.Parse(written) as Dictionary<string, object>;
        List<object> clips = back["clips"] as List<object>;
        Check("its path is the new file", @"H:\Armory\dyelife.mov", (clips[0] as Dictionary<string, object>)["path"]);
        Check("the other clip is untouched", @"F:\kept.mov", (clips[1] as Dictionary<string, object>)["path"]);
        Check("quotes in a name survive the write", "dye \"life\"", ((clips[0] as Dictionary<string, object>)["player"] as Dictionary<string, object>)["name"]);
        Check("numbers, true and null survive the write", "4.5|True|null",
            (clips[0] as Dictionary<string, object>)["seconds"] + "|" + (clips[1] as Dictionary<string, object>)["hud"] + "|" + ((clips[0] as Dictionary<string, object>)["nameplate"] ?? "null"));
        Check("keys keep their order", "format", new List<string>(back.Keys)[0]);
        Check("a list with a byte-order mark still reads", 1, ListOf("\uFEFF" + @"{ ""format"": ""dodstudio-relink-map"", ""moves"": [ { ""from"": ""C:\\x.mov"", ""to"": ""C:\\y.mov"" } ] }").ByFrom.Count);

        Console.WriteLine(failures == 0 ? "all passed" : failures + " failed");
        return failures == 0 ? 0 : 1;
    }
}
