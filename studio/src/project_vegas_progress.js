// project_vegas_progress.js
// What the Render tab's "Export Project to VEGAS" bar and step line show
// (#1264, CLAUDE.md "Visible progress"): finding the videos (one backend
// call that cannot count inside itself, so the bar slides), reading their
// lengths (counted, so the bar fills), then the save dialog and the write.
// The views are pure; `createVegasProgress` puts them on the page.
import { STRINGS } from './strings.js';
import { showBar, hideBar } from './progress_view.js';

/** The step that finds the videos on disk. */
export function lookupView(total) {
  return { pct: 0, indeterminate: true, text: STRINGS.RENDER.exportProjectLooking(total) };
}

/** The step that reads the lengths: `done` of `total` read so far. */
export function lengthsView(done, total) {
  const read = Math.max(0, Math.min(done, total));
  return {
    pct: total > 0 ? Math.round((read / total) * 100) : 0,
    indeterminate: false,
    text: STRINGS.RENDER.exportProjectReading(Math.min(read + 1, total), total),
  };
}

/** The last steps, which cannot be measured: the file name, then the write. */
export function pickingView() {
  return { pct: 0, indeterminate: true, text: STRINGS.RENDER.EXPORT_PROJECT_PICKING };
}
export function writingView() {
  return { pct: 0, indeterminate: true, text: STRINGS.RENDER.EXPORT_PROJECT_WRITING };
}

/**
 * Draws a view on the bar and the line beside it. The elements are looked up
 * on each call, so a page without them (a test) just shows nothing.
 * @returns {{ show: (view: object) => void, hide: () => void }}
 */
export function createVegasProgress(doc = globalThis.document) {
  const bar = () => doc?.querySelector('#export-project-progress');
  const line = () => doc?.querySelector('#export-project-status');
  return {
    show(view) {
      showBar(bar(), view);
      const text = line();
      if (text) text.textContent = view.text;
    },
    hide() {
      hideBar(bar());
      const text = line();
      if (text) text.textContent = '';
    },
  };
}
