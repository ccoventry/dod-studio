// batch_guard.js
// Nothing pulls the rug during a batch (#1085). A running batch marks takes on
// the project it started from: a project opened mid-batch has them written
// into its index against the old project's rows, and its next save prunes them
// away. Restarting for an update ends Studio's watch of the batch the same way.
//
// So while a batch runs, every control that replaces the project is greyed and
// its click refused with the reason, Ctrl+N and Ctrl+O included (they click
// those controls). A click is stopped here rather than by `disabled`, which the
// queue's footer and Load's dialog wait set for their own reasons and would
// clear mid-batch. An open from Explorer or a batch offer never passes through
// a control: project_session.js asks refuseDuringBatch() as well.
//
// `capture_status` is what marks a batch running, not isCaptureRunning(): that
// flag drops on Cancel, while the takes are still being checked and marked.

import { listen as tauriListen } from '@tauri-apps/api/event';
import { logFrontendEvent } from './ipc_bridge.js';
import { batchHeldLine, batchReleasedLine, batchRefusedLine } from './project_log.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

/** The controls held by id. Open Recent's rows come and go, so recent_projects.js
 *  marks those itself from heldReason(). */
const HELD_CONTROLS = ['#new-session-btn', '#load-project-btn', '#rescan-recent-btn', '#clear-all-btn', '#clear-untracked-btn'];

/** Everything a click is refused on. */
const REFUSED_CLICKS = [...HELD_CONTROLS, '.recent-project-item'].join(', ');

/** The class that greys a held control. */
const HELD_CLASS = 'batch-held';

/**
 * Starts watching for batches. Greys and ungreys the controls itself.
 * @param {object} app  isStarting() whether Start Capture's checks are running,
 *   before any capture_status: the batch's payload already holds this project.
 *   `listen`, `toast`, `log` and `now` are swappable for tests.
 * @returns {{ isHeld, heldReason, refuseDuringBatch, onChange }}
 */
export function createBatchGuard({
  isStarting = () => false,
  listen = tauriListen,
  toast = showToast,
  log = logFrontendEvent,
  now = Date.now,
} = {}) {
  const reason = STRINGS.BATCH_GUARD.HELD_TITLE;
  let running = false;
  let heldSince = 0;
  let refused = 0;
  const listeners = [];
  // Each held control's own tooltip, put back when the batch ends.
  const ownTitles = new Map();

  function isHeld() {
    return running || isStarting();
  }

  function holdControl(el) {
    ownTitles.set(el, el.title);
    el.title = reason;
    el.classList.add(HELD_CLASS);
    el.setAttribute('aria-disabled', 'true');
  }

  function releaseControl(el) {
    el.title = ownTitles.get(el) ?? '';
    el.classList.remove(HELD_CLASS);
    el.removeAttribute('aria-disabled');
  }

  function showControls() {
    HELD_CONTROLS.forEach((selector) => {
      const el = document.querySelector(selector);
      if (el) (running ? holdControl : releaseControl)(el);
    });
  }

  /** capture_status's `running`, only acted on when it changes. */
  function setRunning(next) {
    if (next === running) return;
    running = next;
    if (running) {
      heldSince = now();
      refused = 0;
      log(batchHeldLine());
    } else {
      log(batchReleasedLine(now() - heldSince, refused));
    }
    showControls();
    listeners.forEach((listener) => listener(running));
  }

  /** While a batch runs: says why with a toast, and returns true for the
   *  caller to stop. Otherwise returns false and does nothing. */
  function refuseDuringBatch() {
    if (!isHeld()) return false;
    refused += 1;
    log(batchRefusedLine());
    toast(STRINGS.BATCH_GUARD.REFUSED_TOAST, 'info');
    return true;
  }

  // In the capture phase, ahead of each control's own handler. A shortcut's
  // .click() comes through here too.
  document.addEventListener('click', (event) => {
    if (!event.target?.closest?.(REFUSED_CLICKS)) return;
    if (!refuseDuringBatch()) return;
    event.stopImmediatePropagation();
    event.preventDefault();
  }, true);

  Promise.resolve(listen('capture_status', (event) => setRunning(!!event.payload?.running)))
    .catch((err) => console.error('Failed to register capture_status listener (batch guard):', err));

  return {
    /** Whether a batch is running or about to. */
    isHeld,
    /** The tooltip for a held control, or null when nothing is held. */
    heldReason: () => (isHeld() ? reason : null),
    refuseDuringBatch,
    /** Calls `listener(running)` each time a batch starts or ends. */
    onChange: (listener) => { listeners.push(listener); },
  };
}
