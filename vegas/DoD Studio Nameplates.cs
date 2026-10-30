/**
 * DoD Studio Nameplates (#429), a VEGAS Pro 21 script.
 *
 * Puts each player's nameplate picture on its own video track above their
 * highlight clips, lined up with each clip's start and length, so the
 * nameplate can be moved, resized, faded or removed per clip and is drawn at
 * the project's resolution (never upscaled with the footage).
 *
 * Who each clip is, and which picture, comes from the clip list DoD Studio
 * saves beside its VEGAS EDL (Render tab > Export VEGAS EDL...): a file
 * ending in .dodstudio.json. Pictures are set per player on Configuration >
 * Players > Nameplate. How to use it: docs/vegas_nameplates.md.
 *
 * Run it from Tools > Scripting > Run Script... with the clips on the
 * timeline. With some clips selected it does only those; with none selected,
 * every clip. It looks for clip lists in the clips' own folders (and the
 * folder above each), and asks for one when it finds none that fits.
 *
 * It never saves the project. Everything it adds is one undo step
 * (Edit > Undo "DoD Studio nameplates", or Ctrl+Z). Running it again adds
 * another track: undo the first run, or delete its track, before re-running.
 */

using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using System.Windows.Forms;
using ScriptPortal.Vegas;

public class EntryPoint
{
    // ---- Defaults: change these to taste, then run the script again. ----

    // The nameplate's height, as a share of the frame's height (0.2 = a fifth).
    // Its width follows from the picture's own shape.
    const double NAMEPLATE_HEIGHT = 0.20;

    // The gap between the nameplate and the frame's edges, as a share of the
    // frame's height.
    const double MARGIN = 0.04;

    // Which corner it sits in: "BottomLeft", "BottomRight", "TopLeft" or
    // "TopRight".
    const string CORNER = "BottomLeft";

    // Fade in at the clip's start and out at its end, in seconds; 0 for none.
    // A fade never takes more than a third of a short clip.
    const double FADE_IN_SECONDS = 0.25;
    const double FADE_OUT_SECONDS = 0.25;

    // The new track's name.
    const string TRACK_NAME = "Nameplates";

    // What the clip lists' file names end with, and the undo step's name.
    const string MANIFEST_SUFFIX = ".dodstudio.json";
    const string MANIFEST_FORMAT = "dodstudio-clip-manifest";
    const string UNDO_NAME = "DoD Studio nameplates";
    const string TITLE = "DoD Studio Nameplates";

    // ---------------------------------------------------------------------

    /// One clip of a clip list: its file, its player's name and nameplate.
    class ClipEntry
    {
        public string Path;
        public string Player;
        public string Nameplate;
    }

    /// The clip lists read so far: each clip by its full path, and by its
    /// file name alone (for clips moved since the list was saved).
    class Manifests
    {
        public Dictionary<string, ClipEntry> ByPath = new Dictionary<string, ClipEntry>();
        public Dictionary<string, List<ClipEntry>> ByName = new Dictionary<string, List<ClipEntry>>();
        public List<string> Read = new List<string>();
        public List<string> Unreadable = new List<string>();
    }

    public void FromVegas(Vegas vegas)
    {
        Project project = vegas.Project;
        List<VideoEvent> clips = ClipsToDo(project);
        if (clips.Count == 0)
        {
            Say("There are no video clips on the timeline.");
            return;
        }

        Manifests manifests = new Manifests();
        foreach (string file in ManifestsNear(clips)) ReadManifest(file, manifests);
        if (CountMatches(clips, manifests) == 0)
        {
            string[] picked = AskForManifests(clips);
            if (picked == null) return;
            foreach (string file in picked) ReadManifest(file, manifests);
        }

        Dictionary<VideoEvent, ClipEntry> matched = new Dictionary<VideoEvent, ClipEntry>();
        int notListed = 0;
        foreach (VideoEvent clip in clips)
        {
            ClipEntry entry = Find(manifests, MediaPath(clip));
            if (entry == null) notListed++;
            else matched[clip] = entry;
        }

        List<VideoEvent> placed = new List<VideoEvent>();
        int noNameplate = 0;
        List<string> missingFiles = new List<string>();
        foreach (KeyValuePair<VideoEvent, ClipEntry> pair in matched)
        {
            string nameplate = pair.Value.Nameplate;
            if (string.IsNullOrEmpty(nameplate)) noNameplate++;
            else if (!File.Exists(nameplate)) { if (!missingFiles.Contains(nameplate)) missingFiles.Add(nameplate); }
            else placed.Add(pair.Key);
        }

        int added = 0;
        if (placed.Count > 0)
        {
            using (UndoBlock undo = new UndoBlock(project, UNDO_NAME))
            {
                VideoTrack track = new VideoTrack(project, TopTrackIndex(placed), TRACK_NAME);
                project.Tracks.Add(track);
                foreach (VideoEvent clip in placed)
                {
                    if (AddNameplate(project, track, clip, matched[clip])) added++;
                }
                // No picture opened: leave no empty track behind.
                if (added == 0) project.Tracks.Remove(track);
            }
        }

        Say(Summary(added, placed.Count - added, notListed, noNameplate, missingFiles, manifests));
    }

    /// The selected video clips, or every video clip when none is selected.
    static List<VideoEvent> ClipsToDo(Project project)
    {
        List<VideoEvent> all = new List<VideoEvent>();
        List<VideoEvent> selected = new List<VideoEvent>();
        foreach (Track track in project.Tracks)
        {
            if (!track.IsVideo()) continue;
            foreach (TrackEvent ev in track.Events)
            {
                VideoEvent video = ev as VideoEvent;
                if (video == null || MediaPath(video) == null) continue;
                all.Add(video);
                if (video.Selected) selected.Add(video);
            }
        }
        return selected.Count > 0 ? selected : all;
    }

    /// The file an event plays, or null for generated media and empty events.
    static string MediaPath(VideoEvent ev)
    {
        Take take = ev.ActiveTake;
        if (take == null || take.Media == null || take.Media.IsGenerated()) return null;
        string path = take.Media.FilePath;
        return string.IsNullOrEmpty(path) ? null : path;
    }

    /// The clip lists in each clip's folder and the folder above it.
    static List<string> ManifestsNear(List<VideoEvent> clips)
    {
        List<string> folders = new List<string>();
        foreach (VideoEvent clip in clips)
        {
            string folder = SafeFolder(MediaPath(clip));
            for (int up = 0; up < 2 && !string.IsNullOrEmpty(folder); up++)
            {
                if (!folders.Contains(folder)) folders.Add(folder);
                folder = SafeFolder(folder);
            }
        }
        List<string> files = new List<string>();
        foreach (string folder in folders)
        {
            try
            {
                foreach (string file in Directory.GetFiles(folder, "*" + MANIFEST_SUFFIX)) files.Add(file);
            }
            catch (Exception)
            {
                // A folder that can't be listed (gone, no access) has no list to offer.
            }
        }
        return files;
    }

    static string SafeFolder(string path)
    {
        try { return string.IsNullOrEmpty(path) ? null : Path.GetDirectoryName(path); }
        catch (Exception) { return null; }
    }

    /// Asks for the clip lists, starting in the first clip's folder. Null on Cancel.
    static string[] AskForManifests(List<VideoEvent> clips)
    {
        using (OpenFileDialog dialog = new OpenFileDialog())
        {
            dialog.Title = "Pick DoD Studio's clip list (saved beside the EDL)";
            dialog.Filter = "DoD Studio clip list (*" + MANIFEST_SUFFIX + ")|*" + MANIFEST_SUFFIX + "|All files (*.*)|*.*";
            dialog.Multiselect = true;
            string folder = SafeFolder(MediaPath(clips[0]));
            if (!string.IsNullOrEmpty(folder) && Directory.Exists(folder)) dialog.InitialDirectory = folder;
            return dialog.ShowDialog() == DialogResult.OK ? dialog.FileNames : null;
        }
    }

    /// Reads one clip list into `into`; a file that isn't one is noted, not fatal.
    static void ReadManifest(string file, Manifests into)
    {
        if (into.Read.Contains(file) || into.Unreadable.Contains(file)) return;
        try
        {
            Dictionary<string, object> root = Json.Parse(File.ReadAllText(file, Encoding.UTF8)) as Dictionary<string, object>;
            if (root == null || Str(root, "format") != MANIFEST_FORMAT) throw new FormatException("not a DoD Studio clip list");
            List<object> list = Get(root, "clips") as List<object>;
            if (list == null) throw new FormatException("no clips");
            foreach (object item in list)
            {
                Dictionary<string, object> clip = item as Dictionary<string, object>;
                if (clip == null || string.IsNullOrEmpty(Str(clip, "path"))) continue;
                Dictionary<string, object> player = Get(clip, "player") as Dictionary<string, object>;
                ClipEntry entry = new ClipEntry();
                entry.Path = Str(clip, "path");
                entry.Player = player != null ? Str(player, "name") : null;
                entry.Nameplate = Str(clip, "nameplate");
                // A later list wins: it is the newer export of the same clip.
                into.ByPath[PathKey(entry.Path)] = entry;
                string name = NameKey(entry.Path);
                if (!into.ByName.ContainsKey(name)) into.ByName[name] = new List<ClipEntry>();
                into.ByName[name].Add(entry);
            }
            into.Read.Add(file);
        }
        catch (Exception e)
        {
            into.Unreadable.Add(Path.GetFileName(file) + " (" + e.Message + ")");
        }
    }

    /// The entry for a clip's file: by its full path, else by its file name
    /// when exactly one listed clip has that name.
    static ClipEntry Find(Manifests manifests, string mediaPath)
    {
        ClipEntry entry;
        if (manifests.ByPath.TryGetValue(PathKey(mediaPath), out entry)) return entry;
        List<ClipEntry> sameName;
        if (manifests.ByName.TryGetValue(NameKey(mediaPath), out sameName))
        {
            Dictionary<string, ClipEntry> distinct = new Dictionary<string, ClipEntry>();
            foreach (ClipEntry e in sameName) distinct[PathKey(e.Path)] = e;
            if (distinct.Count == 1) foreach (ClipEntry e in distinct.Values) return e;
        }
        return null;
    }

    static int CountMatches(List<VideoEvent> clips, Manifests manifests)
    {
        int n = 0;
        foreach (VideoEvent clip in clips) if (Find(manifests, MediaPath(clip)) != null) n++;
        return n;
    }

    static string PathKey(string path)
    {
        string p = (path ?? "").Replace('/', Path.DirectorySeparatorChar);
        try { p = Path.GetFullPath(p); } catch (Exception) { /* keep it as written */ }
        return p.ToLowerInvariant();
    }

    static string NameKey(string path)
    {
        string p = (path ?? "").Replace('/', Path.DirectorySeparatorChar);
        try { return Path.GetFileName(p).ToLowerInvariant(); } catch (Exception) { return p.ToLowerInvariant(); }
    }

    /// The index of the topmost track holding one of `clips`: the new track
    /// goes there, which puts it above them all.
    static int TopTrackIndex(List<VideoEvent> clips)
    {
        int top = int.MaxValue;
        foreach (VideoEvent clip in clips) top = Math.Min(top, clip.Track.Index);
        return top == int.MaxValue ? 0 : top;
    }

    /// The nameplate as a still on `track`, over `clip`'s span, faded and
    /// placed in its corner. False when VEGAS can't open the picture.
    static bool AddNameplate(Project project, VideoTrack track, VideoEvent clip, ClipEntry entry)
    {
        Media media = project.MediaPool.Find(entry.Nameplate);
        if (media == null) media = Media.CreateInstance(project, entry.Nameplate);
        VideoStream picture = media == null ? null : media.GetVideoStreamByIndex(0);
        if (picture == null) return false;

        VideoEvent plate = track.AddVideoEvent(clip.Start, clip.Length);
        plate.AddTake(picture);
        if (!string.IsNullOrEmpty(entry.Player)) plate.Name = entry.Player;
        double length = clip.Length.ToMilliseconds() / 1000.0;
        double most = length / 3.0;
        if (FADE_IN_SECONDS > 0) plate.FadeIn.Length = Timecode.FromSeconds(Math.Min(FADE_IN_SECONDS, most));
        if (FADE_OUT_SECONDS > 0) plate.FadeOut.Length = Timecode.FromSeconds(Math.Min(FADE_OUT_SECONDS, most));
        Place(project, plate, picture.Width, picture.Height);
        return true;
    }

    /// Sets the event's Pan/Crop so the picture shows NAMEPLATE_HEIGHT of the
    /// frame tall, MARGIN in from CORNER, with the rest of the frame clear.
    ///
    /// Pan/Crop works backwards: its box is the part of the picture, in the
    /// picture's own pixels, that fills the whole frame. So a box larger than
    /// the picture shrinks it, and moving the box moves the picture the other
    /// way. The box keeps the frame's shape, so nothing is stretched.
    static void Place(Project project, VideoEvent plate, int pictureWidth, int pictureHeight)
    {
        if (pictureWidth <= 0 || pictureHeight <= 0 || NAMEPLATE_HEIGHT <= 0) return;
        double frameWidth = project.Video.Width * project.Video.PixelAspectRatio;
        double frameHeight = project.Video.Height;
        double boxHeight = pictureHeight / NAMEPLATE_HEIGHT;
        double boxWidth = boxHeight * frameWidth / frameHeight;
        // Frame pixels per picture pixel, so the margin can be given in the
        // picture's pixels.
        double scale = frameHeight / boxHeight;
        double margin = MARGIN * frameHeight / scale;

        bool right = CORNER.EndsWith("Right", StringComparison.OrdinalIgnoreCase);
        bool top = CORNER.StartsWith("Top", StringComparison.OrdinalIgnoreCase);
        double left = right ? pictureWidth - boxWidth + margin : -margin;
        double upper = top ? -margin : pictureHeight - boxHeight + margin;

        VideoMotionBounds bounds = new VideoMotionBounds(
            new VideoMotionVertex((float)left, (float)upper),
            new VideoMotionVertex((float)(left + boxWidth), (float)upper),
            new VideoMotionVertex((float)(left + boxWidth), (float)(upper + boxHeight)),
            new VideoMotionVertex((float)left, (float)(upper + boxHeight)));
        foreach (VideoMotionKeyframe keyframe in plate.VideoMotion.Keyframes)
        {
            keyframe.Bounds = bounds;
        }
    }

    static string Summary(int added, int unopened, int notListed, int noNameplate, List<string> missingFiles, Manifests manifests)
    {
        StringBuilder text = new StringBuilder();
        if (added > 0) text.AppendLine("Added " + Count(added, "nameplate") + " on the \"" + TRACK_NAME + "\" track. Ctrl+Z takes them all away again.");
        else text.AppendLine("No nameplates were added.");
        text.AppendLine();
        if (manifests.Read.Count == 0) text.AppendLine("No DoD Studio clip list was found. Save one with Render > Export VEGAS EDL... in DoD Studio.");
        else text.AppendLine("Read " + Count(manifests.Read.Count, "clip list") + ": " + Names(manifests.Read) + ".");
        if (notListed > 0) text.AppendLine(Count(notListed, "clip") + " on the timeline " + (notListed == 1 ? "isn't" : "aren't") + " in any clip list.");
        if (noNameplate > 0) text.AppendLine(Count(noNameplate, "clip") + " had no nameplate: the player has none on DoD Studio's Players tab, or the clip isn't one player's.");
        if (missingFiles.Count > 0) text.AppendLine("These nameplate files aren't there any more: " + string.Join(", ", missingFiles.ToArray()));
        if (unopened > 0) text.AppendLine(Count(unopened, "nameplate") + " couldn't be opened as a picture by VEGAS.");
        if (manifests.Unreadable.Count > 0) text.AppendLine("Couldn't read: " + string.Join(", ", manifests.Unreadable.ToArray()));
        return text.ToString().TrimEnd();
    }

    static string Count(int n, string what) { return n + " " + what + (n == 1 ? "" : "s"); }

    static string Names(List<string> files)
    {
        List<string> names = new List<string>();
        foreach (string f in files) names.Add(Path.GetFileName(f));
        return string.Join(", ", names.ToArray());
    }

    static void Say(string text)
    {
        MessageBox.Show(text, TITLE, MessageBoxButtons.OK, MessageBoxIcon.Information);
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

    /// Just enough JSON for the clip list: objects, arrays, strings, numbers,
    /// true, false and null. VEGAS compiles scripts without a JSON library.
    static class Json
    {
        public static object Parse(string text)
        {
            int at = 0;
            object value = ReadValue(text, ref at);
            SkipSpace(text, ref at);
            if (at != text.Length) throw new FormatException("extra text at " + at);
            return value;
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
