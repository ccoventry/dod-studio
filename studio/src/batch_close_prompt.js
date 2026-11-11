// batch_close_prompt.js
// Asks before DoD Studio closes while a capture batch is running (#545,
// part 1). The game keeps capturing without Studio -- the schedule is already
// in the patched demos, or the hook runs it -- but nothing checks the takes
// at the end until Studio next opens. A local `tauri dev` build starts the game
// outside cargo's job object, so it survives that Studio closing too
// (`native::sys::launcher`, #545 part 4). Stopping the batch stays Cancel
// Batch's job.
//
// The game only keeps capturing once the batch's start has reached it: after
// Start, Studio launches the game, waits for the hook's pipe and sends the
// start about 5 s later. Closed before that, the game would sit at its main
// menu. So a close in that window waits for the start first (bounded), and if
// it never gets there, says the batch has not started and cancels it.

import { themedConfirm, dismissThemedConfirm, setThemedConfirmMessage } from './themed_confirm.js';
import { waitForBatchStart, cancelCaptureBatch } from './ipc_bridge.js';
import { STRINGS } from './strings.js';

/** How long a close waits for the batch's start to reach the game. The
 *  pipe opens early in the game's start-up, so this clears a slow one. */
export const START_WAIT_MS = 30000;
/** How long a close waits for a cancelled batch to stop. */
export const CANCEL_WAIT_MS = 15000;

/** Whether a start state from `waitForStart` means the game runs the batch
 *  without Studio. */
export function batchRunsWithoutStudio(state) {
  return state === 'sent' || state === 'not_needed';
}

const KEPT_OPEN = Symbol('kept open');

/**
 * Shows a wait in the themed dialog while `work` runs, counting down from
 * `ms`. Resolves to `work`'s result, or `KEPT_OPEN` when the user picks Keep
 * Studio open first (only offered with `canKeepOpen`).
 */
async function whileWaiting(work, { ms, message, canKeepOpen }) {
  const s = STRINGS.BATCH_CLOSE_MODAL;
  const began = Date.now();
  const text = () => (typeof message === 'function'
    ? message(Math.max(0, Math.ceil((ms - (Date.now() - began)) / 1000)))
    : message);
  const clicked = themedConfirm(text(), {
    title: s.WAIT_TITLE,
    cancelLabel: s.KEEP_OPEN_BUTTON,
    hideConfirm: true,
    hideCancel: !canKeepOpen,
  }).then(() => KEPT_OPEN);
  const tick = setInterval(() => setThemedConfirmMessage(text()), 1000);
  try {
    return await Promise.race([clicked, work()]);
  } finally {
    clearInterval(tick);
    dismissThemedConfirm(false);
  }
}

/** Resolves once `isRunning()` is false, or after `ms`. */
async function untilStopped(isRunning, ms) {
  const began = Date.now();
  while (isRunning() && Date.now() - began < ms) {
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

/**
 * Whether the window may close. True at once when no batch is running;
 * otherwise asks, and only "Close DoD Studio" (or, before the batch has
 * started in the game, "Cancel batch and close") says yes.
 *
 * `waitForStart(ms)` resolves to where the batch's start stands once it is
 * no longer on its way to the game, or `ms` has passed (ipc_bridge's
 * waitForBatchStart). `cancelBatch()` asks the running batch to stop.
 *
 * A second close while the first still asks or waits (the title bar's X
 * clicked again) answers no at once and opens nothing: the dialog is one
 * element, and a second one over it took the first's answer, so the end of
 * one wait dismissed the other's question and Studio stayed open.
 */
export async function confirmCloseDuringBatch(args) {
  if (!args.isRunning()) return true;
  if (asking) return false;
  asking = true;
  try {
    return await askBeforeClose(args);
  } finally {
    asking = false;
  }
}

let asking = false;

async function askBeforeClose({
  isRunning, waitForStart = waitForBatchStart, cancelBatch = cancelCaptureBatch,
}) {
  const s = STRINGS.BATCH_CLOSE_MODAL;

  // Where the start stands now; only a start still on its way is waited for.
  // If Studio can't tell, it asks as it always did rather than cancel a
  // batch that may well be running.
  let state = await waitForStart(0).catch(() => 'sent');
  if (state === 'pending') {
    const waited = await whileWaiting(
      () => waitForStart(START_WAIT_MS).catch(() => 'pending'),
      { ms: START_WAIT_MS, message: s.starting, canKeepOpen: true },
    );
    if (waited === KEPT_OPEN) return false;
    state = waited;
  }
  if (state === 'not_running' || !isRunning()) return true;

  if (batchRunsWithoutStudio(state)) {
    return themedConfirm(s.MESSAGE, {
      title: s.TITLE,
      confirmLabel: s.CLOSE_BUTTON,
      cancelLabel: s.KEEP_OPEN_BUTTON,
    });
  }

  const close = await themedConfirm(s.NOT_STARTED_MESSAGE, {
    title: s.NOT_STARTED_TITLE,
    confirmLabel: s.CANCEL_AND_CLOSE_BUTTON,
    cancelLabel: s.KEEP_OPEN_BUTTON,
    // Red: yes cancels the batch, so its captures never happen (#880).
    destructive: true,
  });
  if (!close) return false;
  // Cancelled, the batch closes the game and records itself as cancelled,
  // so the next start has nothing to catch up on.
  await whileWaiting(async () => {
    await cancelBatch().catch(() => {});
    await untilStopped(isRunning, CANCEL_WAIT_MS);
  }, { ms: CANCEL_WAIT_MS, message: s.CANCELLING, canKeepOpen: false });
  return true;
}
