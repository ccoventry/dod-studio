// takes_shortfall_toast.js
// The summary toast for a batch whose takes aren't all on disk, from
// capture_takes_verified. Each clip without a captured take falls into one
// bucket: cut short by the batch's end (#1081), video that stops early
// (#1105), OBS started late (#1109), failed in the game (#1401), never reached
// because the batch ended first (#1434), or missing. Only a real problem makes
// it an error toast: a user's Cancel that cut one clip and left the rest
// unreached is what they asked for, so it is an info toast.

import { STRINGS } from './strings.js';
import { failedClipCount } from './clip_verdict_toasts.js';

/**
 * How many of `total` clips fall into each bucket when `captured` have their
 * take, and what ended the batch.
 */
export function takeShortfall(payload, captured, total) {
  const blocks = payload?.blocks || [];
  // A take whose video stopped early (#1105) is cut_short too, but the
  // batch's end isn't why. The end cuts one clip at most: its `cut_by`.
  const cut = blocks.filter((b) => b.cut_short && !b.short_take);
  const counts = {
    cutShort: cut.length,
    cutBy: cut[0]?.cut_by,
    stoppedEarly: blocks.filter((b) => b.short_take).length,
    // OBS started recording after the clip did (#1109).
    startedLate: blocks.filter((b) => b.started_late_ms && !b.cut_short).length,
    failed: failedClipCount(payload),
    notReached: blocks.filter((b) => b.not_reached).length,
    endedBy: payload?.ended_by,
  };
  counts.missing = total - captured - counts.cutShort - counts.stoppedEarly
    - counts.startedLate - counts.failed - counts.notReached;
  return counts;
}

/** Whether `by` (a `cut_by` or `ended_by`) is the user's own Cancel. */
const isCancel = (by) => (by ?? 'cancel') === 'cancel';

/**
 * The toast for a batch with `captured` of `total` takes on disk: its text
 * (without the "marked Captured" tail) and 'error' or 'info'.
 */
export function takesShortfallToast(payload, captured, total) {
  const s = takeShortfall(payload, captured, total);
  const problem = s.missing > 0 || s.stoppedEarly > 0 || s.startedLate > 0 || s.failed > 0
    || (s.cutShort > 0 && !isCancel(s.cutBy))
    || (s.notReached > 0 && !isCancel(s.endedBy));
  return { text: STRINGS.CAPTURE.takesFoundMissing(captured, total, s), type: problem ? 'error' : 'info' };
}
