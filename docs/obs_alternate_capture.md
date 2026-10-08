# OBS as an Alternate Capture Method

> **Reference, restructured 2026-10.** The feature shipped ([#65](https://github.com/ccoventry/dod-studio/issues/65) closed; code in `native/src/obs/`).
> This file keeps how OBS is driven, what was measured, the failure-mode table and what is still open.
> The superseded design (scene picker, "Building a scene", Option B/C, the Custom Output analysis, staging, batch-lifecycle
> notes, the Render Studio trim reasoning, the original 2026-09-03 update banner) is in `docs/archive/obs_alternate_capture_design.md`;
> sections named below that are not here ("Custom Output", "Does Render Studio still run", "The scene picker") live there.
>
> **The scene is auto-provisioned.** A dedicated `[DoD-Studio]` profile *and* scene, created and
> **re-verified/repaired on every connect** (`native/src/obs/provision.rs`; see its doc comment), so there is no scene picker.
> The "detect and state, never mutate" discipline narrowed to its actual point: never mutate the user's *own*
> profiles/scenes. **Markers now also arrive over the game's events pipe** (`native/src/obs/pipe_tail.rs`, #434 step 1);
> `qconsole.log` stays the source until the pipe connects and for a game without the hook. Where the text below says
> `qconsole.log`, read "log, or pipe".

> [#65](https://github.com/ccoventry/dod-studio/issues/65), in PRs
> [#72](https://github.com/ccoventry/dod-studio/pull/72) (this document) →
> [#73](https://github.com/ccoventry/dod-studio/pull/73) (`probe_obs`) →
> [#74](https://github.com/ccoventry/dod-studio/pull/74) (the feature).
>
> **What has actually been run:** a full batch producing playable clips with audio; a cancel
> mid-recording; OBS killed between blocks; OBS killed mid-recording; dod-studio killed mid-batch and
> recovered on restart. Every failure path in the table below except the stall watchdog.
>
> - OBS Game Capture captures the HLAE-injected `hl.exe` — verified against a real frame.
> - `qconsole.log` carries the per-block signal at **21–40 ms**, measured over a 17-block batch under
>   the heaviest I/O configuration the pipeline has.
> - Every obs-websocket request the design needs exists on the install tested.
>
> **Not built (as of 2026-10, unchanged):** the Render Studio trim pass (staging item 6, now
> [#84](https://github.com/ccoventry/dod-studio/issues/84), open), and the wall-clock/disk comparison
> against the existing path (item 7, no issue) — so the speed claim remains an expectation.
>
> See "Measured against a live OBS". **One claim was refuted rather than confirmed:** the wall-clock
> saving is small — the real prize is disk. See "What OBS actually buys".
>
> **Scope, set by the user 2026-08-28**, narrowing the issue's own title:
>
> - **Not a replacement.** It sits alongside the existing capture path.
> - **An alternate option for people who care less about quality** and want the convenience.
> - **Separate HUD is out.** "That won't work with OBS", and it is not wanted here. No effort goes
>   into HUD compositing on this path.
>
> Prerequisite reading: `docs/direct_to_video_capture.md` (#42, shipped in PR #68) answered how a
> non-BMP capture artefact flows through take verification, Render Studio's admission predicate and
> export routing. Several of its answers transfer directly and are cited rather than re-derived.
> `docs/goldsrc_dod_quirks.md` and `docs/hlae_protocols.md` hold the engine facts.
>
> **Everything below marked *(unverified)* needs an experiment.** The sections that are settled are
> settled by reading this repository's own code, or — where it says so — by the probe run recorded
> under "Measured against a live OBS" below.

---

## Measured against a live OBS, 2026-08-28

Run with `native/src/bin/probe_obs.rs` against **OBS 32.2.2 / obs-websocket 5.7.4**. Everything in
this section is measurement, not expectation.

**Every request this design needs exists**, including both that were open questions:

| request | | |
|---|---|---|
| `StartRecord` / `StopRecord` / `GetRecordStatus` | YES | the core loop |
| `GetRecordDirectory` / `GetVideoSettings` | YES | preflight |
| **`SetRecordDirectory`** | **YES** | so Option A's per-block export-pool routing is possible |
| **`SplitRecordFile`** | **YES** | so Option B exists as a contingency |
| `GetSceneList` / `GetSceneItemList` / `GetInputList` / `GetInputSettings` / `GetSceneCollectionList` | YES | the scene picker is buildable |
| `SetVideoSettings` / `GetProfileList` / `SetCurrentProfile` / `CreateProfile` | YES | see "Video settings" below |

**Start latency: 59–69 ms**, measured twice — the gap from `StartRecord` to
`RecordStateChanged: OBS_WEBSOCKET_OUTPUT_STARTED`, which is when frames actually begin. The
pre-roll budget is `AUDIO_RESYNC_SECONDS` = 2.0s, so this fits roughly thirty times over.
**Option A's timing holds, and Option B's only real advantage disappears** — it stays a contingency
and should not ship as a second user-facing mode.

**Stop to file finalised: ~1.065 s**, also measured twice. This is a problem worth naming:
`MIN_TAKE_SEPARATION_SECONDS` is **1.0 s**, so OBS needs marginally *longer* to finalise a file than
the pipeline currently guarantees between one take's stop and the next one's start. That constant
was derived for HLAE and must be raised for this path.

**The container was MP4**, not the MKV this document assumed OBS defaults to. Both need handling, or
the setting needs pinning — see the open question on containers.

### The console log carries the signal — measured over a 17-block batch

Run with `probe_obs log` against a real capture: `capture_fps` 120, 1280x720, `ffmpeg_capture` off
and **Separate HUD on**, so HLAE was writing three BMP sequences throughout. That is the heaviest
I/O configuration the pipeline has, which makes it the right test rather than a lucky one.

**Markers reach `qconsole.log` with per-frame granularity.** Commands the pipeline schedules one tick
apart — `SPEED_FLUSH` after `CUSTOM_CMD1_BEFORE`, `CUSTOM_CMD2_AFTER` after `STOP_RECORD` — arrived
**21–40 ms** apart in all 17 blocks. Nothing accumulated, nothing flushed late, and no marker was
ever missing. **`qconsole.log` is a viable signalling channel and the `screenshot` fallback is not
needed.**

The three speed regimes separate cleanly, which is the control that says the numbers are real:

| phase | measured rate |
|---|---|
| fast-forward (`host_framerate 0.05`) | ~5,400 ticks/s |
| pre-roll, settled (`host_framerate 0`) | ~474 ticks/s — real time |
| recorded window (`mirv_recordmovie`) | ~355 ticks/s |

The demo ran at **474 ticks per second**, confirmed independently: `pre_roll_seconds` was 5.0 and
`SPEED_FLUSH`→`START_RECORD` measured 2370 ticks in every block, which is exactly 5.0 x 474.

**Demo time tracks wall clock at `host_framerate 0`, with jitter.** The `AUDIO_SYNC`→`START_RECORD`
span is 1.0s of demo time by construction; across the 15 blocks that got a full lead it measured a
**mean of 1.010s wall-clock, σ ≈ 0.14s, range 0.866–1.194s**. No systematic bias — an earlier
single-sample reading of "13% short" was simply one draw from that spread — but the jitter is real,
and it is why the stop is now driven by an echo rather than a timer.

**The early pre-roll is not yet real time.** `SPEED_FLUSH`→`AUDIO_SYNC` is 4.0s of demo time and
consistently measured shorter in wall-clock (~2.8–3.7s). Whatever the mechanism, the engine has not
settled immediately after leaving fast-forward, and it has by `AUDIO_SYNC`. That is what decides the
trigger point in Option A.

### The hook question: answered, both halves

With the HLAE-launched game running, `hl.exe` had **both** hooks loaded at once:

    AfxHookGoldSrc.dll      HLAE's hook
    graphics-hook32.dll     OBS Game Capture's hook
    OPENGL32.dll            the API both are hooking

So OBS Game Capture injects successfully into an HLAE-injected GoldSrc process, and the game stays
alive and responsive. That retires the *crash* form of the risk.

**And the frames arrive.** A 6-second probe recording taken while that process was rendering came
back with **no black frames at all** (`blackdetect d=0.05 pic_th=0.98`), 179 frames across 5.967s at
30fps — no drops — and an extracted frame is the Day of Defeat menu in colour. Two hooks on the same
OpenGL buffer swap do not fight.

**So Game Capture is viable and Window Capture is not needed as a fallback.** This was the largest
risk in the document and it is now closed on evidence rather than on the absence of a crash.

**A caution worth keeping, because it cost two rounds of analysis.** A Game Capture source whose
target process has exited records black into a file that is valid in every other respect — right
resolution, right frame rate, right duration, sometimes real audio from another source. Two probe
recordings were wasted that way before the cause was understood, and the second one looked exactly
like a hook collision. dod-studio owns the game's lifecycle, and *any* rebuild of the workspace while
`tauri dev` is watching takes `hl.exe` down with it — including a `git checkout` that touches
`Cargo.toml`. The game having been launched earlier in a session is not evidence that it is running
now, which is why `probe_obs` checks and says so.

---

## The one fact everything else follows from

`mirv_recordmovie` does not capture in real time. It pins the engine's timestep to
`1 / mirv_movie_fps` and renders **every** step regardless of wall-clock, so a 40-second clip at 120
fps takes as long as the machine needs and comes out frame-exact every time. The same demo captured
twice is byte-identical.

OBS captures a window in real time. It takes whatever the screen actually showed.

That single difference decides the shape of the whole feature:

| | HLAE (`mirv_recordmovie`) | OBS |
|---|---|---|
| engine timing during a clip | `host_framerate` pinned to `1/fps` | `host_framerate 0` — real time |
| wall-clock per clip | `capture_fps / achieved render fps` — **measured 1.34x real time** at 120fps | exactly the clip's duration, always |
| high FPS / slow motion | the point of the path | not possible |
| dropped frames | impossible by construction | whatever the machine drops |
| determinism | byte-identical reruns | no |
| audio | separate `sound.wav`, muxed later | in the file already |
| output | thousands of BMPs, or one lossless video | one finished, playable file |
| render pass | required | optional (see below) |

**What OBS actually buys — corrected 2026-08-28 by measuring a real batch.** The original claim here
was wall-clock, and that is mostly wrong.

A 17-block batch at `capture_fps` 120, 1280x720, with Separate HUD on (three BMP streams), recorded
**309s of demo time in 413s of wall-clock — 1.34x real time**. OBS is fixed at 1.0x by definition,
so it would have saved about 104s out of the 569s the whole capture phase took: **roughly 18%**.

HLAE's wall-clock cost is `capture_fps / achieved render fps`, and that machine achieved **~90 fps
while writing three BMP streams** at 720p. So on *that* machine, at `capture_fps` 60, HLAE would run
at ~0.67x real time — faster than OBS could ever be, since OBS cannot capture quicker than the clip
plays.

> **The break-even is `capture_fps` = achieved render fps, and it moves with the machine.**

**Which is the part that generalises, and the 1.34x figure is not.** The machine measured here is
high-end; 1.34x is close to the best case for HLAE, so **that measurement is a lower bound on OBS's
wall-clock advantage, not a typical one**. Work the formula for a weaker machine and it inverts:

| achieved render fps | HLAE at `capture_fps` 60 | HLAE at `capture_fps` 120 |
|---|---|---|
| ~90 (measured here) | 0.67x — HLAE wins | 1.34x — OBS saves ~25% |
| ~45 | 1.33x — OBS saves ~25% | 2.7x — OBS saves ~63% |
| ~30 | 2.0x — OBS saves ~50% | 4.0x — OBS saves ~75% |

So the speed pitch is real, just not for the person who measured it. **OBS saves the most time
exactly for the users with the least capable hardware** — which is plausibly the same audience that
wants the convenience option in the first place.

**And that cuts the other way too, which is the tension worth stating plainly.** A weak machine is
also the one that cannot render 60 fps in real time, so OBS drops frames there. The machines where
OBS saves the most wall-clock are the machines where OBS produces the worst output. That sharpens
the dropped-frames open question below from a nicety into the thing that decides whether this path
is usable for the people it most helps.

The durable advantages are elsewhere, and they are large:

- **Disk.** That same batch wrote on the order of **300 GB** of bitmaps (309s x 120fps x 3 streams x
  2.76 MB). The OBS equivalent is a few hundred megabytes. This is the reason `build_batch_queue`
  bin-packs across a pool of drives at all, and it is the difference between needing that machinery
  and not.
- **No render pass.** Audio is already muxed, so a finished, playable file exists the moment the
  clip ends.
- **Hardware encoding for free**, concurrent with capture rather than after it.

That is still a real pitch. On a strong machine it is a disk-and-simplicity pitch; on a weak one it
is a speed pitch as well, with a quality cost attached.


## How OBS gets driven

`obs-websocket` v5 ships inside OBS Studio 28 and later; there is no plugin to install. Default port
`4455`, optional password, JSON over WebSocket, SHA-256 challenge/salt handshake.

The requests this needs:

| request | why |
|---|---|
| `GetVersion` | preflight. Returns `obsVersion`, `rpcVersion` **and `availableRequests`** — which is how to feature-detect the optional requests below rather than version-sniffing. |
| `GetRecordStatus` | `outputActive`, `outputDuration`, `outputBytes`. Per-block verification, and the answer to "is OBS already recording something else". |
| `StartRecord` / `StopRecord` | `StopRecord` returns `outputPath` — the artefact's location, which is what take verification keys off instead of a take folder. |
| `RecordStateChanged` (event) | `OBS_WEBSOCKET_OUTPUT_STARTED` / `STOPPED`, with `outputPath` on stop. Preferable to polling: it reports the moment recording *actually* began, which is not the moment `StartRecord` returned. |
| `GetRecordDirectory` | where files will land, for the preflight report and the disk check. |
| `SetRecordDirectory` | **confirmed present** — per-block export routing without a cross-drive copy. |
| `SplitRecordFile` | **confirmed present** — the basis of Option B below. Per-container support still unchecked. |
| `GetSceneList`, `GetCurrentProgramScene`, `GetSceneItemList`, `GetInputList`, `GetInputSettings` | read-only scene inspection for the preflight. |
| `GetVideoSettings` | canvas/output resolution and `fpsNumerator`/`fpsDenominator` — the number that replaces `mirv_movie_fps` in `take_meta`. |

The client is `native/src/obs/client.rs` (the "no WebSocket client in the workspace" paragraph that
used to stand here, from before it was built, is in the archive).

---


### The channel: the console log is already tick-accurate telemetry

`build_safe_echos` already writes an echo at every stage boundary of every block:

    [dod-studio] SPEED_FLUSH - Tick 41250
    [dod-studio] AUDIO_SYNC - Tick 41350
    [dod-studio] START_RECORD - Tick 41450
    [dod-studio] STOP_RECORD - Tick 44950
    [dod-studio] FAST_FORWARD - Tick 45150

plus a `BREADCRUMB` every `BREADCRUMB_INTERVAL_TICKS`. With `-condebug` — **which
`build_hlae_process` passes on every launch, with no way to turn it off**
(`native/src/patch/types.rs`) — these land in `qconsole.log` beside `hl.exe`, a
file the app already knows about and deletes (`shared::paths::remove_console_log`).

So the signalling channel exists, is tick-accurate, needs no new engine commands, and costs the
capture nothing. **The app has simply never read it.**

The alternative the issue proposed — reusing the `DOD_STUDIO_EXIT_TRIGGER` trick, i.e.
`mirv_movie_filename X; mirv_recordmovie_start; mirv_recordmovie_stop` to make a folder appear —
works, but should be rejected here: it starts a real HLAE recording for an instant, which yanks
`host_framerate` to `1/mirv_movie_fps` and back. That is a visible hitch landing *precisely* at the
first frame of the clip, and under direct-to-video it also spawns an FFmpeg process per block.

**Measured, and the answer is good: 21–40 ms per marker**, across 17 blocks, with HLAE writing three
BMP streams at 120fps throughout. GoldSrc's debug log does flush per line. See "The console log
carries the signal" above.

*(A `screenshot`-based marker was the fallback had the log turned out to be buffered — a console
command with a filesystem side effect, inside the 64-byte `ConsoleCommand` field, costing one frame rather than a
`host_framerate` yank. It is not needed and is recorded here only so the option is not re-derived.)*

### Option A (recommended): drive both ends off the echoes

**Revised 2026-08-28 after measuring a real 17-block batch.** The original plan was to signal the
start and compute the stop by timer, because the log's latency was unknown and a timer avoided
depending on it twice. The measurement removed the reason: markers reach `qconsole.log` in
**21–40 ms**, so there is nothing to avoid.

1. Tail `qconsole.log`. On **`AUDIO_SYNC`** for block *i*, send `StartRecord`.
2. Wait for `RecordStateChanged: STARTED`.
3. On **`STOP_RECORD`** for block *i*, send `StopRecord`.
4. `StopRecord` returns `outputPath`.

Three properties make this work, and two of them are now measured rather than argued:

- **OBS's start latency fits the lead many times over.** `AUDIO_SYNC` fires
  `SOUND_FLUSH_LEAD_SECONDS` = 1.0s of demo time before the record start, measured at **1.010s mean
  wall-clock across 15 blocks**. OBS starts frames in **59–85 ms**. That is a ~14x margin.
- **`AUDIO_SYNC`, not `SPEED_FLUSH`, is the right trigger.** The full pre-roll was 5.0s in the
  measured batch, but its *early* portion does not run at real time — the engine is still settling
  out of fast-forward, and that span consistently measured shorter in wall-clock than its demo
  duration. By `AUDIO_SYNC` it has settled. Triggering there also means only ~1s of pre-roll head
  ends up in the file instead of five, so there is less to trim.
- **Stopping on the echo removes the design's one shaky assumption.** The timer needed demo time to
  track wall clock exactly. It does in the mean, but with **σ ≈ 0.14s** on a 1.0s span — and not
  every block gets the full lead (two of seventeen were clamped, at 454 and 81 ticks, where blocks
  chained or highlights merged). An echo-driven stop is self-correcting and needs none of that. Keep
  the computed duration as a **fallback timeout** for a missing echo, not as the primary mechanism.
- **Each clip is its own recording, so each clip can be routed independently.** With
  `SetRecordDirectory` between blocks, the multi-drive export pool the pipeline already bin-packs
  across keeps working on this path. Nothing else in this design preserves that.

Only the clip and its rolls are ever written, so the disk cost is the output and nothing else.

The cost is that the recording contains the pre-roll and post-roll as head and tail footage. See
"Does Render Studio still run" below — the user can simply trim it in their editor, which is why the
render pass stays *optional* here rather than becoming a requirement.

## The output, and the recommendation that keeps the rest of the pipeline

OBS writes one file wherever its profile points, named by its own filename formatting. `StopRecord`
returns the authoritative path.

**Recommendation: fold it back into the take-folder shape.** After `StopRecord` returns, move the
file to `<block take_folder>/take0000/all/video.<ext>`. On the same volume that is a rename, i.e.
free. The block's `take_folder` is already computed at dispatch (`native/src/patch/builder.rs:788`),
and `shared::paths::take_key` already matches across the `take0000` nesting.

The payoff is large and mostly consists of *not writing code*:

- `collect_image_folders` already admits a stream folder holding a video (`VIDEO_FILE`).
- `stream_video` / `avi_frame_count` already read frame counts out of the container for the progress
  bar — though `avi_frame_count` is AVI-specific, and OBS defaults to MKV. Either constrain the
  container or teach the scanner a second one *(unverified which is less bad)*.
- `renderer.rs` already has the video-input branch, and already omits `-framerate` for a video take
  because a video carries its own timing.
- Export-pool JIT drive routing, output naming (`{demo}_{take}_{stream}_{hash}.{ext}`) and the
  hash-suffixed uniqueness all keep working untouched.
- `take_key` keeps matching, so the capture side and Render Studio still agree about which take is
  which.

The alternative — leaving the file where OBS put it and teaching every downstream component a second
artefact shape — is strictly more work for a worse result.

### The one predicate change

`is_renderable_take` requires **a wav *and* a stream folder**. An OBS take has the video with audio
already inside it and no wav at all, so it fails. This is the single deliberate change, and its own
doc comment states the rule: *"Shared with the capture-side take verification so 'the capture
succeeded' and 'Render Studio can actually see it' can never silently disagree — if this predicate
changes, both sides change together."*

The shape wanted is a third admissible case: a stream folder holding a video **with an audio
stream**, no wav required. Testing for the audio stream rather than just relaxing the wav rule is
what stops a silently-muted OBS take being admitted as renderable — and unlike the frame-sequence
case, that check is cheap, because the container header carries it.

### Take verification gets *stronger*, not weaker

`take_folder_has_content` is deliberately loose — non-empty folder, ignoring our own metadata — so an
unanticipated HLAE layout degrades to a warning. The OBS path can do much better, because the app
knows things it has never known before:

- `GetRecordStatus` reported `outputActive` and a duration for that block.
- `StopRecord` named a file, and that file exists and is non-trivial.
- **The expected duration is known exactly** (the real-time property again), so the recorded duration
  can be asserted within tolerance.

That last check has no equivalent in the BMP path, and it catches precisely the failure this pipeline
keeps getting bitten by: output that exists, looks plausible, and is wrong. `VerifiedBlock` keeps
both tiers — `captured` becomes "OBS reported a file of about the right length", `renderable` stays
the shared predicate.

`take_meta` needs no structural change: it records the capture rate per take, and OBS's rate from
`GetVideoSettings` goes in the same field. The existing `[render-fps-mismatch]` check keeps working.

---

## Render Studio and OBS takes: where it stands

The design for this (the "Does Render Studio still run?" reasoning, and the [#82](https://github.com/ccoventry/dod-studio/issues/82) scoping, closed) is in the archive.

**Scanner + render-path + skip landed 2026-08-28.** `scan_folder_background` now discovers an OBS
take via the shared `take_shape_is_renderable` predicate; `run_render_job` has a `RenderCodec::SourceCopy`
branch (surfaced in Render Studio as a per-job "Skip" toggle, offered only for an OBS-shaped clip)
that copies the OBS-written file straight into the export pool with no FFmpeg pass, and a plain
single-video-input branch for the "render" case when `wav_file` is `None`. **The trim itself did not
ship with this**: nothing on disk records how many seconds of pre-roll/post-roll head and tail an OBS
block actually has, so "render" currently re-encodes the whole clip (codec conversion, export-pool
routing, pipeline naming) without cutting the head/tail. That gap — and the capture-side metadata it
needs — is tracked separately as
[issue #84](https://github.com/ccoventry/dod-studio/issues/84).

---


## Audio: solved, and newly breakable

The genuine simplification. OBS records audio into the file, so the separate `sound.wav` and its mux
disappear, along with the FPS-mismatch class of bug that comes from the two being timed independently
(`docs/hlae_protocols.md`: the wav's duration is the frame count's cross-check — with one file there
is nothing to cross-check, because nothing can drift).

Three things it breaks that the existing path cannot:

1. **Desktop Audio captures the whole machine.** A Discord notification during a clip is in the clip.
   Application Audio Capture targeting `hl.exe` avoids this and is the right recommendation, with the
   caveat that it is a newer Windows capture method. *(Unverified against this setup.)*
2. **`stopsound` fires audibly.** `sys_record_start` is `mirv_recordmovie_start; stopsound`, and
   `sys_sound` fires a `stopsound` a second before it. Under HLAE those land outside or at the very
   edge of the captured audio; under OBS the microphone is already open, so a `stopsound` cutting
   sustained sounds is *recorded*. Whether the pre-roll's `stopsound` is needed at all once nothing is
   being time-warped through the record window is worth asking — it exists to fix fast-forward audio
   corruption, and Option A starts OBS before the flush anyway, putting it in the discarded head.
3. **Nothing enforces that audio was captured.** A muted source produces a perfectly valid file.
   Hence the audio-stream test in the predicate above.

---


## Every way a batch can end, and what stops OBS in each

Worked through 2026-08-28 after the first end-to-end test, because "the cancel button stops OBS" is
only one of the ways a batch stops. The question that matters for every row is the same: **does
anything survive to send `StopRecord`?** Where nothing does, OBS records until the drive fills.

| How it ends | What the engine sees | What stops OBS |
| --- | --- | --- |
| Cancel Batch | Cancel token → `taskkill` → break | `session.finish()`. Verified. |
| `quit` in console | hl.exe gone, no exit trigger | Same path, via the crash branch |
| ALT+F4 | Identical to `quit` | Same |
| End Process hl.exe | Identical to `quit` | Same |
| **`disconnect` in console** | **Nothing — hl.exe is alive and idle at the menu** | **Marker-silence watchdog** |
| Demo fails to load, or ends early | Same as `disconnect` | Same watchdog |
| hl.exe freezes | Same as `disconnect` | Same watchdog |
| OBS closed or crashes | Requests start failing | One reconnect, then abort the batch |
| dod-studio panics | — | Nothing. `panic = "abort"`, no unwind, no `Drop` |
| dod-studio force-killed, or power cut | — | Nothing |

**`disconnect` was the real gap.** Every guard in the capture loop keys off hl.exe being gone or a
file appearing, and `disconnect` produces neither: the process sits happily at the menu while the
batch is over. The signal that was missing is the one already being written — `BREADCRUMB` fires
every 5000 ticks throughout playback regardless of blocks, so its *absence* is the stall.

The threshold cannot be derived from ticks. Demo ticks are frames, so the wall-clock spacing of a
breadcrumb interval depends on the fps the demo was recorded at, and **an unfocused game stops
fast-forwarding entirely** (see `docs/goldsrc_dod_quirks.md`), which stretches a gap of seconds into
minutes with nothing wrong. Hence a five-minute floor plus an adaptive term of three times the
longest gap the batch has already survived — a batch that has demonstrated a four-minute gap is not
stalled at five. Tripping it kills the game and loses the batch, so it is biased towards waiting too
long.

**The last two rows cannot be fixed on the way out.** Release builds set `panic = "abort"`, so a
panic runs no destructor, and a force-quit or a power cut runs nothing at all. From outside the
process all three are the same event: dod-studio is gone and OBS is still recording. So the recovery
is on the way *back in* — `obs::recover` asks OBS at start-up whether it is recording into a folder
shaped like `<take>/take0000/all`, which is a path only this app produces, and offers to stop it and
fold the file. Anything else is somebody's own recording and is left alone.

Two smaller ones found on the way:

- **An OBS killed mid-recording truncates a plain MP4 — it does not destroy it.** Measured on OBS
  32.2.2, 2026-08-28, against a real killed recording: the salvaged file played, decoded cleanly up
  to the moment OBS died (4.2s of an 11s block), and reported only `partial file` with one broken
  audio frame at the tail. The received wisdom that an unfinalised MP4 is a total loss did not hold
  here. MKV and the hybrid/fragmented MP4 variants still lose nothing at all, so preflight
  recommends them — but as a preference, not a rescue. `fold_into_take` keeps whatever container OBS
  wrote either way.
- **A salvaged block's reported duration is wall-clock, not file length.** 11.0s reported against a
  4.2s file, because the seconds between the failure happening and being noticed are counted but not
  recorded. `RecordedBlock::salvaged` marks these; anything comparing a duration must check it.
- **Automatic file splitting silently truncates a block.** `StopRecord` reports only the last piece,
  so everything before it is stranded under a name nothing downstream resolves. Preflight warns.

---


## Open questions

- ~~**`qconsole.log` flush cadence.**~~ **Answered 2026-08-28.** 21–40 ms across a real 17-block
  batch. The `screenshot` fallback is not needed and was never built.
- **Frame pacing under real playback.** *(No issue.)* The delivery test was taken at the menu, where nothing is
  moving. Whether a demo playing at `host_framerate 0` on a loaded machine produces dropped or
  duplicated frames is a different question, and it is the one that decides how good this path
  actually looks. **Still open** — the captured clips decode clean and run the right duration, which
  says nothing about frames OBS silently duplicated to fill time.
- ~~**Which container.**~~ **Settled, and the worry was overstated.** A plain MP4 from an OBS killed
  mid-recording is *truncated, not destroyed*: measured, it played, decoded to the moment OBS died,
  and reported only `partial file` with one broken audio frame. So the container choice is a
  preference — MKV and the hybrid/fragmented variants lose nothing, plain MP4 loses the tail — not a
  correctness gate. What remains is that MP4 needs a second frame-count reader beside
  `avi_frame_count`, while Custom Output writing `utvideo` into an AVI reuses the existing one.
- **Is `stopsound` still wanted in the record window** *(no issue)* once nothing is being time-warped through it?
- **What happens when the machine cannot keep up.** Promoted from a nicety to the question that
  decides who this path is usable for — see the break-even table above: the machines where OBS saves
  the most wall-clock are the machines where OBS drops the most frames. Dropped frames are invisible
  in the output file's metadata: the duration is right and the frames are simply missing.
  `GetRecordStatus` does not report skipped frames; OBS's own stats do. Whether that is reachable
  over the WebSocket, and whether a batch should warn or refuse, is unanswered.
  Tracked partly by [#172](https://github.com/ccoventry/dod-studio/issues/172) ("live OBS drop rate").
- ~~**Whether OBS should be driven at all when it is already streaming.**~~ **Settled: it refuses.**
  `ObsSession::start` fails the batch on an active stream rather than warning, because the failure
  would be visible to the user's audience rather than to them. It refuses an already-running
  recording the same way.
- **The stall watchdog is the one failure path never triggered live** *(no issue)*. Everything else in the table
  above has been tested against a real batch. Reproducing this one means a demo that fails to load
  with hl.exe still alive, plus a five-minute wait; it did arm correctly off a real run in the
  activity log, which is weaker evidence than the rest.
- **Is `MIN_TAKE_SEPARATION_SECONDS` right?** The OBS path uses its own
  `OBS_TAKE_SEPARATION_SECONDS` (> 1.065 s, tested in `native/src/obs/client.rs`); the HLAE-side constant's
  calibration is [#9](https://github.com/ccoventry/dod-studio/issues/9).
- **Measure wall-clock and disk against a real batch** (staging item 7). *(No issue.)*
