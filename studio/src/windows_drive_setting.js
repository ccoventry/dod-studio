// windows_drive_setting.js
// Configuration > Destinations' "Keep free on the Windows drive" (#1430): how
// many GB captures and renders leave free on the drive Windows is installed
// on. The backend does the skipping (native::sys::windows_drive) and reads the
// setting at every clip and render, so a change reaches a running batch; this
// is the field. Kept out of main.js, which only wires (#683).

/** The largest value the field takes, as native's `MAX_KEEP_FREE_GB` clamps it. */
export const MAX_KEEP_FREE_GB = 10000;

const FIELD = '#config-windows-drive-keep-free';

/** The last value loaded or saved, for a field left blank or garbled. */
let savedGb = null;

/**
 * A whole number of GB from 0 to MAX_KEEP_FREE_GB for `raw` (the field's
 * text), or `fallback` when it isn't a number. 0 turns the setting off.
 */
export function keepFreeGbFrom(raw, fallback) {
  const text = String(raw ?? '').trim();
  const n = text === '' ? NaN : Number(text);
  if (!Number.isFinite(n)) return fallback;
  return Math.min(MAX_KEEP_FREE_GB, Math.max(0, Math.round(n)));
}

/**
 * What settings_sync.js saves: the field's value, else the last saved one.
 * `undefined` (left out of the saved file, so the backend's default applies)
 * only before anything has loaded.
 */
export function readWindowsDriveKeepFree() {
  return keepFreeGbFrom(document.querySelector(FIELD)?.value, savedGb) ?? undefined;
}

/** What settings_sync.js loads into the field. */
export function applyWindowsDriveKeepFree(value) {
  savedGb = keepFreeGbFrom(value, savedGb);
  const input = document.querySelector(FIELD);
  if (input && savedGb !== null) input.value = String(savedGb);
}

/**
 * Saves a settled change, with the field shown as the value saved.
 * `persist()` saves the settings file (a promise); `onSaved()` runs once it
 * has, when the backend counts the Windows drive by the new value.
 */
export function startWindowsDriveKeepFree({ persist, onSaved }) {
  const input = document.querySelector(FIELD);
  input?.addEventListener('change', async () => {
    savedGb = keepFreeGbFrom(input.value, savedGb);
    if (savedGb !== null) input.value = String(savedGb);
    await persist();
    onSaved?.();
  });
}
