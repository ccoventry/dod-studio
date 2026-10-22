# OBS as an Alternate Capture Method: the superseded design

> **Archived 2026-10.** The original design document for OBS capture (#65), minus the parts that
> `docs/obs_alternate_capture.md` keeps as the live reference (how OBS is driven, the measurements, the failure-mode
> table, audio admission, open questions). What is here is design that was superseded or finished: the scene
> picker and "Building a scene" (replaced by the auto-provisioned `[DoD-Studio]` profile and scene,
> `native/src/obs/provision.rs`), Option B/C, the Custom Output (FFmpeg) analysis, batch-lifecycle notes, the
> Render Studio trim reasoning (still open as #84), the staging list and the code-touch list. The text is the
> authors' original, in original order, with gaps where it moved to the reference. Several statements (staging
> "Not built", "no WebSocket client in the workspace") were true when written and are not now.

---

# OBS as an Alternate Capture Method

> **Update 2026-09-03 — the scene/profile design below was superseded, not followed.**
> "The scene picker" and "Building a scene (low priority)" sections recommended a dropdown over the
> user's existing scenes/profiles, with scene *creation* explicitly deferred as a low-priority
> nice-to-have, invoked only on request. What actually shipped (`native/src/obs/provision.rs`) goes
> further in the direction "If it is ever automated, it goes through a profile" already pointed:
> a dedicated `[DoD-Studio]` profile *and* scene, auto-created and **re-verified/repaired on every
> connect** rather than created once and left alone — no picker at all, since there is nothing left
> for the user to choose between. The "detect and state, never mutate" discipline these sections
> argue for still holds, just narrowed to its actual point: never mutate the user's *own* profiles/
> scenes. A profile/scene dod-studio creates and names itself was always the doc's own answer to "if
> it is ever automated" — this just stopped waiting to automate it. See that module's doc comment for
> the current design and reasoning; the sections below are the historical case for it, still useful
> context but no longer the plan.

> **Status 2026-08-28 — built and tested against a live OBS.** Tracks

### The app can measure this for the user, with what it already reads

`achieved render fps` is not a number anyone can be expected to know, and it is the only input the
break-even needs. But the console-log tailer built for the start signal **already measures it**: the
breadcrumb and stage markers carry demo ticks and arrival times, so the ratio of demo time to
wall-clock during a recorded window falls straight out of the same stream — that is exactly how the
1.34x above was obtained, after the fact, from a log.

So after one batch the app knows this machine's capture ratio, and can say something useful and
specific rather than generic: whether OBS would be faster *here*, at *this* `capture_fps`, and
whether the machine can sustain the OBS output rate without dropping frames. That is a far better
answer than a settings tooltip guessing on the user's behalf, and it costs nothing extra to compute
once the tailer exists.

**What it costs** beyond quality, and what a user has to be told: capture becomes sensitive to
everything else on the machine. An alt-tab, a stutter on map load, a notification sound landing in
desktop audio, a screensaver — all of these are now *in the clip*, and none of them can reach the
existing path.

---

## The fast-forward still works, and that matters

`sys_fast_forward` is `host_framerate 0.05` and `sys_normal_speed` is `host_framerate 0`
(`native/src/patch/builder.rs`). Nothing is being recorded between blocks, so the fast-forward
between highlights is untouched by this change — a batch is still much shorter than the demos it
walks.

Only the recorded window changes, and it changes by **doing nothing**: with no `mirv_recordmovie` in
play, the engine simply stays at `host_framerate 0` — real time — for exactly the span the pre-roll
already dropped it into. The pre-roll's own reason for existing (flushing the audio buffers the
fast-forward corrupts, `docs/goldsrc_dod_quirks.md`) is unchanged and still needed.

**The consequence worth stating loudly:** at `host_framerate 0` the engine advances demo time by
real elapsed time. So a block's wall-clock duration equals its demo-time duration *exactly*, even on
a machine that stutters — a stall costs frames, not seconds. That fact is what makes the
synchronisation problem below far smaller than it looks.

---

## The synchronisation problem, and why it is smaller than it looks

The pipeline schedules commands at exact frame ordinals *inside the demo*, and injects them as
`ConsoleCommand` frames. OBS is an external process that knows nothing about demo ticks. The capture
engine, meanwhile, spawns one `hl.exe` for the entire batch and then does nothing but poll for
`DOD_STUDIO_EXIT_TRIGGER` (`native/src/capture_engine.rs`) — it has no per-block awareness at all.

Something has to cross that gap. There are three ways, and the second is the recommendation.


### Option B: record continuously, split at boundaries

**This is not "one big file, cut up afterwards".** `SplitRecordFile` closes the current file and
opens the next one *live*, mid-recording, so OBS itself writes the separate clips. Recording never
stops between blocks, so the output is an alternating sequence:

    [fast-forward junk] [clip 1] [fast-forward junk] [clip 2] [fast-forward junk] ...

and the app deletes the junk segments as they close. The demo is never held as a single file, and the
clips arrive already separate.

What it buys is the elimination of encoder start/stop entirely: no start latency to absorb, and no
exposure to `MIN_TAKE_SEPARATION_SECONDS`-style risk from a stop/start cycle landing too tight.

What it costs:

- **Transient disk for the junk segments.** Small — fast-forward is fast — but non-zero, and it is
  strictly more than Option A writes.
- **Per-block drive routing is gone.** One continuous recording writes to one directory, so the
  export pool collapses to a single drive for the whole chain.
- **It depends on a request** whose availability and per-container support both need checking.

### Which one ships

**Option A, and Option B is a contingency rather than a second user-facing mode.**

Option B's only real advantage is removing start/stop latency — which matters *only if* OBS's start
latency does not comfortably fit inside the pre-roll. **It fits: 59–69 ms measured, against a 2.0s
pre-roll.** So B buys nothing and costs drive routing.

Shipping both as a user choice means maintaining two signalling paths, two verification shapes and
two cleanup paths, on the feature explicitly scoped as the *convenience* option. That is a poor
trade unless the measurements force it. Prototype B if `SplitRecordFile` turns out to be available,
keep it in the back pocket, and revisit only if A's timing does not hold.

### Option C: precompute everything. Rejected.

Fast-forward duration is not predictable — it runs as fast as the machine renders — so wall-clock
offsets from batch start cannot be computed ahead of time. At least one signal per block is
mandatory.

---

## Does Render Studio still run?

**Yes, and it should be offered rather than skipped** — the opposite of the issue's title, and the
head/tail footage from Option A is why.

The render pass on this path is not an encode of thousands of bitmaps. It is: trim the pre-roll and
post-roll, apply the pipeline's naming, and route to the export pool. On a compact hardware-encoded
file that is seconds of work, and it delivers exactly the consistency the issue worried about losing.

Trim precision is the one real decision:

- `-ss` with `-c copy` snaps to a keyframe. OBS's default keyframe interval is around 2s, which is
  the same order as the pre-roll — too coarse.
- Re-encoding gives a frame-exact trim, and on an already-compressed clip it is fast. Given this path
  exists for people trading quality for convenience, a second encode is a smaller compromise here
  than anywhere else in the pipeline.

Recommendation: re-encode, and expose "leave the take as OBS wrote it" as the skip. Do **not** ask
the user to change OBS's keyframe interval — see below. **Unless Custom Output is in play, in which
case there is a better answer** — see the next section.

**Scoped as [issue #82](https://github.com/ccoventry/dod-studio/issues/82), 2026-08-28 — read that
before building this.** The recommendation above turned out to rest on a gap: `scan_folder_background`
(`native/src/hlcr/scanner.rs`) hard-requires a `sound.wav` to discover a take at all, so an OBS take
— which has no wav, its audio already being in the video — is never scanned into a `ClipData` and
never reaches Render Studio's queue in the first place. `is_renderable_take`, built earlier in this
same effort, does correctly recognize the shape, but that is the *Capture Studio verification* path,
a different function from the *Render Studio scanner*. So this is three pieces, not one: scanner
support for the shape, a render path with no separate wav to mux (`run_render_job` currently hard-fails
without one), and the trim itself — which is the easy part once the other two exist. Two decisions
locked in when this was scoped: `ClipData.wav_file` becomes `Option<String>` rather than staying a
bare `String`, and "skip" still routes the take into the export pool (naming applied, same as a
rendered take) just without re-encoding — not a no-op "mark it Rendered" with no file movement at all.


## Custom Output (FFmpeg): lossless capture, and exactly what it costs

OBS's Advanced output mode offers a **Custom Output (FFmpeg)** recording type that exposes the
container, video codec, audio codec and encoder arguments directly. That means OBS can be told to
write `utvideo`, `ffv1` or `libx264 -qp 0` — the same lossless intermediates
`docs/direct_to_video_capture.md` measured for the HLAE path.

**This changes the framing of the whole feature.** The OBS route is the lower-quality option because
capture is *real-time* — dropped frames, no 300fps, no determinism. It is **not** lower-quality
because of compression, and the document should not imply the two are the same cost.

Three consequences follow, and the middle one is the prize:

1. **Lossless codecs are all-intra**, so every frame is a keyframe. The keyframe-snapping objection
   to `-ss` with `-c copy` disappears: the Render Studio trim becomes **frame-exact and a genuine
   stream copy**, seconds of work with no quality loss. That is strictly better than the re-encode
   recommended above, and it is available only in this mode.
2. **`utvideo` in an AVI makes the artefact byte-shaped like the direct-to-video one.**
   `collect_image_folders`, `stream_video`, `avi_frame_count` and the renderer's video branch would
   all work unchanged — the OBS path inherits #42's plumbing for free rather than needing a second
   container taught to the scanner.
3. **OBS states it is "provided with no safeguards".** Misconfiguration produces a broken file
   silently, which is the exact failure class this pipeline keeps being bitten by. It makes the
   duration assertion in `VerifiedBlock` more valuable, not less.

### The cost, measured 2026-08-28

**`SetRecordDirectory` does not steer Custom Output.** Tested directly, with consent, and restored
afterwards:

    AdvOut/RecType -> FFmpegOutput          [took]
    SetRecordDirectory -> <new path>
      GetRecordDirectory  : <new path>      [followed]
      AdvOut/FFFilePath   : <unchanged>     [did NOT follow]

Custom Output keeps its own path in `AdvOut/FFFilePath`, and the recording request only steers the
standard output. So **out of the box, lossless capture and Option A's per-block drive routing are
mutually exclusive** — which matters, because per-block routing across the export pool is the main
advantage Option A has over Option B.

### Which is also the argument for the dedicated profile

`SetProfileParameter` **can** write `AdvOut/FFFilePath` directly — verified, since restoring it is
exactly that write. So per-block routing in Custom Output is achievable; it is simply a *profile*
write rather than a recording call.

That is the same act this document already refuses to perform on somebody's existing profile, and
the same act it already sanctions inside a profile we created. So the two threads converge:

> **A dedicated `dod-studio` profile is not just about the canvas. It is what allows lossless capture
> and per-clip export routing to coexist at all.**

Inside our own profile, writing `FFFilePath` per block is unremarkable. Inside theirs it is not
something to do, and in Standard mode it is not needed, because `SetRecordDirectory` works there.

**So the shape is:** Standard mode works today with routing and needs no profile writes, at the cost
of codec choice. Custom Output buys lossless, all-intra capture and a free frame-exact trim, and
requires the dedicated profile to keep routing. Offer Standard first; treat Custom Output as the
quality tier that comes with the profile.

Measured incidentally, and it closes an open question: the live profile's Standard container is
**`hybrid_mp4`**, which is the crash-safe MP4 variant. So the "MP4 risks an unfinalised file" worry
does not apply to a default modern OBS.

---

## The capture source is the real unknown

OBS needs a source pointed at the game. `hl.exe` runs **windowed** (`-windowed` is hardcoded in
`build_hlae_process`) at the configured `-w`/`-h`, which helps: a windowed target is what Window
Capture handles best, and the resolution is known ahead of time, so the OBS canvas can be checked
against it in the preflight rather than discovered by scaling artefacts.

**Game Capture was the risk, and it is answered.** HLAE's `AfxHookGoldSrc.dll` is already injected
into `hl.exe` and already hooks its OpenGL presentation; OBS Game Capture injects its own graphics
hook into the same process to do the same thing.

Measured 2026-08-28: both hooks load into the same live process, the game keeps running, **and the
captured frames contain the game** — see "Measured against a live OBS" above. Window Capture is kept
in mind as a fallback that injects nothing and therefore cannot collide, but nothing currently
requires it.

Fall back to **Window Capture**, which does not inject anything and therefore cannot collide, at the
cost of requiring the window to stay unoccluded. Display Capture is the last resort.

**On the scene itself: detect and state, never mutate.** This is the same discipline `cfg_scan`
applies to the game's own `.cfg` files and the direct-to-video work applies to HLAE's `ffmpeg.ini` —
someone's OBS scene collection is their streaming setup, not our scratch space. Creating a scene, or
repointing an existing source, could break a livestream to fix a capture. The same rule covers the
recording format, encoder, keyframe interval and file-splitting settings: read them, report anything
that will produce a bad result, change nothing.

### The scene picker

"Detect and state" does not have to mean making the user describe their OBS setup by hand. Everything
needed to populate **a dropdown of their actual scenes** is a read:

| request | what it gives the picker |
|---|---|
| `GetSceneCollectionList` | the active collection, and that there are others |
| `GetSceneList` | the dropdown's contents |
| `GetSceneItemList` | what each scene actually contains |
| `GetInputList` | each source's `inputKind` — `game_capture`, `window_capture`, `monitor_capture`, `wasapi_output_capture`, `wasapi_process_output_capture` |
| `GetInputSettings` | which window a capture source is pointed at |

So the picker can do better than list names: it can badge each scene with whether it holds a capture
source aimed at `hl.exe`, which kind (and therefore whether it is the one that might collide with
`AfxHookGoldSrc`), and whether any audio source is present at all — the failure that otherwise
produces a perfectly valid silent clip. That turns the preflight from a list of complaints into a
choice with the consequences written next to it.

**Scene names are scoped to a scene collection.** A remembered scene name is not meaningful on its
own: switch collections and it either vanishes or, worse, resolves to something unrelated. Store the
collection alongside the name and re-validate at dispatch.

`SetCurrentProgramScene` — switching to the chosen scene when a batch starts — is worth calling out
as a *third* category, between reading and editing. It changes live state, so it is a mutation; but
it is reversible, destroys nothing, and is exactly what a user picking a scene is asking for.
Restore the previously-active scene when the batch ends, and say so in the UI.

### Video settings, and why the canvas is not ours to set

`SetVideoSettings` is present, so canvas resolution, output resolution and FPS *can* be set from
code. They mostly should not be, and the reason is not the one it looks like.

**A canvas is not a scene property.** It belongs to the active **profile** and is shared by every
scene, so writing to it for a capture writes to whatever else the user does in OBS.

**The damage is the transforms, not the resolution.** Scene item positions and scales are stored in
canvas coordinates, in the scene *collection*. Drop the canvas from 1920x1080 to 1280x720 and every
source in every scene is mispositioned. Nothing in the API signals that, and it is the failure a
user would notice next time they streamed rather than during our batch.

Two further constraints, both measured or documented rather than assumed:

- **`SetVideoSettings` is refused while an output is active.** It has to run in preflight, before
  `StartRecord`. It cannot rescue a batch mid-flight.
- **Profiles and scene collections are separate axes.** Switching profile keeps the current scene
  collection, so a dedicated profile alone does *not* solve the transform problem — the sources are
  still laid out for the old canvas.

The mismatch is worth detecting regardless, and detecting it is nearly free: the pipeline already
knows the game's resolution (`resolution_width` / `resolution_height`), so comparing it against
`GetVideoSettings` is one comparison.

**And it is not a cosmetic check.** The live install had a 1920x1080 canvas, a 1280x720 output, and
a game rendering at 1280x720 — and the captured frame shows what that actually produces: the game
occupying roughly **854x480 in the top-left of a 1280x720 frame**, black everywhere else. The source
sits at its native size on a canvas 1.5x larger, and the whole canvas is then scaled down by 1.5 to
reach the output. About **two thirds of the pixels are discarded before the encoder ever sees
them.**

Nothing in OBS flags this; the recording is perfectly valid and simply much worse than the machine
is capable of. Output FPS was also 30, which on this path governs the clip entirely.

Setting the base canvas to match the game — 1280x720 here — makes the whole path 1:1 with no
resampling at all. That is a one-line preflight comparison away from being impossible to get wrong.

**So: detect and report, do not write.** The preflight says the canvas disagrees with the game's
resolution and what to set it to, and says when the output FPS is low. Always correct, cannot break
anything, and a one-time manual fix is cheap.

**If it is ever automated, it goes through a profile.** `CreateProfile` and `SetCurrentProfile` are
both present, and a profile carries canvas, output resolution, FPS, recording format, encoder and
output directory — every setting this feature wants pinned. Create a `dod-studio` profile, switch to
it for the batch, switch back after. Additive and reversible, never editing the profile they are
already using. `SetSceneItemTransform` exists to re-fit a source afterwards, but only ever inside a
scene we created.

That puts it in the same tier as the scene builder below, and it should land with it.

### Building a scene (low priority)

`CreateScene`, `CreateInput`, `SetInputSettings` and `CreateSceneItem` make an offered "set one up
for me" feasible, and the recommended configuration is non-obvious enough to be worth automating:
a capture source targeting the `hl.exe` window, an Application Audio Capture on the same process
rather than Desktop Audio, and a canvas matching the pipeline's own `-w`/`-h`.

The discipline that makes this acceptable is that it is **purely additive**: create a *new* scene,
never touch an existing one. That is a different act from repointing a source someone is streaming
with. It must still be explicitly invoked, never run as part of a preflight, and it should say what
it is about to create before creating it.

Worth having, worth doing last. The picker is what makes the feature usable; scene creation only
makes first-time setup nicer, and it is the part most likely to age badly as OBS's input kinds and
settings change under it.

---

## Batch lifecycle: what must not be forgotten

The capture engine's failure and cancel paths currently assume the only external process worth
worrying about is one it spawned. OBS is not.

- **Cancellation.** `taskkill /F /IM hl.exe` fires and the batch unwinds — with OBS still recording,
  forever, into the user's drive. `StopRecord` must be part of the cancel path.
- **Crash.** The engine already distinguishes "hl.exe never started", "crashed mid-capture" and "exit
  trigger seen". Every one of those exits needs the same `StopRecord`.
- **`CaptureCleanupGuard`.** This is the right home for it: it already runs on drop for every path out
  of the batch, which is exactly the guarantee needed. It is not currently async and the WebSocket
  client is — worth resolving deliberately rather than by standing up a runtime inside a `Drop`.
- **Preflight, and fail fast.** OBS not running, WebSocket refused, wrong password, already recording,
  no source pointed at the game — every one of these otherwise produces a batch that runs to
  completion and captures nothing. The existing pre-launch drive-headroom check is the precedent:
  refuse before spawning, not after.
- **Disk accounting is wrong for this path.** `build_batch_queue` bin-packs blocks across drives by
  estimated BMP footprint, and the engine re-validates `drive_headroom` before launch. OBS output is
  one or two orders of magnitude smaller and lands wherever OBS points, so those estimates would
  refuse batches that would fit comfortably. The check should move to OBS's own record directory with
  an estimate sized to this path.
- **`_route_N` junctions and `mirv_movie_filename` become dead weight** — HLAE writes nothing. They
  can stay (harmless, and the `DOD_STUDIO_EXIT_TRIGGER` alias still uses the mechanism) or be skipped;
  no reason to touch them in a first version.
- **Capture modes are mutually exclusive.** Frame sequence / direct-to-video / OBS is a three-way
  choice, not three checkboxes. `ffmpeg_capture` and an OBS mode must not both be settable.
- **`MIN_TAKE_SEPARATION_SECONDS`** (1.0s) exists because a `mirv_recordmovie` stop/start cycle that
  tight risks a take landing without audio. The rule transfers, and **the number does not**: OBS took
  **~1.065s** to finalise a file after `StopRecord` returned, measured twice. That is already longer
  than the separation the merge rule guarantees, so on this path the constant must be raised — two
  highlights that merge today would otherwise produce a stop/start cycle OBS cannot service.

---

## Where this touches the codebase

- **`native/src/capture_engine.rs`** — the batch loop gains a log tailer and an OBS client;
  `CaptureCleanupGuard` gains `StopRecord`; the pre-launch checks gain a preflight.
- **`native/src/obs/`** (new) — WebSocket client, handshake, request/event types. Behind
  `#[cfg(not(target_arch = "wasm32"))]` like the rest of the process/IO surface.
- **`native/src/patch/types.rs`** — `PatcherConfig` gains the OBS settings beside `ffmpeg_capture`
  and `ffmpeg_capture_codec`, which are the pattern to copy.
- **`native/src/hlcr/scanner.rs`** — `is_renderable_take` gains the video-with-audio case;
  `avi_frame_count` needs a companion if the container is not AVI.
- **`native/src/hlcr/renderer.rs`** — a trim branch on the existing video-input path.
- **`studio/src-tauri/src/capture_manager.rs`** — `VerifiedBlock`'s two tiers gain the
  duration assertion; settings plumbing for host/port/password.
- **`studio/src/`** — capture-mode selector, connection settings, preflight report. Every
  `invoke()` needs its `.catch()`.
- **`native/src/strings.rs`** — new user-facing strings go here, per the centralisation pass.

Nothing in `dod/`, `analysis/`, `dem-patch/` or the patcher's frame-writing code is affected. This
feature does not change a single injected frame — which is the strongest argument for it being a
tractable piece of work.

---

## Staging

0. ~~**Talk to OBS.**~~ **Done 2026-08-28.** `probe_obs obs` — every needed request is present, and
   the start latency is 59–69 ms. See "Measured against a live OBS".
1. ~~**Prove Game Capture actually delivers frames.**~~ **Done 2026-08-28.** Both hooks coexist and
   the captured frames contain the game. Method worth reusing:
   `tools/ffmpeg.exe -i <file> -vf blackdetect=d=0.05:pic_th=0.98 -an -f null -`, then extract one
   frame and look at it — a valid-but-empty recording is the failure mode here, and only the second
   step catches it.
2. ~~**Tail the console log.**~~ **Done 2026-08-28.** 21–40 ms marker latency across 17 blocks under
   the heaviest I/O configuration. The channel works; the `screenshot` fallback is not needed.
3. ~~**Wire one block end to end.**~~ **Done 2026-08-28.** Log tail → `StartRecord` on `AUDIO_SYNC`
   → `StopRecord` on `STOP_RECORD` → folded into the take folder. Verified against a real batch:
   blocks land as `<take>/take0000/all/video.mp4`, decode clean, and carry their audio.
4. ~~**The predicate**~~ **Done 2026-08-28.** Both sides at once, testing for the audio *stream* —
   a muted OBS source writes a perfectly valid silent file, and admitting it would render a clip
   with no sound and no error.
5. ~~**Preflight, cancel and crash paths.**~~ **Done and tested live 2026-08-28.** See the failure
   table above. `MIN_TAKE_SEPARATION_SECONDS` is mode-aware via `OBS_TAKE_SEPARATION_SECONDS`.
6. **The trim pass in Render Studio**, and the setting to skip it. *Not built.*
7. **Measure it.** Wall-clock and disk against a real batch, next to the same batch through the
   existing path. The speed claim is still an expectation. *Not done.*

---

---

## Appendix: paragraph replaced in the reference

**No WebSocket client is in the workspace today.** `native/Cargo.toml` has `tokio` (with `rt`,
`macros`, `process`, `fs`, `sync`, `time`) and `serde_json`, so `tokio-tungstenite` plus a SHA-256
and base64 crate for the handshake is the addition. That is a new dependency on a crate family this
project has not used, and is worth a moment's thought before it lands.
