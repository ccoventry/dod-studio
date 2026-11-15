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
 * A clip cut into several back-to-back events (slowed or sped up in VEGAS)
 * gets one nameplate over the whole run, not one per piece.
 *
 * Run it from Tools > Scripting > Run Script... with the clips on the
 * timeline. With some clips selected it does only those (File > Import > EDL
 * leaves the first imported clip selected: use Edit > Select None first); with
 * none selected, every clip. It looks for clip lists in the clips' own folders (and the
 * folder above each), and asks for one when it finds none that fits.
 *
 * It never saves the project. Everything it adds is one undo step
 * (Edit > Undo "DoD Studio nameplates", or Ctrl+Z). Running it again reuses
 * the "Nameplates" track and adds only the clips that have no nameplate over
 * their exact start and length yet.
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

    // Events of one clip that touch within this many frames (or overlap, like
    // a crossfade) share one nameplate: a clip cut into pieces and slowed
    // down in VEGAS is still one clip.
    const int JOIN_FRAMES = 4;

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
        // A HUD-only render laid over its clip: ours, but it takes no nameplate.
        public bool Hud;
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
        bool selectedOnly;
        List<VideoEvent> clips = ClipsToDo(project, out selectedOnly);
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
        int hudPasses = 0;
        foreach (VideoEvent clip in clips)
        {
            ClipEntry entry = Find(manifests, MediaPath(clip));
            if (entry == null) notListed++;
            else if (entry.Hud) hudPasses++;
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

        // A clip split into back-to-back events (cut, ctrl-dragged to slow
        // motion) gets one nameplate over the whole run, not one per piece.
        List<Run> runs = RunsToDo(clips, placed);
        int pieces = 0;
        foreach (Run run in runs) pieces += run.Events.Count;

        // A run that already has a nameplate over its exact span is left alone.
        VideoTrack existing = FindTrack(project);
        List<Run> todo = new List<Run>();
        int alreadyThere = 0;
        foreach (Run run in runs)
        {
            if (existing != null && HasNameplate(existing, run.Start, run.Length)) alreadyThere++;
            else todo.Add(run);
        }

        int added = 0;
        if (todo.Count > 0)
        {
            using (UndoBlock undo = new UndoBlock(project, UNDO_NAME))
            {
                VideoTrack track = existing;
                if (track == null)
                {
                    track = new VideoTrack(project, TopTrackIndex(project), TRACK_NAME);
                    project.Tracks.Add(track);
                }
                foreach (Run run in todo)
                {
                    if (AddNameplate(project, track, run.Start, run.Length, matched[run.Events[0]])) added++;
                }
                // No picture opened: leave no empty track behind.
                if (existing == null && added == 0) project.Tracks.Remove(track);
            }
        }

        Say(Summary(added, todo.Count - added, alreadyThere, pieces, runs.Count, notListed, hudPasses, noNameplate, missingFiles, manifests, selectedOnly, clips.Count));
    }

    /// The first video track named TRACK_NAME, or null.
    static VideoTrack FindTrack(Project project)
    {
        foreach (Track track in project.Tracks)
        {
            if (track.IsVideo() && track.Name == TRACK_NAME) return (VideoTrack)track;
        }
        return null;
    }

    /// True when `track` has an event over exactly this start and length.
    static bool HasNameplate(VideoTrack track, Timecode start, Timecode length)
    {
        foreach (TrackEvent ev in track.Events)
        {
            if (Math.Abs(ev.Start.ToMilliseconds() - start.ToMilliseconds()) < 0.5
                && Math.Abs(ev.Length.ToMilliseconds() - length.ToMilliseconds()) < 0.5) return true;
        }
        return false;
    }

    /// One nameplate's worth of events: one clip, or several pieces of it.
    class Run
    {
        public List<VideoEvent> Events = new List<VideoEvent>();
        public Timecode Start;
        public Timecode Length;
    }

    /// Joins the pieces of a split clip (see GroupPieces) among `clips`, and
    /// keeps the runs whose events are in `placed`. Events of other media
    /// (a HUD pass, a clip not in any list) are in the sequence and so break
    /// a run; they just get no nameplate.
    static List<Run> RunsToDo(List<VideoEvent> clips, List<VideoEvent> placed)
    {
        List<Piece> pieces = new List<Piece>();
        foreach (VideoEvent clip in clips)
        {
            Piece piece = new Piece();
            piece.Track = clip.Track.Index;
            piece.Key = PathKey(MediaPath(clip));
            piece.Start = clip.Start.ToMilliseconds();
            piece.End = clip.End.ToMilliseconds();
            pieces.Add(piece);
        }
        double frame = 1000.0 / Math.Max(1.0, clips[0].Project.Video.FrameRate);
        List<Run> runs = new List<Run>();
        foreach (List<int> group in GroupPieces(pieces, JOIN_FRAMES * frame))
        {
            if (!placed.Contains(clips[group[0]])) continue;
            Run run = new Run();
            Timecode end = clips[group[0]].End;
            foreach (int i in group)
            {
                run.Events.Add(clips[i]);
                if (clips[i].End > end) end = clips[i].End;
            }
            run.Start = run.Events[0].Start;
            run.Length = end - run.Start;
            runs.Add(run);
        }
        return runs;
    }

    /// An event reduced to what the joining rule looks at.
    internal class Piece
    {
        public int Track;
        public string Key;
        public double Start;
        public double End;
    }

    /// Groups of indexes into `pieces`, each group one clip's back-to-back
    /// events. Per track, in start order, a piece joins the group before it
    /// when it is the same media (`Key`: speed and length don't matter) and
    /// starts no later than `toleranceMs` after the group's end (an overlap,
    /// like a crossfade, joins too). Anything between them on the track, or a
    /// real gap, starts a new group.
    internal static List<List<int>> GroupPieces(List<Piece> pieces, double toleranceMs)
    {
        List<int> order = new List<int>();
        for (int i = 0; i < pieces.Count; i++) order.Add(i);
        order.Sort(delegate(int a, int b)
        {
            int c = pieces[a].Track.CompareTo(pieces[b].Track);
            if (c == 0) c = pieces[a].Start.CompareTo(pieces[b].Start);
            return c == 0 ? a.CompareTo(b) : c;
        });
        List<List<int>> groups = new List<List<int>>();
        List<int> current = null;
        double currentEnd = 0;
        foreach (int i in order)
        {
            Piece p = pieces[i];
            Piece last = current == null ? null : pieces[current[current.Count - 1]];
            bool joins = last != null && last.Track == p.Track && last.Key == p.Key
                && p.Start <= currentEnd + toleranceMs;
            if (!joins)
            {
                current = new List<int>();
                groups.Add(current);
                currentEnd = p.End;
            }
            else currentEnd = Math.Max(currentEnd, p.End);
            current.Add(i);
        }
        return groups;
    }

    /// The selected video clips, or every video clip when none is selected.
    /// The nameplate track's own stills are never clips. `selectedOnly` says
    /// which of the two it returned.
    static List<VideoEvent> ClipsToDo(Project project, out bool selectedOnly)
    {
        List<VideoEvent> all = new List<VideoEvent>();
        List<VideoEvent> selected = new List<VideoEvent>();
        foreach (Track track in project.Tracks)
        {
            if (!track.IsVideo() || track.Name == TRACK_NAME) continue;
            foreach (TrackEvent ev in track.Events)
            {
                VideoEvent video = ev as VideoEvent;
                if (video == null || MediaPath(video) == null) continue;
                all.Add(video);
                if (video.Selected) selected.Add(video);
            }
        }
        selectedOnly = selected.Count > 0;
        return selectedOnly ? selected : all;
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
                entry.Hud = Get(clip, "hud") is bool && (bool)Get(clip, "hud");
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

    /// The index of the topmost video track other than the nameplate track:
    /// the new track goes there, which puts it above every clip and above any
    /// HUD pass laid over one, so a HUD pass never covers a nameplate.
    static int TopTrackIndex(Project project)
    {
        int top = int.MaxValue;
        foreach (Track track in project.Tracks)
        {
            if (track.IsVideo() && track.Name != TRACK_NAME) top = Math.Min(top, track.Index);
        }
        return top == int.MaxValue ? 0 : top;
    }

    /// The nameplate as a still on `track`, over `clip`'s span, faded and
    /// placed in its corner. False when VEGAS can't open the picture.
    static bool AddNameplate(Project project, VideoTrack track, Timecode start, Timecode clipLength, ClipEntry entry)
    {
        Media media = project.MediaPool.Find(entry.Nameplate);
        if (media == null) media = Media.CreateInstance(project, entry.Nameplate);
        VideoStream picture = media == null ? null : media.GetVideoStreamByIndex(0);
        if (picture == null) return false;

        VideoEvent plate = track.AddVideoEvent(start, clipLength);
        plate.AddTake(picture);
        if (!string.IsNullOrEmpty(entry.Player)) plate.Name = entry.Player;
        double length = clipLength.ToMilliseconds() / 1000.0;
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

    static string Summary(int added, int unopened, int alreadyThere, int pieces, int runCount, int notListed, int hudPasses, int noNameplate, List<string> missingFiles, Manifests manifests, bool selectedOnly, int clipCount)
    {
        StringBuilder text = new StringBuilder();
        if (added > 0) text.AppendLine("Added " + Count(added, "nameplate") + " on the \"" + TRACK_NAME + "\" track. Ctrl+Z takes them all away again.");
        else text.AppendLine("No nameplates were added.");
        if (pieces > runCount) text.AppendLine(Count(runCount, "nameplate") + " for " + Count(pieces, "event") + ": " + Count(pieces - runCount, "split piece") + " joined into the clip before " + (pieces - runCount == 1 ? "it" : "them") + ".");
        if (alreadyThere > 0) text.AppendLine(Count(alreadyThere, "clip") + " already had a nameplate over its exact start and length, so " + (alreadyThere == 1 ? "it was" : "they were") + " left alone.");
        text.AppendLine();
        if (selectedOnly) text.AppendLine("Worked on the " + Count(clipCount, "selected clip") + " only. With nothing selected (Edit > Select None) it does every clip.");
        else text.AppendLine("Nothing was selected, so it worked on every clip (" + clipCount + ").");
        if (manifests.Read.Count == 0) text.AppendLine("No DoD Studio clip list was found. Save one with Render > Export VEGAS EDL... in DoD Studio.");
        else text.AppendLine("Read " + Count(manifests.Read.Count, "clip list") + ": " + Names(manifests.Read) + ".");
        if (notListed > 0) text.AppendLine(Count(notListed, "clip") + " on the timeline " + (notListed == 1 ? "isn't" : "aren't") + " in any clip list.");
        if (hudPasses > 0) text.AppendLine(Count(hudPasses, "HUD pass") + " on the timeline " + (hudPasses == 1 ? "was" : "were") + " left alone: the clip under " + (hudPasses == 1 ? "it carries" : "them carries") + " the nameplate.");
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
