# Half-Life Advanced Effects (HLAE) Protocols

How the capture pipeline launches HLAE, ends a batch, and checks what HLAE wrote. Repo-wide rules (the "HLAE Game Capture" name, slash escaping for HLAE console paths, process polling and cancellation) live in `CLAUDE.md` and are not repeated here.

## Commands
- **GoldSrc HLAE takes the legacy command set**: `mirv_movie_filename`, `mirv_movie_fps`, `mirv_movie_ffmpeg`. CS:GO-era commands such as `mirv_streams` do not exist in AfxHookGoldSrc and fail as invalid console input.
- **The pipeline owns the recording commands.** Which commands a user may type into Initial or Scheduled Commands, and why, is set by `native::patch::cfg_scan`; see `docs/command_tiers.md`.

## Ending a batch
GoldSrc drops `quit` and `exec` from a demo's message stream, so a demo cannot end the game itself. A batch ends one of three ways:
- **The events pipe (normal route, #434).** The hook DLL reports `BATCH_COMPLETE` and Studio ends `hl.exe`.
- **The exit-trigger folder (fallback).** At the end of the schedule `mirv_movie_filename` points at `DOD_STUDIO_EXIT_TRIGGER` beside `hl.exe`, which HLAE creates as a directory. `capture_engine.rs` polls for it and ends `hl.exe` by pid.
- **No Studio watching (#545).** If no Studio is connected to the events pipe a few seconds after `BATCH_COMPLETE`, the hook quits the game itself (`goldsrc-hooks/src/batch_end.rs`).

## Recording blocks
- **Contiguous recording.** Rapid, sequential kills are fused into one continuous recording block. A block cannot be split across drives mid-recording, so the drive-reservation math must fit each whole block on one target drive.

## Launch & Handoff
- **The Launcher Exits Immediately — That Is Not a Crash.** `HLAE.exe -customLoader -autoStart` spawns, injects `AfxHookGoldSrc.dll` into `hl.exe`, and returns with status `0` about 2-3 seconds later. The orchestrator must treat the launcher's exit as a handoff and keep watching `hl.exe`, never take it as batch failure.
- **The Custom Loader Leaves the Working Directory at the Game Root.** `hl.exe` runs with its own folder as cwd, not the mod folder, so anything the engine writes relative to cwd — `qconsole.log` most notably — lands *beside the executable*, one level above `dod/`. Cleanup and log-reading code that assumes the mod folder finds nothing and says nothing. See `docs/goldsrc_dod_quirks.md`.

## Take Output & Verification
- **Layout.** A take lands at `<capture_dir>/<session_id>/chain_JJ_bN/take0000/`. The `takeNNNN` level is HLAE's own auto-numbering and is why `shared::paths::take_key` skips a trailing `take*` component — the block folder is the identity, not the take folder.
- **Frame-sequence mode writes BMP frames** into `all/`, one file per frame. A 40-second clip at 120fps is ~4,800 files and >13GB, so *never* enumerate them to answer a question about duration. Direct-to-video mode (`mirv_movie_ffmpeg`) writes a video file instead; see `docs/direct_to_video_capture.md`.
- **`sound.wav` is the cheap integrity check.** Audio is always a separate `sound.wav`, which the render pass muxes in. It is 22,050Hz 16-bit stereo — block align 4, byte rate 88,200 — so its duration is `(bytes - 44) / 88200` from the file size alone, no decoding and no directory walk. A take is internally consistent when that equals `frame_count / capture_fps`. Measured across all 15 takes of a verified batch, the two agree to two decimal places every time, which makes a mismatch a real signal: HLAE writes audio to match the frames it actually wrote, so audio and video cannot drift apart within a take — they can only both be short.
- **Do not compare a take's duration to its record window using an average tickrate.** The window is `record_stop_tick - record_start_tick` in frame records, and converting that to seconds with a demo-wide average produces several seconds of scatter on long clips in *both* directions — an artifact of the arithmetic, not of the capture. See the "Pure Float Timestamps" rule in `docs/app_architecture.md`.
