# Frame Blend: a VEGAS Pro plugin for motion blur from 300 fps captures (R&D, #1048)

An OpenFX video effect that averages several captured frames into each output frame. A 300 fps
capture on a 60 fps timeline gets real motion blur (5 frames per output frame, the look of #1039's
`2_normal_blur5_60.mp4`), which VEGAS's own Frame blend resample cannot make: it mixes the two
frames nearest each output frame, and at 300 to 60 fps every output frame lands exactly on one.

R&D prototype: not part of Studio's build, CI or installer. C++ against the plain OpenFX C API;
the eight OFX 1.4 headers it needs are vendored in `third_party/openfx/` (BSD-3, licence there).

## What VEGAS Pro 21 does with it (measured, build 315)

Measured by running VEGAS with a `-SCRIPT:` probe (`tools/vegas_probe.cs`, run by
`tools/run_vegas_probe.ps1`; no UI clicks) on
`m00cat_TEK_halle_Allies_1k_row05_4.mov` (ProRes HQ, 2560x1440, 300 fps): a 60 fps project, events
at normal speed, 5x slow and 2.5x slow, short renders, and the plugin's own log, which hashes every
frame it fetches.

| | As an **Event FX** | As a **Media FX** |
|---|---|---|
| Time the plugin sees | the event's own time, in 60 fps frames | the media's own time, in 300 fps frames |
| Source clip frame rate | 60 (the event, already conformed) | 300 |
| Asking for the frames between two output frames | snaps to whole 60 fps frames: t, t+0.2, t+0.4, t+0.6 all return the same image | every request is a real captured frame |
| Distinct frames per output frame (5 asked) | 2 (a 4:1 double image, not blur) | 5 |
| Result | Frame Blend passes the event through unchanged (it detects this) | the blur works, 8-bit and 32-bit float projects |

- **Temporal clip access works** in VEGAS (the host says so, and every fetch at another time
  returned an image).
- **The event's speed does not reach the plugin.** An Event FX sees the event after the speed change
  but only at whole 60 fps frames; a Media FX sees the media before any event uses it. So
  "follow the event's speed" is not possible from inside the plugin.
- **No admin needed:** VEGAS loads OFX plugins from the folders in the `OFX_PLUGIN_PATH`
  environment variable as well as `C:\Program Files\Common Files\OFX\Plugins`.
- A project at 300 fps is not an escape: VEGAS capped the project at 240 fps, and during a 60 fps
  render it still gave the Event FX a 60 fps time base.
- **One hang:** in one of two identical runs, rendering a 5x-slow event of the clip with Frame Blend
  as Media FX froze VEGAS (no CPU, plugin not called). The rerun, and a 32-bit float run, finished.
  Watch for it.

## How to use it

1. Install it (below) and restart VEGAS. It is under Video FX, group **DoD Studio**, named
   **Frame Blend**.
2. **Project Media**: right-click the 300 fps clip, **Media FX...**, add **Frame Blend**. Frames to
   blend **5**, Capture frame rate **300**.
3. Every normal-speed event from that clip now has the blur. Set the project to 60 fps (not 59.94)
   and the events' resample to **Disable resample**.
4. **Slowed (ctrl-dragged) events:** a Media FX blurs every frame of the clip whatever the event's
   speed, so a 5x slow event would show each captured frame smeared over the next 4. For a slowed
   part, make a **subclip** of the clip (in the Trimmer, select the range, right-click,
   **Create Subclip**) and build the slowed event from the subclip. A subclip has its own Media FX
   (measured: an event from a subclip with none was not touched by the plugin):
   - 5x slow: no Frame Blend on the subclip (every captured frame, sharp).
   - 2.5x slow: Frame Blend on the subclip, Frames to blend **2**.

| Setting | Default | What it does |
|---|---|---|
| Frames to blend | 5 | Captured frames per output frame. Keyframable. 1 turns it off. |
| Capture frame rate | 300 | The clip's capture rate; sets the time between blended frames. |
| Frames used | From this frame on | Output frame k = mean of captured frames 5k..5k+4, as in #1039's FFmpeg sample (`tmix` + keep 1 of 5), so the two compare frame for frame. "Centred" uses 5k-2..5k+2 instead. |
| Step between frames | One captured frame | "One host frame" is a fallback for a host that counts time in captured frames but reports a lower rate. Not needed in VEGAS. |
| Write diagnostics log | on | Appends what VEGAS passes to `%TEMP%\dodstudio_frame_blend.log` (or the file named by `DODSTUDIO_FRAME_BLEND_LOG`). R&D builds leave it on; it caps itself at the first 300 renders per effect, then 1 in 60. |

At the end of the clip, the frames past the end repeat the last one (no fade to black).

## Install (no admin)

Put `FrameBlend.ofx.bundle` in a folder of your own, for example `<repo>\local\vegas-frame-blend\`,
and point `OFX_PLUGIN_PATH` at that folder for your user account (PowerShell, once):

    [Environment]::SetEnvironmentVariable('OFX_PLUGIN_PATH', '<folder holding FrameBlend.ofx.bundle>', 'User')

Then restart VEGAS (started from a new Explorer or Start menu, so it sees the variable). If
`OFX_PLUGIN_PATH` is already set, add the folder after a `;`. To uninstall, remove the variable or
the folder.

With admin rights, copying `FrameBlend.ofx.bundle` into `C:\Program Files\Common Files\OFX\Plugins\`
works too.

## Build

    powershell -ExecutionPolicy Bypass -File vegas-frame-blend\build.ps1 -Test

Needs Visual Studio 2022 with the C++ x64 tools. Writes `build\FrameBlend.ofx.bundle\` and
`build\ofx_host_harness.exe`; `-Test` runs the harness, `-OutDir <folder>` also copies the bundle
there. Release, static runtime (`/MT`), so the .ofx needs no VC++ redistributable.

**Signing (WDAC):** on a PC that enforces WDAC, unsigned DLLs are blocked. `build.ps1` signs the
.ofx and the harness with the `LocalRustDev` code-signing certificate when the current user has
one, the same step as `build-and-launch.ps1`, and leaves them unsigned otherwise. On the dev PC
where this was built, user-mode code integrity was not enforced (`Win32_DeviceGuard`
UsermodeCodeIntegrityPolicyEnforcementStatus = 0) and VEGAS loaded the unsigned build.

## Test harness

`harness/ofx_host_harness.cpp` is a minimal OFX host of its own: it loads the .ofx, describes it,
and renders from a synthetic clip (a white square moving 2 px per captured frame), checking every
output channel against an average it works out itself. The scenarios model what VEGAS was measured
doing, plus the edges:

- Media FX model (300 fps time base), 8-bit and 32-bit float, 5 frames: 5 distinct captured frames,
  exact. 2 frames, 1 frame (identity), centred 5 and 4, last frame of the clip with and without a
  reported frame range.
- Event FX model (60 fps clip, in-between times snap to whole frames): the plugin passes through.
- A host with a 60 fps time base that does deliver the in-between frames, 16-bit; the "one host
  frame" fallback.
- Speed at 2560x1440 with the 5 source frames cached: about 4-5 ms per frame 8-bit, 6-7 ms float
  (16 threads).

All pass. The output row for 5 frames shows the smear (`2244557777554422`: the 8 px square at 5
positions, 0-9 scale).

## Not done

- Per-event automatic blend from the event's speed: VEGAS does not give the plugin what it needs
  (above).
- The Retimer context: VEGAS lists `OfxImageEffectContextRetimer` as supported, but it is not known
  where VEGAS offers retimer plugins to the user. Not tried.
- Shipping or packaging for other users (#1048: not now).
