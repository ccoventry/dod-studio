/**
 * DoD Studio Relink Clips (#1514), a VEGAS Pro 21 script.
 *
 * Points the clips of a VEGAS project you are already editing at their new
 * files after they were renamed or moved, and keeps the edit:
 * every event stays where it is, with its trims, speed, fades, effects and
 * Pan/Crop, and the nameplates on top stay too.
 *
 * It does what Project Media > right-click > Replace does by hand, for every
 * clip at once: each media file in the project that is on DoD Studio's relink
 * list is swapped for its new file (Media.ReplaceWith), and every event using
 * it follows. Timeline events themselves are never touched.
 *
 * Where each file went comes from the relink list DoD Studio saves (Render tab
 * > Export VEGAS Relink List..., or From a Project Backup...): a file ending in
 * .relink.json. The script looks for one beside the project, then beside the
 * clips' old files, and asks for one when it finds none. How to use it:
 * docs/vegas_relink.md.
 *
 * It also updates the paths in DoD Studio's clip lists (.dodstudio.json, for
 * the nameplate script) it finds beside the relink list, the project and the
 * new files, keeping a copy of each as it was (.before-relink).
 *
 * It never saves the project: check the clips, then File > Save As a new .veg.
 * Every swap is one undo step (Edit > Undo "DoD Studio relink clips", or
 * Ctrl+Z); the clip lists' copies put those back.
 */

using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using System.Windows.Forms;
using ScriptPortal.Vegas;

public class EntryPoint
{
    // Two lengths further apart than this, in seconds (about 3 frames at 60
    // fps), are named in the end box: a new file that starts or ends
    // somewhere else no longer fits the edit's trims.
    const double LENGTH_TOLERANCE_SECONDS = 0.05;

    // How many file names a line of the end box lists before "and N more".
    const int NAMES_SHOWN = 12;

    const string MAP_SUFFIX = ".relink.json";
    const string MAP_FORMAT = "dodstudio-relink-map";
    const string MANIFEST_SUFFIX = ".dodstudio.json";
    const string MANIFEST_FORMAT = "dodstudio-clip-manifest";
    const string BACKUP_SUFFIX = ".before-relink";
    const string UNDO_NAME = "DoD Studio relink clips";
    const string TITLE = "DoD Studio Relink Clips";

    /// One line of the relink list.
    internal class Move
    {
        public string From;
        public string To;
        public string Why;
    }

    /// What to do with one media file of the project.
    internal enum Outcome { Swap, AlreadyRight, TargetMissing, NotListed }

    internal class Plan
    {
        public Outcome Outcome;
        // The move it follows (Swap, TargetMissing), its `To` the file at the
        // end of any chain of moves.
        public Move Move;
        public string To;
    }

    /// The relink lists read: each move by its `from`, and every `to`.
    internal class RelinkList
    {
        public Dictionary<string, Move> ByFrom = new Dictionary<string, Move>();
        // A set: Dictionary, since HashSet needs System.Core, which VEGAS
        // may not hand the script compiler.
        public Dictionary<string, bool> Targets = new Dictionary<string, bool>();
        public List<string> Read = new List<string>();
        public List<string> Unreadable = new List<string>();
    }

    public void FromVegas(Vegas vegas)
    {
        Project project = vegas.Project;
        List<Media> media = ProjectMedia(project);
        if (media.Count == 0)
        {
            Say("This project has no media files.", vegas);
            return;
        }

        RelinkList list = new RelinkList();
        foreach (string file in ListsNear(project, media)) ReadList(file, list);
        if (list.Read.Count == 0 || CountListed(media, list) == 0)
        {
            string[] picked = AskForLists(project);
            if (picked == null) return;
            foreach (string file in picked) ReadList(file, list);
        }

        List<string> missing = new List<string>();
        List<string> otherLength = new List<string>();
        List<string> failed = new List<string>();
        int swapped = 0;
        int alreadyRight = 0;
        int notListed = 0;
        int leftUnused = 0;
        Predicate<string> exists = delegate(string path) { return File.Exists(path); };

        using (UndoBlock undo = new UndoBlock(project, UNDO_NAME))
        {
            foreach (Media old in media)
            {
                Plan plan = PlanFor(old.FilePath, list, exists);
                if (plan.Outcome == Outcome.NotListed) { notListed++; continue; }
                if (plan.Outcome == Outcome.AlreadyRight) { alreadyRight++; continue; }
                if (plan.Outcome == Outcome.TargetMissing) { missing.Add(Path.GetFileName(plan.To)); continue; }
                try
                {
                    Media fresh = project.MediaPool.Find(plan.To);
                    if (fresh == null) fresh = Media.CreateInstance(project, plan.To);
                    if (fresh == null) throw new InvalidOperationException("VEGAS couldn't open it");
                    double before = SecondsOf(old);
                    old.ReplaceWith(fresh);
                    swapped++;
                    if (StaysUnused(old)) leftUnused++;
                    double after = SecondsOf(fresh);
                    if (LengthDiffers(before, after))
                    {
                        otherLength.Add(Path.GetFileName(plan.To) + LengthNote(before, after));
                    }
                }
                catch (Exception e)
                {
                    failed.Add(Path.GetFileName(old.FilePath) + " (" + e.Message + ")");
                }
            }
        }

        List<string> clipLists = new List<string>();
        List<string> clipListsFailed = new List<string>();
        if (list.ByFrom.Count > 0) UpdateClipLists(ClipListsNear(project, list), list, clipLists, clipListsFailed);

        Say(Summary(swapped, alreadyRight, notListed, leftUnused, missing, otherLength, failed, list, clipLists, clipListsFailed), vegas);
    }

    /// The project's media files: every pool entry that is a file, not a
    /// generated media (titles, solid colours). Collected first, so swapping
    /// never changes the pool while it is being walked.
    static List<Media> ProjectMedia(Project project)
    {
        List<Media> all = new List<Media>();
        foreach (object item in project.MediaPool)
        {
            Media m = item as Media;
            if (m == null && item is DictionaryEntry) m = ((DictionaryEntry)item).Value as Media;
            if (m == null || m.IsGenerated() || string.IsNullOrEmpty(m.FilePath)) continue;
            if (!all.Contains(m)) all.Add(m);
        }
        return all;
    }

    /// What to do with the media at `mediaPath`: swap it (the list maps it,
    /// following a chain of moves to its end, and that file is there), leave
    /// it (it already is a listed new file), say its new file is missing, or
    /// leave it because the list doesn't name it.
    internal static Plan PlanFor(string mediaPath, RelinkList list, Predicate<string> exists)
    {
        Plan plan = new Plan();
        string key = PathKey(mediaPath);
        Move move;
        if (!list.ByFrom.TryGetValue(key, out move))
        {
            plan.Outcome = list.Targets.ContainsKey(key) ? Outcome.AlreadyRight : Outcome.NotListed;
            return plan;
        }
        // A file moved twice (B to C after A to B) ends at C. A loop stops
        // before it would come back round.
        string to = move.To;
        Dictionary<string, bool> seen = new Dictionary<string, bool>();
        seen[key] = true;
        Move next;
        while (list.ByFrom.TryGetValue(PathKey(to), out next) && !seen.ContainsKey(PathKey(next.To)))
        {
            seen[PathKey(to)] = true;
            to = next.To;
        }
        plan.Move = move;
        plan.To = to;
        if (PathKey(to) == key) plan.Outcome = Outcome.AlreadyRight;
        else plan.Outcome = exists(to) ? Outcome.Swap : Outcome.TargetMissing;
        return plan;
    }

    /// True when both lengths are known and further apart than the tolerance.
    internal static bool LengthDiffers(double a, double b)
    {
        return !double.IsNaN(a) && !double.IsNaN(b) && a > 0 && b > 0 && Math.Abs(a - b) > LENGTH_TOLERANCE_SECONDS;
    }

    /// True when the swapped-out entry is still in Project Media with no
    /// clip using it. Read guarded: whether ReplaceWith keeps the old entry
    /// at all is VEGAS's business.
    static bool StaysUnused(Media old)
    {
        try { return old.UseCount == 0; } catch (Exception) { return false; }
    }

    static double SecondsOf(Media m)
    {
        try { return m.Length.ToMilliseconds() / 1000.0; } catch (Exception) { return double.NaN; }
    }

    static string LengthNote(double before, double after)
    {
        return " (" + before.ToString("0.00", CultureInfo.InvariantCulture) + " s, now " + after.ToString("0.00", CultureInfo.InvariantCulture) + " s)";
    }

    /// The relink lists beside the project and beside each media file's (old)
    /// folder and the one above it.
    static List<string> ListsNear(Project project, List<Media> media)
    {
        List<string> folders = new List<string>();
        AddFolder(folders, SafeFolder(project.FilePath));
        foreach (Media m in media)
        {
            string folder = SafeFolder(m.FilePath);
            AddFolder(folders, folder);
            AddFolder(folders, SafeFolder(folder));
        }
        return FilesIn(folders, "*" + MAP_SUFFIX);
    }

    /// The clip lists beside the relink lists read, the project, and the new
    /// files (and the folder above each).
    static List<string> ClipListsNear(Project project, RelinkList list)
    {
        List<string> folders = new List<string>();
        foreach (string file in list.Read) AddFolder(folders, SafeFolder(file));
        AddFolder(folders, SafeFolder(project.FilePath));
        foreach (Move move in list.ByFrom.Values)
        {
            string folder = SafeFolder(move.To);
            AddFolder(folders, folder);
            AddFolder(folders, SafeFolder(folder));
        }
        return FilesIn(folders, "*" + MANIFEST_SUFFIX);
    }

    static void AddFolder(List<string> folders, string folder)
    {
        if (!string.IsNullOrEmpty(folder) && !folders.Contains(folder)) folders.Add(folder);
    }

    static List<string> FilesIn(List<string> folders, string pattern)
    {
        List<string> files = new List<string>();
        foreach (string folder in folders)
        {
            try
            {
                if (!Directory.Exists(folder)) continue;
                foreach (string file in Directory.GetFiles(folder, pattern)) if (!files.Contains(file)) files.Add(file);
            }
            catch (Exception)
            {
                // A folder that can't be listed (gone, no access) has nothing to offer.
            }
        }
        return files;
    }

    static string SafeFolder(string path)
    {
        try { return string.IsNullOrEmpty(path) ? null : Path.GetDirectoryName(path); }
        catch (Exception) { return null; }
    }

    /// Asks for the relink list, starting in the project's folder. Null on Cancel.
    static string[] AskForLists(Project project)
    {
        using (OpenFileDialog dialog = new OpenFileDialog())
        {
            dialog.Title = "Pick DoD Studio's relink list (Render > Export VEGAS Relink List...)";
            dialog.Filter = "DoD Studio relink list (*" + MAP_SUFFIX + ")|*" + MAP_SUFFIX + "|JSON (*.json)|*.json|All files (*.*)|*.*";
            dialog.Multiselect = true;
            string folder = SafeFolder(project.FilePath);
            if (!string.IsNullOrEmpty(folder) && Directory.Exists(folder)) dialog.InitialDirectory = folder;
            return dialog.ShowDialog() == DialogResult.OK ? dialog.FileNames : null;
        }
    }

    static int CountListed(List<Media> media, RelinkList list)
    {
        int n = 0;
        foreach (Media m in media)
        {
            string key = PathKey(m.FilePath);
            if (list.ByFrom.ContainsKey(key) || list.Targets.ContainsKey(key)) n++;
        }
        return n;
    }

    /// Reads one relink list into `into`; a file that isn't one is noted, not fatal.
    static void ReadList(string file, RelinkList into)
    {
        if (into.Read.Contains(file) || into.Unreadable.Contains(file)) return;
        try
        {
            AddMoves(ParseMoves(File.ReadAllText(file, Encoding.UTF8)), into);
            into.Read.Add(file);
        }
        catch (Exception e)
        {
            into.Unreadable.Add(Path.GetFileName(file) + " (" + e.Message + ")");
        }
    }

    /// The moves of a relink list's text. Throws on text that isn't one.
    internal static List<Move> ParseMoves(string text)
    {
        Dictionary<string, object> root = Json.Parse(text) as Dictionary<string, object>;
        if (root == null || Str(root, "format") != MAP_FORMAT) throw new FormatException("not a DoD Studio relink list");
        List<object> items = Get(root, "moves") as List<object>;
        if (items == null) throw new FormatException("no moves");
        List<Move> moves = new List<Move>();
        foreach (object item in items)
        {
            Dictionary<string, object> obj = item as Dictionary<string, object>;
            if (obj == null || string.IsNullOrEmpty(Str(obj, "from")) || string.IsNullOrEmpty(Str(obj, "to"))) continue;
            Move move = new Move();
            move.From = Str(obj, "from");
            move.To = Str(obj, "to");
            move.Why = Str(obj, "why") ?? "moved";
            moves.Add(move);
        }
        return moves;
    }

    /// Adds `moves` to `into`: a later list wins for the same `from`, as the
    /// newer export of it.
    internal static void AddMoves(List<Move> moves, RelinkList into)
    {
        foreach (Move move in moves)
        {
            into.ByFrom[PathKey(move.From)] = move;
            into.Targets[PathKey(move.To)] = true;
        }
    }

    /// Rewrites each clip list in `files` whose clips name a moved file, after
    /// keeping a copy of it as it was beside it.
    static void UpdateClipLists(List<string> files, RelinkList list, List<string> updated, List<string> failedFiles)
    {
        foreach (string file in files)
        {
            try
            {
                string text = File.ReadAllText(file, Encoding.UTF8);
                Dictionary<string, object> root = Json.Parse(text) as Dictionary<string, object>;
                if (root == null || Str(root, "format") != MANIFEST_FORMAT) continue;
                int changed = RelinkClipList(root, list);
                if (changed == 0) continue;
                File.WriteAllText(FreeBackupName(file), text, new UTF8Encoding(false));
                File.WriteAllText(file, Json.Write(root) + "\n", new UTF8Encoding(false));
                updated.Add(Path.GetFileName(file) + " (" + changed + ")");
            }
            catch (Exception e)
            {
                failedFiles.Add(Path.GetFileName(file) + " (" + e.Message + ")");
            }
        }
    }

    /// Points each clip of a parsed clip list whose `path` is a moved file at
    /// its new one (following chains, as PlanFor does). Returns how many changed.
    internal static int RelinkClipList(Dictionary<string, object> root, RelinkList list)
    {
        List<object> clips = Get(root, "clips") as List<object>;
        if (clips == null) return 0;
        int changed = 0;
        foreach (object item in clips)
        {
            Dictionary<string, object> clip = item as Dictionary<string, object>;
            string path = clip == null ? null : Str(clip, "path");
            if (string.IsNullOrEmpty(path)) continue;
            Plan plan = PlanFor(path, list, delegate(string p) { return true; });
            if (plan.Outcome != Outcome.Swap) continue;
            clip["path"] = plan.To;
            changed++;
        }
        return changed;
    }

    /// `file` + BACKUP_SUFFIX, or with -2, -3... when that is taken: an
    /// earlier copy is never overwritten.
    static string FreeBackupName(string file)
    {
        string name = file + BACKUP_SUFFIX;
        for (int n = 2; File.Exists(name); n++) name = file + BACKUP_SUFFIX + "-" + n;
        return name;
    }

    internal static string PathKey(string path)
    {
        string p = (path ?? "").Replace('/', '\\');
        try { p = Path.GetFullPath(p); } catch (Exception) { /* keep it as written */ }
        return p.ToLowerInvariant();
    }

    static string Summary(int swapped, int alreadyRight, int notListed, int leftUnused, List<string> missing, List<string> otherLength, List<string> failed, RelinkList list, List<string> clipLists, List<string> clipListsFailed)
    {
        StringBuilder text = new StringBuilder();
        if (swapped > 0) text.AppendLine("Swapped " + Count(swapped, "media file") + " for " + (swapped == 1 ? "its" : "their") + " new file. Every clip using " + (swapped == 1 ? "it" : "them") + " keeps its place and edits. Ctrl+Z undoes it all.");
        else text.AppendLine("No media files were swapped.");
        if (alreadyRight > 0) text.AppendLine(Count(alreadyRight, "media file") + " already pointed at the new file.");
        if (missing.Count > 0) text.AppendLine(Count(missing.Count, "new file") + (missing.Count == 1 ? " isn't" : " aren't") + " there, so " + (missing.Count == 1 ? "its clip was" : "their clips were") + " left as they were: " + Names(missing) + ". Run Find Moved Videos in DoD Studio and export the list again.");
        if (otherLength.Count > 0) text.AppendLine(Count(otherLength.Count, "new file") + " " + (otherLength.Count == 1 ? "has" : "have") + " a different length from the old one, so check " + (otherLength.Count == 1 ? "its" : "their") + " trims: " + Names(otherLength) + ".");
        if (failed.Count > 0) text.AppendLine(Count(failed.Count, "media file") + " couldn't be swapped: " + Names(failed) + ".");
        if (leftUnused > 0) text.AppendLine(Count(leftUnused, "old entry") + " stay in Project Media unused: Project Media > Remove All Unused Media clears them.");
        if (notListed > 0) text.AppendLine(Count(notListed, "other media file") + " in the project " + (notListed == 1 ? "isn't" : "aren't") + " on the relink list and " + (notListed == 1 ? "was" : "were") + " left alone.");
        text.AppendLine();
        if (list.Read.Count > 0) text.AppendLine("Read " + Count(list.Read.Count, "relink list") + ": " + FileNames(list.Read) + ".");
        if (list.Unreadable.Count > 0) text.AppendLine("Couldn't read: " + string.Join(", ", list.Unreadable.ToArray()));
        if (clipLists.Count > 0) text.AppendLine("Updated the paths in " + Count(clipLists.Count, "clip list") + " for the nameplate script (the old one kept as " + BACKUP_SUFFIX + "): " + string.Join(", ", clipLists.ToArray()) + ".");
        if (clipListsFailed.Count > 0) text.AppendLine("Couldn't update: " + string.Join(", ", clipListsFailed.ToArray()));
        text.AppendLine("The project isn't saved: check it, then File > Save As a new .veg.");
        return text.ToString().TrimEnd();
    }

    static string Count(int n, string what)
    {
        if (n == 1) return n + " " + what;
        return n + " " + (what.EndsWith("y") ? what.Substring(0, what.Length - 1) + "ies" : what + "s");
    }

    static string Names(List<string> names)
    {
        if (names.Count <= NAMES_SHOWN) return string.Join(", ", names.ToArray());
        return string.Join(", ", names.GetRange(0, NAMES_SHOWN).ToArray()) + " and " + (names.Count - NAMES_SHOWN) + " more";
    }

    static string FileNames(List<string> files)
    {
        List<string> names = new List<string>();
        foreach (string f in files) names.Add(Path.GetFileName(f));
        return string.Join(", ", names.ToArray());
    }

    static void Say(string text, Vegas vegas)
    {
        // The VEGAS build, so a report from an older one can be told apart.
        string version = "";
        try { version = "\n\nVEGAS " + vegas.Version; } catch (Exception) { /* not worth failing the box over */ }
        MessageBox.Show(text + version, TITLE, MessageBoxButtons.OK, MessageBoxIcon.Information);
    }

    static object Get(Dictionary<string, object> obj, string key)
    {
        object value;
        return obj.TryGetValue(key, out value) ? value : null;
    }

    static string Str(Dictionary<string, object> obj, string key)
    {
        return Get(obj, key) as string;
    }

    /// Just enough JSON for the relink list and the clip list: objects,
    /// arrays, strings, numbers, true, false and null, read and written.
    /// VEGAS compiles scripts without a JSON library.
    internal static class Json
    {
        public static object Parse(string text)
        {
            int at = 0;
            // A byte-order mark is not JSON, but an editor may have added one.
            if (text.Length > 0 && text[0] == '﻿') at = 1;
            object value = ReadValue(text, ref at);
            SkipSpace(text, ref at);
            if (at != text.Length) throw new FormatException("extra text at " + at);
            return value;
        }

        /// `value` as indented JSON, objects' keys in the order they were read.
        public static string Write(object value)
        {
            StringBuilder text = new StringBuilder();
            WriteValue(text, value, 0);
            return text.ToString();
        }

        static void WriteValue(StringBuilder text, object value, int depth)
        {
            if (value == null) { text.Append("null"); return; }
            if (value is bool) { text.Append((bool)value ? "true" : "false"); return; }
            if (value is double) { text.Append(((double)value).ToString("R", CultureInfo.InvariantCulture)); return; }
            string s = value as string;
            if (s != null) { WriteString(text, s); return; }
            Dictionary<string, object> obj = value as Dictionary<string, object>;
            if (obj != null)
            {
                if (obj.Count == 0) { text.Append("{}"); return; }
                text.Append("{\n");
                int i = 0;
                foreach (KeyValuePair<string, object> pair in obj)
                {
                    Indent(text, depth + 1);
                    WriteString(text, pair.Key);
                    text.Append(": ");
                    WriteValue(text, pair.Value, depth + 1);
                    text.Append(++i < obj.Count ? ",\n" : "\n");
                }
                Indent(text, depth);
                text.Append('}');
                return;
            }
            List<object> list = value as List<object>;
            if (list != null)
            {
                if (list.Count == 0) { text.Append("[]"); return; }
                text.Append("[\n");
                for (int i = 0; i < list.Count; i++)
                {
                    Indent(text, depth + 1);
                    WriteValue(text, list[i], depth + 1);
                    text.Append(i + 1 < list.Count ? ",\n" : "\n");
                }
                Indent(text, depth);
                text.Append(']');
                return;
            }
            throw new FormatException("can't write a " + value.GetType().Name);
        }

        static void Indent(StringBuilder text, int depth)
        {
            text.Append(' ', depth * 2);
        }

        static void WriteString(StringBuilder text, string s)
        {
            text.Append('"');
            foreach (char c in s)
            {
                switch (c)
                {
                    case '"': text.Append("\\\""); break;
                    case '\\': text.Append("\\\\"); break;
                    case '\n': text.Append("\\n"); break;
                    case '\r': text.Append("\\r"); break;
                    case '\t': text.Append("\\t"); break;
                    case '\b': text.Append("\\b"); break;
                    case '\f': text.Append("\\f"); break;
                    default:
                        if (c < ' ') text.Append("\\u").Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                        else text.Append(c);
                        break;
                }
            }
            text.Append('"');
        }

        static void SkipSpace(string s, ref int at)
        {
            while (at < s.Length && char.IsWhiteSpace(s[at])) at++;
        }

        static object ReadValue(string s, ref int at)
        {
            SkipSpace(s, ref at);
            if (at >= s.Length) throw new FormatException("unexpected end");
            char c = s[at];
            if (c == '{') return ReadObject(s, ref at);
            if (c == '[') return ReadArray(s, ref at);
            if (c == '"') return ReadString(s, ref at);
            if (Word(s, ref at, "true")) return true;
            if (Word(s, ref at, "false")) return false;
            if (Word(s, ref at, "null")) return null;
            return ReadNumber(s, ref at);
        }

        static bool Word(string s, ref int at, string word)
        {
            if (string.CompareOrdinal(s, at, word, 0, word.Length) != 0) return false;
            at += word.Length;
            return true;
        }

        static Dictionary<string, object> ReadObject(string s, ref int at)
        {
            Dictionary<string, object> obj = new Dictionary<string, object>();
            at++;
            SkipSpace(s, ref at);
            if (at < s.Length && s[at] == '}') { at++; return obj; }
            while (true)
            {
                SkipSpace(s, ref at);
                if (at >= s.Length || s[at] != '"') throw new FormatException("expected a name at " + at);
                string key = ReadString(s, ref at);
                SkipSpace(s, ref at);
                if (at >= s.Length || s[at] != ':') throw new FormatException("expected ':' at " + at);
                at++;
                obj[key] = ReadValue(s, ref at);
                SkipSpace(s, ref at);
                if (at < s.Length && s[at] == ',') { at++; continue; }
                if (at < s.Length && s[at] == '}') { at++; return obj; }
                throw new FormatException("expected ',' or '}' at " + at);
            }
        }

        static List<object> ReadArray(string s, ref int at)
        {
            List<object> list = new List<object>();
            at++;
            SkipSpace(s, ref at);
            if (at < s.Length && s[at] == ']') { at++; return list; }
            while (true)
            {
                list.Add(ReadValue(s, ref at));
                SkipSpace(s, ref at);
                if (at < s.Length && s[at] == ',') { at++; continue; }
                if (at < s.Length && s[at] == ']') { at++; return list; }
                throw new FormatException("expected ',' or ']' at " + at);
            }
        }

        static string ReadString(string s, ref int at)
        {
            StringBuilder text = new StringBuilder();
            at++;
            while (at < s.Length)
            {
                char c = s[at++];
                if (c == '"') return text.ToString();
                if (c != '\\') { text.Append(c); continue; }
                if (at >= s.Length) break;
                char e = s[at++];
                switch (e)
                {
                    case 'n': text.Append('\n'); break;
                    case 'r': text.Append('\r'); break;
                    case 't': text.Append('\t'); break;
                    case 'b': text.Append('\b'); break;
                    case 'f': text.Append('\f'); break;
                    case 'u':
                        if (at + 4 > s.Length) throw new FormatException("bad \\u escape");
                        text.Append((char)int.Parse(s.Substring(at, 4), NumberStyles.HexNumber, CultureInfo.InvariantCulture));
                        at += 4;
                        break;
                    default: text.Append(e); break; // \" \\ \/
                }
            }
            throw new FormatException("unterminated string");
        }

        static double ReadNumber(string s, ref int at)
        {
            int start = at;
            while (at < s.Length && "+-0123456789.eE".IndexOf(s[at]) >= 0) at++;
            double value;
            if (at == start || !double.TryParse(s.Substring(start, at - start), NumberStyles.Float, CultureInfo.InvariantCulture, out value))
            {
                throw new FormatException("bad value at " + start);
            }
            return value;
        }
    }
}
