// input_refresh.js
// Keeps what hangs off a text field in step with it however its value
// changes (#535). Undo and redo (Ctrl+Z / Ctrl+Y), paste and drag-drop all
// fire `input`, but `change` only fires on blur or Enter, and only when the
// value differs from what it was when last committed -- so a refresh wired to
// `change` alone can go stale after an undo until the field loses focus.

/** How long after the last `input` the refresh runs. */
export const REFRESH_AFTER_TYPING_MS = 150;

/**
 * Runs `refresh` about `delayMs` after the last `input` on `el`, and at once
 * on `change` (blur or Enter), cancelling any that's pending. For side
 * effects too costly to run on every keystroke: a queue re-render, a warnings
 * check, a settings save. Returns a function that cancels a pending refresh.
 */
export function refreshAfterTyping(el, refresh, delayMs = REFRESH_AFTER_TYPING_MS) {
  if (!el) return () => {};
  let timer = null;
  const cancel = () => {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  };
  el.addEventListener('input', () => {
    cancel();
    timer = setTimeout(() => {
      timer = null;
      refresh();
    }, delayMs);
  });
  el.addEventListener('change', () => {
    cancel();
    refresh();
  });
  return cancel;
}
