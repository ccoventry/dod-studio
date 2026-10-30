// VEGAS script for #1048: builds a throwaway 60 fps project with a 300 fps
// clip at normal speed, 5x slow and 2.5x slow, puts Frame Blend on the events
// (mode "event"), on the media (mode "media"), or on the media with the slowed
// events cut from a subclip (mode "subclip"), renders 0.25 s of each, then
// discards the project and quits. The plugin's own log records what VEGAS
// passes it. Run through run_vegas_probe.ps1, which sets the variables below.
using System;
using System.IO;
using System.Text;
using ScriptPortal.Vegas;

public class EntryPoint {
    StreamWriter log;

    void L(string s) { log.WriteLine(DateTime.Now.ToString("HH:mm:ss.fff") + " " + s); log.Flush(); }

    PlugInNode FindByName(PlugInNode node, string name) {
        foreach (PlugInNode c in node) {
            if (c.Name != null && c.Name.Contains(name)) return c;
            PlugInNode r = FindByName(c, name);
            if (r != null) return r;
        }
        return null;
    }

    VideoEvent AddEvent(Vegas vegas, VideoTrack track, Media media, PlugInNode fx, double at, double len,
                        double offset, double rate, VideoResampleMode mode, string label) {
        VideoEvent ev = track.AddVideoEvent(Timecode.FromSeconds(at), Timecode.FromSeconds(len));
        Take take = ev.AddTake(media.GetVideoStreamByIndex(0));
        take.Offset = Timecode.FromSeconds(offset);
        ev.PlaybackRate = rate;
        ev.ResampleMode = mode;
        Effect ef = fx != null ? ev.Effects.AddEffect(fx) : null;
        L(label + ": timeline " + at + "-" + (at + len) + " s, media offset " + offset + " s, rate " + rate +
          ", resample " + mode + ", effect added: " + (ef != null) + ", OFX: " + (ef != null && ef.OFXEffect != null));
        if (ef != null && ef.OFXEffect != null) {
            foreach (OFXParameter p in ef.OFXEffect.Parameters) L("  param " + p.Name + " (" + p.GetType().Name + ")");
        }
        return ev;
    }

    public void FromVegas(Vegas vegas) {
        string outDir = Environment.GetEnvironmentVariable("FRAME_BLEND_PROBE_OUT");
        string clip = Environment.GetEnvironmentVariable("FRAME_BLEND_PROBE_CLIP");
        string pixel = Environment.GetEnvironmentVariable("FRAME_BLEND_PROBE_PIXEL");
        log = new StreamWriter(Path.Combine(outDir, "script.log"), true);
        try {
            L("script start; project before: " + (vegas.Project != null ? vegas.Project.FilePath : "(none)"));
            vegas.NewProject(false, false);
            Project p = vegas.Project;
            p.Video.Width = 2560;
            p.Video.Height = 1440;
            p.Video.FrameRate = Environment.GetEnvironmentVariable("FRAME_BLEND_PROBE_MODE") == "proj300" ? 300.0 : 60.0;
            p.Video.FieldOrder = VideoFieldOrder.ProgressiveScan;
            if (pixel == "float") p.Video.PixelFormat = PixelFormat.Float32BitFullRange;
            L("project: " + p.Video.Width + "x" + p.Video.Height + " @ " + p.Video.FrameRate + " fps, " + p.Video.PixelFormat);

            Media media = Media.CreateInstance(p, clip);
            VideoStream vs = media.GetVideoStreamByIndex(0);
            L("media: " + clip + " frame rate " + vs.FrameRate + ", length " + media.Length);

            PlugInNode fx = vegas.VideoFX.FindChildByUniqueID("{Svfx:com.dodstudio.FrameBlend}");
            L("FindChildByUniqueID: " + (fx != null ? fx.Name : "null"));
            if (fx == null) fx = FindByName(vegas.VideoFX, "Frame Blend");
            if (fx == null) { L("Frame Blend not found in Video FX"); return; }
            L("plug-in: '" + fx.Name + "' id " + fx.UniqueID + " OFX " + fx.IsOFX);

            string mode = Environment.GetEnvironmentVariable("FRAME_BLEND_PROBE_MODE");
            if (mode == "media") {
                Effect mf = media.Effects.AddEffect(fx);
                L("media FX added: " + (mf != null) + ", OFX: " + (mf != null && mf.OFXEffect != null));
                fx = null;
            }
            // subclip mode: Frame Blend on the full media only; the slowed
            // events use a subclip of the same file, to see whether a subclip
            // has its own Media FX.
            Media slowMedia = media;
            if (mode == "subclip") {
                Effect mf = media.Effects.AddEffect(fx);
                L("media FX added to the full media: " + (mf != null));
                fx = null;
                slowMedia = new Subclip(p, clip, Timecode.FromSeconds(0), Timecode.FromSeconds(9.9), false, "slow subclip");
                L("subclip: IsSubclip " + slowMedia.IsSubclip() + ", its effects: " + slowMedia.Effects.Count);
            }
            VideoTrack track = p.AddVideoTrack();
            AddEvent(vegas, track, media, fx, 0.0, 1.0, 2.0, 1.0, VideoResampleMode.Disable, "E1 normal speed");
            AddEvent(vegas, track, slowMedia, fx, 1.5, 1.0, 4.0, 0.2, VideoResampleMode.Disable, "E2 5x slow");
            AddEvent(vegas, track, slowMedia, fx, 3.0, 1.0, 5.0, 0.4, VideoResampleMode.Disable, "E3 2.5x slow");
            AddEvent(vegas, track, media, fx, 4.5, 1.0, 2.0, 1.0, VideoResampleMode.Smart, "E4 normal speed, smart resample");
            AddEvent(vegas, track, slowMedia, fx, 6.0, 1.0, 4.0, 0.2, VideoResampleMode.Smart, "E5 5x slow, smart resample");

            RenderTemplate pick = null;
            foreach (Renderer r in vegas.Renderers) {
                foreach (RenderTemplate t in r.Templates) {
                    if (!t.IsValid() || t.VideoStreamCount < 1) continue;
                    L("template: " + r.FileTypeName + " | " + t.Name + " | " + t.VideoWidth + "x" + t.VideoHeight + " @ " + t.VideoFrameRate);
                    bool sixty = Math.Abs(t.VideoFrameRate - 60.0) < 0.001;
                    if (sixty && (pick == null || (t.VideoWidth == 2560 && t.VideoHeight == 1440))) pick = t;
                }
            }
            if (pick == null) { L("no 60 fps template found"); return; }
            L("render template: " + pick.Name + " (" + pick.VideoWidth + "x" + pick.VideoHeight + " @ " + pick.VideoFrameRate + ")");

            // Slowed events first: a hang there should not cost the others.
            string order = Environment.GetEnvironmentVariable("FRAME_BLEND_PROBE_ORDER") ?? "1,2,3,4,5";
            double[] starts = { 0.25, 1.75, 3.25, 4.75, 6.25 };
            foreach (string item in order.Split(',')) {
                int i = int.Parse(item) - 1;
                RenderArgs args = new RenderArgs(p);
                args.OutputFile = Path.Combine(outDir, "E" + (i + 1) + "_" + (pixel ?? "8bit") + "_" + (mode ?? "event") + pick.FileExtensions[0].TrimStart('*'));
                args.RenderTemplate = pick;
                args.Start = Timecode.FromSeconds(starts[i]);
                args.Length = Timecode.FromSeconds(0.25);
                L("render E" + (i + 1) + " " + starts[i] + "+0.25 s -> " + args.OutputFile);
                RenderStatus st = vegas.Render(args);
                L("  status " + st);
            }
        } catch (Exception e) {
            L("exception: " + e);
        } finally {
            try { vegas.NewProject(false, false); L("project discarded"); } catch (Exception e) { L("discard failed: " + e.Message); }
            L("exit");
            log.Close();
            vegas.Exit();
        }
    }
}
