// batch_resume.js
//
// Offers to capture the clips a batch never got to (#20). A batch that ran
// while Studio was closed is caught up on at the next start
// (batch_catch_up.js); when the game closed or crashed before the end, some
// of its clips have no take. This asks once whether to capture those now and,
// on yes, opens the batch's project if another one is open (its own
// unsaved-changes prompt first), ticks just those highlights and runs Start
// Capture Batch with all its usual checks.
//
// The batch's ids are all it needs: the manifest records which highlight
// each payload index is and the project it came from (#545). A batch with no
// project can only be resumed while its demos are in the open queue.

import { themedConfirm, isThemedConfirmOpen } from './themed_confirm.js';
import { streakUid } from './take_index.js';
import { showToast } from './toast.js';
import { switchNavTab, setCaptureDetailSubtab } from './nav.js';
import { STRINGS } from './strings.js';

/** The ids of the highlights in `batch`'s blocks that have no take. */
export function uncapturedUids(batch) {
  const uids = batch?.streak_uids || [];
  const left = (batch?.blocks || [])
    .filter((block) => !block.captured)
    .flatMap((block) => (block.source_streak_indices || []).map((i) => uids[i]))
    .filter(Boolean);
  return [...new Set(left)];
}

/** How many of `uids` are highlights in `demos`. */
export function countListed(demos, uids) {
  const wanted = new Set(uids);
  return (demos || []).reduce(
    (n, demo) => n + (demo.streaks || []).filter((s) => wanted.has(streakUid(demo.path, s))).length,
    0,
  );
}

/**
 * Ticks exactly the highlights in `demos` whose ids are in `uids` and unticks
 * every other one, in place, so Start Capture Batch captures just those.
 * Returns how many it ticked.
 */
export function applySelectOnly(demos, uids) {
  const wanted = new Set(uids);
  let ticked = 0;
  (demos || []).forEach((demo) => (demo.streaks || []).forEach((streak) => {
    streak.selected = wanted.has(streakUid(demo.path, streak));
    if (streak.selected) ticked += 1;
  }));
  return ticked;
}

function fileName(path) {
  return path.split(/[\\/]/).pop() || path;
}

/** How often the offer looks again while another dialog is open. */
const DIALOG_POLL_MS = 500;

const defaultIo = {
  confirm: themedConfirm,
  dialogOpen: isThemedConfirmOpen,
  wait: (ms) => new Promise((resolve) => { setTimeout(resolve, ms); }),
  toast: showToast,
  showHighlights: () => {
    switchNavTab('workspace');
    setCaptureDetailSubtab('highlights');
  },
};

/**
 * @param {object} app  What this needs: openProject(path) project_session's
 *   Open Recent route (resolves whether it opened), getSessionPath() the open
 *   project file or null, getDemos() the queue, onSelectionChange() to redraw
 *   the tables and mark the project unsaved, and startCapture() Start Capture
 *   Batch's click (capture_pane.js). `io` swaps the dialog and toasts out in
 *   tests.
 */
export function createBatchResume({ openProject, getSessionPath, getDemos, onSelectionChange, startCapture, io = defaultIo }) {
  /** The question, or null when there is nothing Studio could resume. */
  function question(batch, result, uids) {
    const S = STRINGS.BATCH_RESUME;
    const project = batch.project_path || null;
    if (!project && countListed(getDemos(), uids) === 0) return null;
    const left = result.total - result.captured;
    const where = project ? S.inProject(fileName(project)) : S.IN_QUEUE;
    return `${S.message(result.captured, result.total, left)}\n\n${where}`;
  }

  /**
   * Asks whether to capture `batch`'s clips that have no take, and starts
   * that batch on yes. `result` is what batch_catch_up counted
   * (`{ captured, total }`). Resolves once the batch has been handed to
   * Start Capture Batch, or the offer was declined or impossible.
   */
  async function offer(batch, result) {
    const uids = uncapturedUids(batch);
    if (uids.length === 0) return;
    const text = question(batch, result, uids);
    if (!text) return;
    const S = STRINGS.BATCH_RESUME;
    // The offer can come at any time (when the old batch's game closes): a
    // dialog the user has open is answered first, or it would be replaced
    // and its caller never hear back.
    while (io.dialogOpen?.()) await io.wait(DIALOG_POLL_MS);
    const yes = await io.confirm(text, { title: S.TITLE, confirmLabel: S.CAPTURE_BUTTON, cancelLabel: S.NOT_NOW_BUTTON });
    if (!yes) return;

    const project = batch.project_path || null;
    if (project && project !== getSessionPath() && !(await openProject(project))) return;
    if (applySelectOnly(getDemos(), uids) === 0) {
      io.toast(S.NONE_FOUND, 'error');
      return;
    }
    onSelectionChange();
    io.showHighlights();
    await startCapture();
  }

  return { offer };
}
