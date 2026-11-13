// windows_drive_setting.js
// Configuration > Destinations' "Keep free on the Windows drive" (#1430): how
// many GB captures and renders leave free on the drive Windows is installed
// on. The backend does the skipping (native::sys::windows_drive) and reads the
// setting at every clip and render, so a change reaches a running batch; this
// is the field, and the wording Start Capture Batch's check shows when the
// setting is what's short. Kept out of main.js, which only wires (#683).

import { STRINGS } from './strings.js';
import { gigabytes } from './byte_size.js';

/** The largest value the field takes, as native's `MAX_KEEP_FREE_GB` clamps it. */
export const MAX_KEEP_FREE_GB = 10000;

/** The setting before settings load, as native's `DEFAULT_KEEP_FREE_GB`. */
export const DEFAULT_KEEP_FREE_GB = 50;

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
 * What Start Capture Batch counts on, from the capture folders' real free
 * space and `room` (capture_pool_room's `{ free_bytes, usable_bytes }`): the
 * free space less what the setting keeps on the Windows drive. The free space
 * itself when `room` is unknown (the call failed).
 */
export function usableCaptureBytes(freeBytes, room) {
  const usable = room?.usable_bytes;
  return Number.isFinite(usable) ? Math.min(usable, freeBytes) : freeBytes;
}

/**
 * Start Capture Batch's banner when the setting is what stops a capture that
 * needs `requiredBytes`: the folders have free space the setting keeps back.
 * `null` when the setting holds nothing back or the capture fits in what it
 * leaves. Names the setting and both ways out (#1430's live test: the banner
 * said no folder had "any free space" with 2 GB free on C:).
 */
export function windowsDriveShortWarning({ requiredBytes, freeBytes, usableBytes, keepFreeGb }) {
  if (!(usableBytes < freeBytes) || !(requiredBytes > usableBytes)) return null;
  const S = STRINGS.WINDOWS_DRIVE;
  const keep = keepFreeGb ?? DEFAULT_KEEP_FREE_GB;
  return usableBytes <= 0
    ? S.noneUsableWarning(gigabytes(requiredBytes), gigabytes(freeBytes), keep)
    : S.someUsableWarning(gigabytes(requiredBytes), gigabytes(freeBytes), gigabytes(usableBytes), keep);
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
