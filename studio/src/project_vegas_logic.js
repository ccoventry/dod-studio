// project_vegas_logic.js
//
// The pure part of the whole-project VEGAS export (#1264): which of the
// project's rendered videos go in, and how they become EDL clips.

import { streakUid } from './take_index.js';
import { captureOf } from './take_record.js';
import { fileNameOf } from './path_display.js';

/**
 * How a recorded render's `stream` starts when it was a HUD-only pass. The
 * project records the take folder's history (`attempt_stream` in native's
 * take_cleanup.rs: `hud:<folder>`), not the Render tab's job label that
 * vegas_edl.js reads (`HUD ONLY`).
 */
const HUD_STREAM_PREFIX = 'hud:';

/** The uids of the highlights ticked in `demos`. */
export function tickedUids(demos) {
  const uids = new Set();
  (demos || []).forEach((demo) => {
    (demo.streaks || []).forEach((streak) => {
      if (streak.selected) uids.add(streakUid(demo.path, streak));
    });
  });
  return uids;
}

/**
 * Which videos to export. When ticked highlights have videos, only those
 * (`scope: 'ticked'`); otherwise every video of the project (`'all'`). A take
 * holding several highlights counts when any of them is ticked.
 * @param {Array<{uids: string[]}>} videos  `projectRenderedVideos` order
 * @returns {{ videos: object[], scope: 'ticked'|'all' }}
 */
export function videosToExport(videos, demos) {
  const ticked = tickedUids(demos);
  if (ticked.size > 0) {
    const chosen = videos.filter((video) => (video.uids || []).some((uid) => ticked.has(uid)));
    if (chosen.length > 0) return { videos: chosen, scope: 'ticked' };
  }
  return { videos, scope: 'all' };
}

/**
 * EDL clips (the shape `vegasEdl` reads) for `videos`, in their order.
 * `lengths` maps a path to its length in seconds; a video without one is
 * counted in `skipped`.
 */
export function clipsOfVideos(videos, takeIndex, lengths) {
  const clips = [];
  let skipped = 0;
  videos.forEach((video) => {
    const seconds = lengths.get(video.output_path);
    if (!(seconds > 0)) { skipped += 1; return; }
    clips.push({
      name: fileNameOf(video.output_path),
      path: video.output_path,
      seconds,
      take_key: video.takeKey || '',
      hud: String(video.stream || '').startsWith(HUD_STREAM_PREFIX),
      takeFolder: captureOf(takeIndex, video.takeKey)?.take_folder || '',
    });
  });
  return { clips, skipped };
}
