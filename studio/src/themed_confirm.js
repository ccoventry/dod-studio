// Themed replacement for @tauri-apps/plugin-dialog's confirm() at plain
// delete-confirmation call sites. The native dialog is WebView2's own
// unstyleable box (plain white, default OK/Cancel) and clashes with the
// app's dark theme; main.js's #clear-all-modal already proved the pattern
// for the richer tracked-delete case (with a Save-First escalation), this
// generalizes the same "ask, await the answer" Promise shape for the plain
// case that just needs a themed Confirm/Cancel. See issue #43.
import { STRINGS } from './strings.js';

let pendingResolve = null;
let modal, titleEl, messageEl, okBtn, cancelBtn;

export function initThemedConfirm() {
  modal = document.querySelector('#themed-confirm-modal');
  if (!modal) return;
  titleEl = document.querySelector('#themed-confirm-title');
  messageEl = document.querySelector('#themed-confirm-message');
  okBtn = document.querySelector('#themed-confirm-ok-btn');
  cancelBtn = document.querySelector('#themed-confirm-cancel-btn');

  okBtn?.addEventListener('click', () => resolveAndClose(true));
  cancelBtn?.addEventListener('click', () => resolveAndClose(false));
}

function resolveAndClose(result) {
  if (modal) modal.style.display = 'none';
  pendingResolve?.(result);
  pendingResolve = null;
}

/**
 * Same Promise<boolean> shape as plugin-dialog's confirm(message, options),
 * so it drops into existing `await confirm(...)` call sites unchanged.
 */
export function themedConfirm(message, { title, confirmLabel, cancelLabel } = {}) {
  if (titleEl) titleEl.textContent = title || STRINGS.THEMED_CONFIRM_MODAL.TITLE_DEFAULT;
  if (messageEl) messageEl.textContent = message;
  if (okBtn) okBtn.textContent = confirmLabel || STRINGS.THEMED_CONFIRM_MODAL.CONFIRM_BUTTON;
  if (cancelBtn) cancelBtn.textContent = cancelLabel || STRINGS.THEMED_CONFIRM_MODAL.CANCEL_BUTTON;
  if (modal) modal.style.display = 'flex';
  return new Promise((resolve) => { pendingResolve = resolve; });
}
