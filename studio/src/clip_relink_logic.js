// clip_relink_logic.js
//
// The pure part of Find Moved Videos (#1442): what the backend is asked to
// find, how its report updates the project's record of its renders
// (take_renders.js), and what the bar and the toast say.

import { STRINGS } from './strings.js';
import { relocateRender } from './take_renders.js';

/**
 * The backend request for `videos` (`projectRenderedVideos` order): each
 * render's tag parts (take key, start, stream) and what the project knows of
 * its file.
 */
export function clipsToFind(videos) {
  return (videos || []).map((video) => ({
    take_key: video.takeKey,
    started_unix_ms: video.started_unix_ms,
    stream: video.stream,
    output_path: video.output_path || '',
    output_size_bytes: Number.isFinite(video.output_size_bytes) ? video.output_size_bytes : null,
    content_hash: video.content_hash || null,
  }));
}

/**
 * Writes `report` (same order as `videos`) into the project's record: each
 * found video's path, size and fingerprint. Returns `{ moved, notFound,
 * changed }`: videos found somewhere new, videos not found anywhere, and
 * records that changed (a fingerprint taken in place counts, so the project
 * keeps it).
 */
export function applyRelinkReport(takeIndex, videos, report) {
  const result = { moved: 0, notFound: 0, changed: 0 };
  (videos || []).forEach((video, i) => {
    const found = report?.clips?.[i];
    if (!found || found.found_by === 'missing' || !found.path) {
      result.notFound += 1;
      return;
    }
    if (found.found_by !== 'in_place') result.moved += 1;
    const changed = relocateRender(takeIndex, video.takeKey, video, {
      output_path: found.path,
      output_size_bytes: found.size_bytes,
      content_hash: found.content_hash,
    });
    if (changed) result.changed += 1;
  });
  return result;
}

/** The bar and line for a `clip_relink_progress` event. */
export function relinkProgressView(event) {
  const done = Math.max(0, Number(event?.done) || 0);
  const total = Math.max(0, Number(event?.total) || 0);
  const pct = total > 0 ? Math.round((Math.min(done, total) / total) * 100) : 0;
  if (event?.step === 'listing') {
    return { pct: 0, indeterminate: true, text: STRINGS.RENDER.findMovedListing(done) };
  }
  const n = Math.min(done + 1, total);
  if (event?.step === 'checking') {
    return { pct, indeterminate: false, text: STRINGS.RENDER.findMovedChecking(n, total) };
  }
  return { pct, indeterminate: false, text: STRINGS.RENDER.findMovedTagging(n, total) };
}

/** The activity-log line for what the scan changed in the project. */
export function relinkAppliedLine({ moved, notFound, changed }, total) {
  return `[clip-relink] project updated: ${moved} of ${total} video(s) found in a new place, ${notFound} not found, ${changed} record(s) changed`;
}
