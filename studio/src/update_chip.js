// update_chip.js
// The menu bar's update button (#868): beside Help, it says an update is
// out, or downloaded and waiting for a restart, without opening a menu.
// Clicking it opens the update window. updater_pane.js drives it.

import { STRINGS } from './strings.js';

/**
 * What the button shows: hidden with no update, "Update 0.2.1 available"
 * once a check finds one, "Restart to update" once it has downloaded, or
 * "Restart after the batch" while a capture batch runs.
 * `state` is `{ version: string|null, ready: boolean, batchRunning: boolean }`.
 */
export function updateChipView({ version = null, ready = false, batchRunning = false } = {}) {
  const S = STRINGS.UPDATE_CHIP;
  if (ready && batchRunning) return { hidden: false, text: S.RESTART_AFTER_BATCH, title: S.RESTART_AFTER_BATCH_TITLE };
  if (ready) return { hidden: false, text: S.READY, title: S.READY_TITLE };
  if (version) return { hidden: false, text: S.available(version), title: S.AVAILABLE_TITLE };
  return { hidden: true, text: '', title: '' };
}

/** Shows `state` on the button, if the page has one. */
export function showUpdateChip(state) {
  const chip = document.querySelector('#update-chip');
  if (!chip) return;
  const view = updateChipView(state);
  chip.hidden = view.hidden;
  chip.textContent = view.text;
  chip.title = view.title;
}

/**
 * The update window's Restart to Apply and Download & Install while
 * `batchRunning`. Both wait: a restart ends Studio's watch of the batch, and on
 * Windows the installer closes Studio. Neither happens by itself when the
 * batch ends, since a restart would lose the batch's unsaved results.
 */
export function updateButtonsView(batchRunning) {
  const S = STRINGS.UPDATE_MODAL;
  if (!batchRunning) return { held: false, restartText: S.RESTART_BUTTON, restartTitle: '', installTitle: '' };
  return {
    held: true,
    restartText: S.RESTART_AFTER_BATCH_BUTTON,
    restartTitle: S.RESTART_AFTER_BATCH_TITLE,
    installTitle: S.INSTALL_AFTER_BATCH_TITLE,
  };
}

/**
 * While Studio stays open, how often the background check runs again, so a
 * release that comes out after start-up still shows (#868).
 */
export const RECHECK_MS = 4 * 60 * 60 * 1000;
