// capture_estimate.js
// The capture disk-space estimate, kept free of the DOM and Tauri so it can
// be unit tested.

/**
 * A highlight's first and last selected kill, in seconds into the demo.
 *
 * From the kill times (`kills[i][1]`), not `start_tick / demo_fps`:
 * start_tick/end_tick are frame-record indices, and records are not evenly
 * spaced, so dividing by the tickrate drifts further off the later the
 * highlight is (#464). Falls back to that arithmetic only for a streak with
 * no kills.
 */
export function streakSeconds(streak) {
  const kills = streak.kills || [];
  if (kills.length === 0) {
    const fps = streak.demo_fps || 100;
    return [streak.start_tick / fps, streak.end_tick / fps];
  }
  const last = kills.length - 1;
  const end = Math.min(streak.end_index ?? last, last);
  const start = Math.min(streak.start_index ?? 0, end);
  return [kills[start][1], kills[end][1]];
}

/** A highlight's selected kills' times, in seconds into the demo. */
export function streakKillSeconds(streak) {
  const kills = streak.kills || [];
  if (kills.length === 0) return [];
  const last = kills.length - 1;
  const end = Math.min(streak.end_index ?? last, last);
  const start = Math.min(streak.start_index ?? 0, end);
  return kills.slice(start, end + 1).map((k) => k[1]);
}

/** Where a fast-forwarded gap starts after its first kill and ends before
 *  the next, in seconds: `builder::kill_gaps`' AFTER_KILL_SECONDS and
 *  BEFORE_KILL_SECONDS (Review's own margins, #665). */
export const RAMP_AFTER_KILL_SECONDS = 2;
export const RAMP_BEFORE_KILL_SECONDS = 4;

/**
 * Seconds of take a clip saves by playing its long gaps between kills sped
 * up (#831): for each gap over `overSeconds`, the stretch between the two
 * margins records at `1 / speed` of its length. `killGaps` null (gaps kept)
 * saves nothing.
 */
export function rampSavedSeconds(killSeconds, killGaps) {
  if (!killGaps || !(killGaps.speed > 1) || !(killGaps.overSeconds > 0)) return 0;
  const sorted = [...killSeconds].sort((a, b) => a - b);
  let saved = 0;
  for (let i = 1; i < sorted.length; i++) {
    const gap = sorted[i] - sorted[i - 1];
    if (gap <= killGaps.overSeconds) continue;
    const ramp = gap - RAMP_AFTER_KILL_SECONDS - RAMP_BEFORE_KILL_SECONDS;
    if (ramp > 0) saved += ramp * (1 - 1 / killGaps.speed);
  }
  return saved;
}

/**
 * Sums required capture bytes across every selected streak, merging
 * overlapping (or touching) pre/post-roll windows *within each source demo*
 * before billing them for disk space — two highlights that share footage
 * must not be double-counted, since the engine records that overlap once.
 * Base cost is `w * h * 3` bytes/frame at the configured capture FPS, unless
 * `bytesPerFrame` is given — AGR mode passes its own much smaller figure
 * (`AGR_BYTES_PER_FRAME`, mirroring `sys::disk::AGR_BYTES_PER_FRAME`). With
 * `bytesPerSecond` the take is billed by its length instead (OBS, #1096).
 *
 * With `killGaps` (`{ overSeconds, speed }`, Fast-forward gaps between kills
 * on a Seek Between Clips batch, #831), each take is billed less the
 * stretches its long gaps record sped up ([`rampSavedSeconds`]).
 *
 * Does not account for `mirv_movie_separate_hud 1` typed into Initial
 * Commands — that triples the real cost (HUD pass recorded as its own
 * stream), but there is no longer a dedicated setting to read it from, and
 * this does not parse Initial Commands text to find it.
 */
export function computeRequiredCaptureBytes(currentScannedDemos, opts) {
  const {
    preRollSeconds, postRollSeconds,
    recordStartLead, recordStopTrail,
    captureFps, resWidth, resHeight, bytesPerFrame, bytesPerSecond, killGaps,
  } = opts;
  let totalSeconds = 0;

  (currentScannedDemos || []).forEach(demo => {
    const intervals = (demo.streaks || [])
      // Opt-in model (detail_pane.js): a streak counts as selected only once
      // explicitly checked. `undefined` covers both demos never opened in the
      // Highlight Details view and every non-recording-player streak (which
      // never renders as a checkable row at all) — neither should ever be
      // billed for capture space.
      .filter(streak => streak.selected === true)
      .map((streak) => [...streakSeconds(streak), streakKillSeconds(streak)])
      .sort((a, b) => a[0] - b[0]);

    // Two different windows are at play, and mixing them up is what this used
    // to get wrong:
    //  - whether two highlights collapse into ONE take is decided by
    //    pre/post-roll (native/src/patch/builder.rs's blocks_merge), and
    //  - how many frames actually get written is start-lead -> stop-trail
    //    (PatcherConfig::calculate_total_capture_duration).
    // So merge on the roll window, then bill the lead/trail window.
    let mergedStart = null;
    let mergedEnd = null;
    let mergedKills = [];
    const bill = () => {
      totalSeconds += recordStartLead + (mergedEnd - mergedStart) + recordStopTrail
        - rampSavedSeconds(mergedKills, killGaps);
    };
    intervals.forEach(([start, end, kills]) => {
      if (mergedStart === null) {
        mergedStart = start;
        mergedEnd = end;
        mergedKills = [...kills];
      } else if (start - preRollSeconds <= mergedEnd + postRollSeconds) {
        mergedEnd = Math.max(mergedEnd, end);
        mergedKills.push(...kills);
      } else {
        bill();
        mergedStart = start;
        mergedEnd = end;
        mergedKills = [...kills];
      }
    });
    if (mergedStart !== null) {
      bill();
    }
  });

  // A take billed by time, not frames (OBS, #1096): whole seconds, as the
  // backend bills each block.
  if (bytesPerSecond != null) return Math.ceil(bytesPerSecond * Math.ceil(Math.max(0, totalSeconds)));
  const frames = Math.ceil(Math.max(0, totalSeconds) * captureFps);
  return frames * (bytesPerFrame ?? resWidth * resHeight * 3);
}

/** What one recorded AGR frame costs on disk — see `sys::disk::AGR_BYTES_PER_FRAME`. */
export const AGR_BYTES_PER_FRAME = 8 * 1024;

/** Bytes a pixel of a lossless video take — see `sys::disk::LOSSLESS_VIDEO_BYTES_PER_PIXEL` (#1096). */
export const LOSSLESS_VIDEO_BYTES_PER_PIXEL = 2;

/**
 * What one frame is billed at for a capture mode and codec, or `undefined`
 * for raw frames (3 bytes a pixel): AGR's own figure, and a compressed
 * direct-to-video take at two thirds of raw. Mirrors the backend's
 * `block_byte_estimate`.
 */
export function bytesPerFrameFor(mode, codec, width, height) {
  if (mode === 'agr') return AGR_BYTES_PER_FRAME;
  if (mode === 'direct_to_video' && codec !== 'rawvideo') return width * height * LOSSLESS_VIDEO_BYTES_PER_PIXEL;
  return undefined;
}

/**
 * MB (10^6 bytes) a minute an OBS take is billed at, per OBS Quality: the
 * largest take measured in #1060 at 2560x1440 and 120 fps plus ~20% margin.
 * Mirrors `sys::disk::obs_mb_per_minute`.
 */
export const OBS_MB_PER_MINUTE = {
  small: 2300, standard: 3800, high: 5700, max: 6900, obs_simple: 4000,
};

/**
 * Bytes a second of an OBS take is billed at: the quality's rate, scaled up
 * (never down) for a picture bigger than 2560x1440 or a capture faster than
 * 120 fps. Mirrors `sys::disk::calculate_obs_bytes`, which the pinned
 * figures in both test suites keep in step.
 */
export function obsBytesPerSecond(quality, width, height, obsFps) {
  const perMinute = OBS_MB_PER_MINUTE[quality] ?? OBS_MB_PER_MINUTE.standard;
  const pixels = Math.max(0, width) * Math.max(0, height) / (2560 * 1440);
  const rate = Math.max(0, obsFps) / 120;
  return perMinute * 1_000_000 / 60 * Math.max(1, pixels) * Math.max(1, rate);
}
