// preview_trim_status.js
// Launch Preview of a demo over the engine's entity limit plays a trimmed
// copy (#207, studio/src-tauri/src/trimmed_demos.rs). While the backend makes
// one (a few seconds for a WSOD25 HLTV half), the Launch Preview button says
// which step it is on, from its `preview_trim` events; when the preview plays
// a copy, a toast says so.

import { listen } from '@tauri-apps/api/event';
import { showToast } from './toast.js';

/** The backend's event: `{ status, done }`. */
export const PREVIEW_TRIM_EVENT = 'preview_trim';

/** How long the "plays a trimmed copy" toast stays. */
const DONE_TOAST_MS = 8000;

/** What one event does: `{ label }` for the button while the copy is made,
 *  `{ toast }` once the preview plays it, or null for an empty event. */
export function previewTrimUpdate(payload) {
  const status = payload?.status;
  if (typeof status !== 'string' || !status) return null;
  return payload.done ? { toast: status } : { label: status };
}

/** Shows `preview_trim` events on `button` (its label) and as a toast.
 *  Resolves to the function that stops listening; never rejects. */
export async function watchPreviewTrim(button) {
  return listen(PREVIEW_TRIM_EVENT, (event) => {
    const update = previewTrimUpdate(event.payload);
    if (update?.label && button) button.textContent = update.label;
    if (update?.toast) showToast(update.toast, 'info', DONE_TOAST_MS);
  }).catch((err) => {
    // The preview still runs; only its trim's progress goes unshown.
    console.error('Could not listen for preview_trim:', err);
    return () => {};
  });
}
