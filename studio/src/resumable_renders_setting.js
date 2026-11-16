// resumable_renders_setting.js
// Configuration > Render's "Resumable renders" (#1451): long renders split
// into pieces, so a stopped render resumes instead of starting from 0. The
// backend reads the saved setting as each render batch starts
// (render_manager.rs) and native's renderer does the splitting; this is the
// checkbox, saved with the rest of the settings. Kept out of main.js, which
// only wires (#683).

/** The setting before settings load, as the backend's `default_true`. */
export const DEFAULT_RESUMABLE_RENDERS = true;

const BOX = '#render-resumable-checkbox';

/** The last value loaded or saved, for when the box isn't on the page. */
let saved = DEFAULT_RESUMABLE_RENDERS;

/** The saved value for `value` from the settings file: on unless it says off. */
export function resumableFrom(value) {
  return value !== false;
}

/** What settings_sync.js saves: the box, else the last value loaded. */
export function readResumableRenders() {
  const box = document.querySelector(BOX);
  return box ? box.checked : saved;
}

/** What settings_sync.js loads into the box (`render_resumable`). */
export function applyResumableRenders(value) {
  saved = resumableFrom(value);
  const box = document.querySelector(BOX);
  if (box) box.checked = saved;
}

/** Saves a change of the box: `persist()` saves the settings file. */
export function startResumableRenders({ persist }) {
  const box = document.querySelector(BOX);
  box?.addEventListener('change', () => {
    saved = box.checked;
    persist();
  });
}
