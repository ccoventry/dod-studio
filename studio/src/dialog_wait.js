// dialog_wait.js — what Studio shows while a file dialog is on its way.
// The first dialog of a session can take seconds to appear: Windows sets it
// up only on first use, and it disables Studio's window first, so over that
// window the page's busy cursor gives way to the plain arrow and the wait
// looked like nothing was happening. So from the click the busy cursor shows
// (it does until the window is disabled), and when the dialog still isn't up
// after NOTICE_DELAY_MS a notice says it's coming, until it appears. A quick
// dialog never shows the notice.

import { fileDialogShown } from './ipc_bridge.js';
import { STRINGS } from './strings.js';

/** How long a dialog may take before the notice shows: most appear in well
 *  under half a second. */
export const NOTICE_DELAY_MS = 500;
/** How often, once the notice is up, Studio checks whether the dialog is. */
export const POLL_MS = 100;

/** The busy cursor over the whole window (styles.css `html.picking-path`). */
function showBusyCursor(on) {
  if (typeof document === 'undefined') return;
  document.documentElement.classList.toggle('picking-path', !!on);
}

/** The "Opening the file dialog…" notice (styles.css `#file-dialog-notice`). */
function showNotice(on) {
  if (typeof document === 'undefined') return;
  let el = document.getElementById('file-dialog-notice');
  if (!el) {
    if (!on) return;
    el = document.createElement('div');
    el.id = 'file-dialog-notice';
    el.setAttribute('role', 'status');
    el.textContent = STRINGS.FILE_DIALOG.OPENING;
    document.body.appendChild(el);
  }
  el.hidden = !on;
}

/**
 * @param {object} deps
 *   isShown() whether a dialog window is on screen; setBusy(on) the cursor;
 *   setNotice(on) the notice; setTimer / clearTimer the timers.
 * @returns {{ set(on): void, around(work): Promise }}  `set(true)` when a
 *   dialog is asked for, `set(false)` when it returns; `around(work)` both,
 *   around an async `work()`. Overlapping waits share one cursor and notice.
 */
export function createDialogWait({
  // Looked up at call time, so a test mocking ipc_bridge.js without it still
  // loads every module that imports this one.
  isShown = () => fileDialogShown(),
  setBusy = showBusyCursor,
  setNotice = showNotice,
  delayMs = NOTICE_DELAY_MS,
  pollMs = POLL_MS,
  setTimer = (fn, ms) => setTimeout(fn, ms),
  clearTimer = (id) => clearTimeout(id),
} = {}) {
  let waits = 0;
  let timer = null;
  // Bumped whenever the wait ends, so a check still in flight is dropped.
  let generation = 0;

  async function check(gen) {
    timer = null;
    let shown;
    try {
      shown = !!(await isShown());
    } catch {
      shown = true;
    }
    if (gen !== generation) return;
    setNotice(!shown);
    if (!shown) timer = setTimer(() => check(gen), pollMs);
  }

  function set(on) {
    if (on) {
      waits += 1;
      if (waits > 1) return;
      setBusy(true);
      const gen = generation;
      timer = setTimer(() => check(gen), delayMs);
      return;
    }
    if (waits === 0) return;
    waits -= 1;
    if (waits > 0) return;
    generation += 1;
    if (timer !== null) clearTimer(timer);
    timer = null;
    setBusy(false);
    setNotice(false);
  }

  async function around(work) {
    set(true);
    try {
      return await work();
    } finally {
      set(false);
    }
  }

  return { set, around };
}

// The app's one instance, shared by pick_path.js and the Save dialogs.
const shared = createDialogWait();
export const setDialogWait = (on) => shared.set(on);
export const whileDialogOpens = (work) => shared.around(work);
