// batch_close_prompt.js
// Asks before DoD Studio closes while a capture batch is running (#545,
// part 1). The game keeps capturing without Studio -- the schedule is already
// in the patched demos -- but nothing checks the takes or closes the game at
// the end, and a local `tauri dev` build takes the game down with it (cargo's
// job object). Stopping the batch stays Cancel Batch's job.

import { themedConfirm } from './themed_confirm.js';
import { STRINGS } from './strings.js';

/** The prompt's text: the note about local builds only where it applies. */
export function batchCloseMessage(isLocalBuild) {
  const s = STRINGS.BATCH_CLOSE_MODAL;
  return isLocalBuild ? `${s.MESSAGE}\n\n${s.LOCAL_BUILD_NOTE}` : s.MESSAGE;
}

/**
 * Whether the window may close. True at once when no batch is running;
 * otherwise asks, and only "Close DoD Studio" says yes.
 */
export async function confirmCloseDuringBatch({ isRunning, isLocalBuild }) {
  if (!isRunning()) return true;
  const s = STRINGS.BATCH_CLOSE_MODAL;
  return themedConfirm(batchCloseMessage(await isLocalBuild()), {
    title: s.TITLE,
    confirmLabel: s.CLOSE_BUTTON,
    cancelLabel: s.KEEP_OPEN_BUTTON,
  });
}
