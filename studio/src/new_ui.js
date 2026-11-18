// new_ui.js
// The "New UI" switch (#1085): the facelift's new pages stay hidden behind
// it until the whole rework is ready. Off by default, so the app looks and
// behaves as it did before the facelift. An app setting, per PC, not per
// project (settings.json's `new_ui`), shown as a box on Configuration's
// Pipeline tab.
//
// How a new page gates itself: `onNewUiChange(listener)` registers
// `listener(on)`, called at once and on every flip, to add or remove the
// page's way in (a menu item, a button) and to close the page when it turns
// off; and the page's open() returns early unless `isNewUiOn()`. Nothing else
// is needed: with no way in and open() refusing, the page can't be reached.
// The PR that removes the switch removes these calls.

import { logFrontendEvent } from './ipc_bridge.js';
import { STRINGS } from './strings.js';

let on = false;
const listeners = new Set();

/** Whether the new pages are shown. */
export function isNewUiOn() {
  return on;
}

/** Registers `listener(on)`: called now, and again each time the switch
 *  flips. Returns a function that unregisters it. */
export function onNewUiChange(listener) {
  listeners.add(listener);
  listener(on);
  return () => listeners.delete(listener);
}

/** Sets the switch and tells every listener when it changed. */
function setNewUi(value) {
  const next = value === true;
  if (next === on) return;
  on = next;
  listeners.forEach((listener) => listener(on));
}

/** The loaded settings file's `new_ui` (settings_sync.js). Not logged: it
 *  is no flip, only what was saved. */
export function loadNewUi(saved) {
  setNewUi(saved);
  const box = document.querySelector('#config-new-ui');
  if (box) box.checked = on;
}

/**
 * Wires Configuration's box: a flip shows or hides the new pages at once,
 * is logged and saved (`persist()`, main.js's persistAppSettings).
 */
export function createNewUi({ persist }) {
  const box = document.querySelector('#config-new-ui');
  if (!box) return;
  box.checked = on;
  box.addEventListener('change', () => {
    setNewUi(box.checked);
    logFrontendEvent(STRINGS.NEW_UI.switchedLine(on));
    persist();
  });
}
