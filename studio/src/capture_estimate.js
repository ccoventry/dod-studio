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

/**
 * Sums required capture bytes across every selected streak, merging
 * overlapping (or touching) pre/post-roll windows *within each source demo*
 * before billing them for disk space — two highlights that share footage
 * must not be double-counted, since the engine records that overlap once.
 * Base cost is `w * h * 3` bytes/frame at the configured capture FPS, unless
 * `bytesPerFrame` is given — AGR mode passes its own much smaller figure
 * (`AGR_BYTES_PER_FRAME`, mirroring `sys::disk::AGR_BYTES_PER_FRAME`).
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
    captureFps, resWidth, resHeight, bytesPerFrame,
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
      .map(streakSeconds)
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
    const bill = () => {
      totalSeconds += recordStartLead + (mergedEnd - mergedStart) + recordStopTrail;
    };
    intervals.forEach(([start, end]) => {
      if (mergedStart === null) {
        mergedStart = start;
        mergedEnd = end;
      } else if (start - preRollSeconds <= mergedEnd + postRollSeconds) {
        mergedEnd = Math.max(mergedEnd, end);
      } else {
        bill();
        mergedStart = start;
        mergedEnd = end;
      }
    });
    if (mergedStart !== null) {
      bill();
    }
  });

  const frames = Math.ceil(Math.max(0, totalSeconds) * captureFps);
  return frames * (bytesPerFrame ?? resWidth * resHeight * 3);
}

/** What one recorded AGR frame costs on disk — see `sys::disk::AGR_BYTES_PER_FRAME`. */
export const AGR_BYTES_PER_FRAME = 8 * 1024;
