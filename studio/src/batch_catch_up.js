// batch_catch_up.js
//
// Marks the highlights of a capture batch that ran while Studio was closed
// (#545). The backend (`batch_catch_up.rs`) checks such a batch's takes on
// disk when Studio starts, or once the game closes if one is still running;
// this maps each take back to its highlights through the ids the batch's
// manifest recorded and marks them Captured, as capture_pane.js does at the
// end of a batch it watched. A take Render While Capturing rendered and
// deleted before Studio closed (#861: the backend's `rendered`) is marked
// Rendered: its clip is done, and that mark was lost with the unsaved project.
//
// A batch started from a project file is marked in that file, unless it is
// the project open right now: then it is marked in the open queue and saved
// with it, unless the project already had unsaved edits, which stay the
// user's to save (#1344). A batch with no project is
// marked in the open queue, which matches only if those demos are loaded.

import { listen } from '@tauri-apps/api/event';
import { catchUpUnwatchedBatches, filesExist, readProjectFile, writeTextFile } from './ipc_bridge.js';
import { HIGHLIGHT_STATUS } from './status_colors.js';
import { streakUid, recordTake, setVerifiedStatus } from './take_index.js';
import { captureRateOf, attachCaptures, capturesToSave } from './take_record.js';
import { openProjectText, buildProjectFile, saveProjectFile } from './project_file.js';
import { updateSummary } from './project_migrations.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import { fileNameOf } from './path_display.js';
import { createBatchResume } from './batch_resume.js';

/** The event the backend sends once a running game has closed. */
export const CATCH_UP_EVENT = 'batch_catch_up';

/** How long the "still running in the game" toast stays up. */
const WAITING_TOAST_MS = 10000;

// Set while the backend waits for a running game to close before checking a
// batch from before Studio closed. Module state: capture_pane.js reads it.
let waitingForGame = false;

/** Whether a batch from before Studio closed may still be capturing in an
 *  open game, unchecked: Start Capture Batch refuses until it has been. */
export function isBatchStillRunningInGame() {
  return waitingForGame;
}

/**
 * Each captured block of `batch` as `{ takeKey, uids, meta, rendered }`, the
 * highlights it covers and whether its clip was rendered. A block can cover several highlights (overlapping ones are
 * recorded as one take).
 */
export function capturedTakes(batch) {
  const uids = batch?.streak_uids || [];
  return (batch?.blocks || [])
    .filter((block) => block.captured)
    .map((block) => ({
      takeKey: block.take_key,
      meta: { session_id: batch.session_id, capture_mode: batch.capture_mode, capture_fps: captureRateOf(batch.capture_mode, batch), take_folder: block.take_folder },
      uids: (block.source_streak_indices || []).map((i) => uids[i]).filter(Boolean),
      rendered: block.rendered === true,
    }))
    .filter((take) => take.uids.length > 0);
}

/**
 * Marks `batch`'s captured highlights in `demos` and records its takes in
 * `takeIndex`, both in place. Status only moves forward, with the same rule
 * as a watched batch: a hand-set Rendered is overruled, a verified one is
 * kept. Returns `{ captured, total, marked, markedRendered, changed }`.
 */
export function applyCaughtUpBatch(batch, demos, takeIndex) {
  const takes = capturedTakes(batch);
  // Each highlight's status from its take: Rendered when that take's clip
  // was finished, else Captured.
  const wanted = new Map();
  takes.forEach(({ takeKey, uids, meta, rendered }) => {
    if (takeKey) recordTake(takeIndex, takeKey, uids, meta);
    const status = rendered ? HIGHLIGHT_STATUS.RENDERED : HIGHLIGHT_STATUS.CAPTURED;
    uids.forEach((uid) => { if (wanted.get(uid) !== HIGHLIGHT_STATUS.RENDERED) wanted.set(uid, status); });
  });

  let marked = 0;
  let markedRendered = 0;
  let changed = takes.length > 0;
  (demos || []).forEach((demo) => {
    (demo.streaks || []).forEach((streak) => {
      const status = wanted.get(streakUid(demo.path, streak));
      if (!status) return;
      if (streak.status === HIGHLIGHT_STATUS.RENDERED && !streak.statusByHand) return;
      if (streak.statusByHand) changed = true;
      if (setVerifiedStatus(streak, status)) {
        marked += 1;
        if (status === HIGHLIGHT_STATUS.RENDERED) markedRendered += 1;
      }
    });
  });

  const total = (batch?.blocks || []).length;
  const captured = (batch?.blocks || []).filter((b) => b.captured).length;
  return { captured, total, marked, markedRendered, changed: changed || marked > 0 };
}

function toastKind(result) {
  return result.captured < result.total ? 'info' : 'success';
}

const defaultIo = {
  catchUp: catchUpUnwatchedBatches,
  readProject: readProjectFile,
  writeProject: writeTextFile,
  exists: filesExist,
  listen,
  toast: showToast,
};

/**
 * @param {object} app  What this needs from main.js: getDemos() the queue,
 *   getTakeIndex(), getSessionPath() the open project file or null,
 *   onStatusChange() to re-render and mark the project unsaved, and for the
 *   offer to capture the rest (batch_resume.js) openProject(path) and
 *   startCapture(). saveIfOnlyChange(change) from project_session.js saves
 *   the open project when the marks are its only change. `io` swaps the IPC
 *   and toasts out in tests, `resume` the offer.
 */
export function createBatchCatchUp({
  getDemos, getTakeIndex, getSessionPath, onStatusChange, openProject, startCapture, io = defaultIo,
  saveIfOnlyChange = async (change) => ({ result: await change(), saved: false }),
  resume = createBatchResume({ openProject, getSessionPath, getDemos, onSelectionChange: onStatusChange, startCapture }),
}) {
  /** Marks `batch` in the project file it came from. Returns a note for the toast. */
  async function applyToProjectFile(batch, path) {
    try {
      const opened = openProjectText(await io.readProject(path));
      const { data } = opened;
      data.takeIndex = data.takeIndex || {};
      attachCaptures(data.takeIndex, data.takeCaptures);
      const result = applyCaughtUpBatch(batch, data.demos || [], data.takeIndex);
      if (!result.changed) return { result, note: '' };
      data.takeCaptures = capturesToSave(data.takeIndex);
      const raised = await saveProjectFile(path, buildProjectFile(data, opened),
        { read: io.readProject, write: io.writeProject, exists: io.exists });
      const note = ` ${STRINGS.BATCH_CATCH_UP.projectUpdated(fileNameOf(path) || path)}`;
      if (!raised) return { result, note };
      const summary = await updateSummary(data.demos, data.takeIndex);
      return { result, note: `${note} ${STRINGS.PROJECT_FILE.updatedToast(summary, raised.backup)}` };
    } catch (err) {
      return {
        result: applyCaughtUpBatch(batch, [], {}),
        note: ` ${STRINGS.BATCH_CATCH_UP.projectNotUpdated(fileNameOf(path) || path, err?.message || err)}`,
        failed: true,
      };
    }
  }

  /** Marks `batch` and toasts what it found. Resolves its counts, or null
   *  when its project file could not be updated. */
  async function applyBatch(batch) {
    const project = batch.project_path;
    if (project && project !== getSessionPath()) {
      const { result, note, failed } = await applyToProjectFile(batch, project);
      io.toast(`${STRINGS.BATCH_CATCH_UP.caughtUp(result.captured, result.total, result.marked, result.markedRendered)}${note}`,
        failed ? 'error' : toastKind(result));
      return failed ? null : result;
    }
    const { result, saved } = await saveIfOnlyChange(() => {
      const marks = applyCaughtUpBatch(batch, getDemos(), getTakeIndex());
      if (marks.changed) onStatusChange();
      return marks;
    }, { madeChange: (marks) => marks.changed });
    const note = saved ? ` ${STRINGS.BATCH_CATCH_UP.SAVED_OPEN_PROJECT}` : '';
    io.toast(`${STRINGS.BATCH_CATCH_UP.caughtUp(result.captured, result.total, result.marked, result.markedRendered)}${note}`, toastKind(result));
    return result;
  }

  /** Marks every batch, then offers to capture what the newest one with
   *  clips missing never got to: one question, not one per batch. */
  async function applyBatches(batches) {
    let unfinished = null;
    for (const batch of batches || []) {
      const result = await applyBatch(batch);
      if (result && result.captured < result.total) unfinished = { batch, result };
    }
    if (unfinished) await resume.offer(unfinished.batch, unfinished.result);
  }

  /** Run once at startup, before any batch of this Studio's own. */
  async function start() {
    // Listening before asking: the game can close between the backend's look
    // and its reply, so the event may come first, and then there is nothing
    // left to wait for.
    let checkedAfterGame = false;
    // Unheard, the end of the wait would never clear the refusal, so Start
    // is only held while the event can arrive.
    const listening = await io.listen(CATCH_UP_EVENT, (event) => {
      checkedAfterGame = true;
      waitingForGame = false;
      return applyBatches(event.payload);
    }).then(() => true, (err) => {
      console.error(`Failed to register ${CATCH_UP_EVENT} listener:`, err);
      return false;
    });
    try {
      const found = await io.catchUp();
      if (found?.waiting_for_game && listening && !checkedAfterGame) {
        waitingForGame = true;
        io.toast(STRINGS.BATCH_CATCH_UP.WAITING_FOR_GAME, 'info', WAITING_TOAST_MS);
      }
      await applyBatches(found?.batches);
    } catch (err) {
      io.toast(STRINGS.BATCH_CATCH_UP.checkFailed(err), 'error');
    }
  }

  return { start, applyBatches };
}
