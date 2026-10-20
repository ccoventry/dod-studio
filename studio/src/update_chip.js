// update_chip.js
// The menu bar's update button (#868): beside Help, it says an update is
// out, or downloaded and waiting for a restart, without opening a menu.
// Clicking it opens the update window. updater_pane.js drives it.

import { STRINGS } from './strings.js';

/**
 * What the button shows: hidden with no update, "Update 0.2.1 available"
 * once a check finds one, "Restart to update" once it has downloaded.
 * `state` is `{ version: string|null, ready: boolean }`.
 */
export function updateChipView({ version = null, ready = false } = {}) {
  const S = STRINGS.UPDATE_CHIP;
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
 * While Studio stays open, how often the background check runs again, so a
 * release that comes out after start-up still shows (#868).
 */
export const RECHECK_MS = 4 * 60 * 60 * 1000;
