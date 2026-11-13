// batch_untick.js
//
// Unticks the highlights a capture batch captured, so the next Start Capture
// Batch only takes what is left. Run when a batch's takes are verified (the
// end of every batch: done, cancelled, or the game closed or crashed) and
// when a batch that ended while Studio was closed is caught up on
// (batch_catch_up.js). A clip the game reported short (#1410) stays ticked so
// the next batch records it again; a clip that failed or was never reached has
// no take, so it was never unticked. Only highlights in the open queue are
// touched.

import { streakUid } from './take_index.js';
import { logFrontendEvent } from './ipc_bridge.js';
import { STRINGS } from './strings.js';

/** The ids of the highlights `blocks` cover, through `batch.streak_uids`. */
export function blockUids(batch, blocks) {
  const uids = batch?.streak_uids || [];
  const covered = (blocks || []).flatMap((block) => (block.source_streak_indices || []).map((i) => uids[i]));
  return [...new Set(covered.filter(Boolean))];
}

/** Whether `block` produced a clip the game reported short (#1410). */
function isShortClip(block) {
  return Boolean(block.captured && block.clip_verdict?.short);
}

/**
 * Splits the highlights of `batch`'s captured blocks into `done` (captured
 * whole: untick) and `short` (captured but short: stay ticked). A highlight
 * in both, merged into one take with a short one, stays ticked.
 */
export function capturedUids(batch) {
  const blocks = batch?.blocks || [];
  const short = new Set(blockUids(batch, blocks.filter(isShortClip)));
  const done = blockUids(batch, blocks.filter((b) => b.captured && !isShortClip(b))).filter((uid) => !short.has(uid));
  return { done, short: [...short] };
}

/**
 * Unticks the highlights in `demos` whose ids are in `uids`, in place.
 * Returns how many were ticked and are now not.
 */
export function untickUids(demos, uids) {
  const wanted = new Set(uids);
  let unticked = 0;
  (demos || []).forEach((demo) => (demo.streaks || []).forEach((streak) => {
    if (!streak.selected || !wanted.has(streakUid(demo.path, streak))) return;
    streak.selected = false;
    unticked += 1;
  }));
  return unticked;
}

/**
 * @param {object} app  getDemos() the open queue; onSelectionChange() redraws
 *   the tables and marks the project unsaved. `log` swaps the activity log
 *   out in tests.
 */
export function createBatchUntick({ getDemos, onSelectionChange, log = logFrontendEvent }) {
  /**
   * Unticks `batch`'s captured highlights in the open queue, redraws when any
   * changed and logs the counts. `batch` is `{ streak_uids, blocks }`.
   * Returns how many it unticked.
   */
  function apply(batch) {
    const { done, short } = capturedUids(batch);
    if (done.length === 0) return 0;
    const unticked = untickUids(getDemos(), done);
    if (unticked > 0) onSelectionChange();
    Promise.resolve(log(STRINGS.BATCH_UNTICK.logLine(unticked, short.length))).catch(() => {});
    return unticked;
  }

  return { apply };
}
