// take_cleanup.js
//
// "After a clip is finished" (#644, the automatic half of Delete Take): keep a
// take, move it to the Recycle Bin, or delete it for good once its clip has
// rendered, and "Only when space is needed" to hold the permanent delete back
// until a capture or render batch is short of room.
//
// The backend does the work (take_cleanup_manager.rs) and decides which takes
// may go: only ones whose every render finished with the clip file still
// there. This module is the setting (its hint, the option that needs the
// permanent choice, and the confirm when that choice is turned on) and what
// the backend reports: a progress bar in the Render tab, then one toast.
//
// Everything above the page wiring is DOM-free so it can be unit tested
// (take_cleanup.test.js).
import { listen } from '@tauri-apps/api/event';
import { STRINGS } from './strings.js';
import { themedConfirm } from './themed_confirm.js';
import { showToast } from './toast.js';

export const AFTER_FINISH = { KEEP: 'keep', RECYCLE: 'recycle', DELETE: 'delete' };

/** The setting's value, with anything unknown read as Keep, as the backend does. */
export function normalizeChoice(value) {
  return Object.values(AFTER_FINISH).includes(value) ? value : AFTER_FINISH.KEEP;
}

/** "Only when space is needed" only means something with the permanent choice:
 *  the Recycle Bin frees no space until it is emptied. */
export function whenNeededAvailable(choice) {
  return choice === AFTER_FINISH.DELETE;
}

/** With "Only when space is needed" on, finished takes may cover a capture's
 *  shortfall, so the capture tab doesn't stop the batch up front: the backend
 *  makes room first and stops the batch itself when that can't help. */
export function makesRoomBeforeCapture(choice, whenNeeded) {
  return choice === AFTER_FINISH.DELETE && !!whenNeeded;
}

/** The line under the setting, saying what the current choice does. */
export function settingHint(choice, whenNeeded) {
  if (choice === AFTER_FINISH.RECYCLE) return STRINGS.TAKE_CLEANUP.HINT_RECYCLE;
  if (choice === AFTER_FINISH.DELETE) {
    return whenNeeded ? STRINGS.TAKE_CLEANUP.HINT_DELETE_WHEN_NEEDED : STRINGS.TAKE_CLEANUP.HINT_DELETE;
  }
  return STRINGS.TAKE_CLEANUP.HINT_KEEP;
}

/** Turning permanent deletion on asks first; leaving it on, or loading it, doesn't. */
export function needsConfirm(previous, next) {
  return next === AFTER_FINISH.DELETE && previous !== AFTER_FINISH.DELETE;
}

/** The progress line for a `take_cleanup_progress` payload. */
export function progressText({ step, done, total } = {}) {
  return STRINGS.TAKE_CLEANUP.progressLine(step, done, total);
}

/**
 * The toast for a `take_cleanup_done` payload: `{ message, level }`, or `null`
 * when nothing went and nothing needs saying.
 */
export function doneToast(payload, formatSize) {
  const deleted = payload?.deleted || [];
  const failed = payload?.failed || [];
  if (failed.length > 0) {
    return { message: STRINGS.TAKE_CLEANUP.failedToast(failed.length, failed[0].error), level: 'error' };
  }
  if (payload?.short) {
    return {
      message: STRINGS.TAKE_CLEANUP.shortToast(
        payload.reason,
        formatSize(payload.short.needed_bytes),
        formatSize(payload.short.available_bytes),
      ),
      level: 'warning',
    };
  }
  if (deleted.length === 0) return null;
  const total = deleted.reduce((sum, d) => sum + (Number(d.size_bytes) || 0), 0);
  return {
    message: STRINGS.TAKE_CLEANUP.doneToast(deleted.length, formatSize(total), payload.reason, payload.method),
    level: 'success',
  };
}

/**
 * The setting's controls, with every side effect injected.
 *
 * deps:
 *   confirm() -> Promise<boolean>   the permanent-deletion confirm
 *   render({ choice, whenNeeded, whenNeededEnabled, hint })  paints the controls
 *   onChange()                      persist the settings
 */
export function createTakeCleanupSetting({ confirm, render, onChange }) {
  let choice = AFTER_FINISH.KEEP;
  let whenNeeded = false;

  function paint() {
    render({
      choice,
      whenNeeded,
      whenNeededEnabled: whenNeededAvailable(choice),
      hint: settingHint(choice, whenNeeded),
    });
  }

  return {
    /** Loads saved values without asking: they were confirmed when set. */
    set(nextChoice, nextWhenNeeded) {
      choice = normalizeChoice(nextChoice);
      whenNeeded = !!nextWhenNeeded;
      paint();
    },
    get() {
      return { choice, whenNeeded };
    },
    /** The select changed. A cancelled confirm puts the old choice back. */
    async choose(next) {
      const value = normalizeChoice(next);
      if (needsConfirm(choice, value) && !(await confirm())) {
        paint();
        return;
      }
      choice = value;
      paint();
      onChange?.();
    },
    setWhenNeeded(value) {
      whenNeeded = !!value;
      paint();
      onChange?.();
    },
  };
}

/**
 * Subscribes to the backend's reports. `showProgress(progress | null)` paints
 * the bar; `toast(message, level)` shows the end.
 */
export function createTakeCleanupReports({ listen: subscribe, showProgress, toast, formatSize }) {
  subscribe('take_cleanup_progress', (event) => {
    const progress = event?.payload;
    if (progress?.step) showProgress(progress);
  });
  subscribe('take_cleanup_done', (event) => {
    showProgress(null);
    const result = doneToast(event?.payload, formatSize);
    if (result) toast(result.message, result.level);
  });
}

// ── Page wiring ──────────────────────────────────────────────────────────────

let setting = null;
// Settings load before the page wiring runs (main.js's boot order), so a
// value loaded early waits here for initTakeCleanup.
let loaded = { choice: AFTER_FINISH.KEEP, whenNeeded: false };

function sizeText(bytes) {
  const gb = (Number(bytes) || 0) / (1024 * 1024 * 1024);
  return gb >= 1
    ? STRINGS.RENDER.fileSizeGb(gb.toFixed(2))
    : STRINGS.RENDER.fileSizeMb(((Number(bytes) || 0) / (1024 * 1024)).toFixed(0));
}

function renderSetting({ choice, whenNeeded, whenNeededEnabled, hint }) {
  const select = document.querySelector('#config-take-after-finish');
  const box = document.querySelector('#config-take-delete-when-needed');
  const hintEl = document.querySelector('#config-take-after-finish-hint');
  if (select) select.value = choice;
  if (box) {
    box.checked = whenNeeded;
    box.disabled = !whenNeededEnabled;
    const title = whenNeededEnabled ? STRINGS.TAKE_CLEANUP.WHEN_NEEDED_TITLE : STRINGS.TAKE_CLEANUP.WHEN_NEEDED_OFF_TITLE;
    box.title = title;
    const label = document.querySelector('label[for="config-take-delete-when-needed"]');
    if (label) label.title = title;
  }
  if (hintEl) hintEl.textContent = hint;
}

/** The Render tab's take-delete bar, shared with Delete Take: one delete at a
 *  time shows there, whoever started it. */
function showProgress(progress) {
  const wrap = document.querySelector('#render-take-delete-progress');
  if (!wrap) return;
  wrap.style.display = progress ? 'flex' : 'none';
  if (!progress) return;
  const pct = progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0;
  const fill = wrap.querySelector('.progress-bar-fill');
  if (fill) fill.style.width = `${pct}%`;
  const label = wrap.querySelector('#render-take-delete-progress-label');
  if (label) label.textContent = progressText(progress);
}

/** What settings_sync.js saves. */
export function getTakeCleanupSetting() {
  const { choice, whenNeeded } = setting ? setting.get() : loaded;
  return { take_after_finish: choice, take_delete_only_when_space_needed: whenNeeded };
}

/** Whether the current setting makes room before a capture (capture_pane.js's launch guard). */
export function takeCleanupMakesRoom() {
  const { choice, whenNeeded } = setting ? setting.get() : loaded;
  return makesRoomBeforeCapture(choice, whenNeeded);
}

/** What settings_sync.js loads. */
export function setTakeCleanupSetting(choice, whenNeeded) {
  loaded = { choice: normalizeChoice(choice), whenNeeded: !!whenNeeded };
  setting?.set(loaded.choice, loaded.whenNeeded);
}

/** `onSettingsChange` persists the settings when the user changes one. */
export function initTakeCleanup({ onSettingsChange } = {}) {
  setting = createTakeCleanupSetting({
    confirm: () => themedConfirm(STRINGS.TAKE_CLEANUP.CONFIRM_MESSAGE, {
      title: STRINGS.TAKE_CLEANUP.CONFIRM_TITLE,
      confirmLabel: STRINGS.TAKE_CLEANUP.CONFIRM_BUTTON,
    }),
    render: renderSetting,
    onChange: onSettingsChange,
  });
  setting.set(loaded.choice, loaded.whenNeeded);

  document.querySelector('#config-take-after-finish')
    ?.addEventListener('change', (e) => { setting.choose(e.target.value); });
  document.querySelector('#config-take-delete-when-needed')
    ?.addEventListener('change', (e) => setting.setWhenNeeded(e.target.checked));

  createTakeCleanupReports({
    listen: (name, handler) => listen(name, handler)
      .catch((err) => console.error(`Failed to register ${name} listener (take cleanup):`, err)),
    showProgress,
    toast: (message, level) => showToast(message, level, level === 'success' ? 6000 : 10000),
    formatSize: sizeText,
  });
}
