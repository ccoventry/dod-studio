// Themed replacement for @tauri-apps/plugin-dialog's confirm() at plain
// delete-confirmation call sites. The native dialog is WebView2's own
// unstyleable box (plain white, default OK/Cancel) and clashes with the
// app's dark theme; main.js's #clear-all-modal already proved the pattern
// for the richer tracked-delete case (with a Save-First escalation), this
// generalizes the same "ask, await the answer" Promise shape for the plain
// case that just needs a themed Confirm/Cancel. See issue #43.
import { STRINGS } from './strings.js';

let pendingResolve = null;
let modal, titleEl, messageEl, okBtn, cancelBtn, detailsEl, footerEl;

export function initThemedConfirm() {
  modal = document.querySelector('#themed-confirm-modal');
  if (!modal) return;
  titleEl = document.querySelector('#themed-confirm-title');
  messageEl = document.querySelector('#themed-confirm-message');
  okBtn = document.querySelector('#themed-confirm-ok-btn');
  cancelBtn = document.querySelector('#themed-confirm-cancel-btn');
  // A list under the message, and a line after it, for dialogs that need
  // more than a sentence (#477's moved demos). Made here so every page that
  // hosts the modal gets them.
  if (messageEl && !detailsEl) {
    detailsEl = document.createElement('div');
    detailsEl.id = 'themed-confirm-details';
    detailsEl.style.cssText = 'display: none; flex-direction: column; gap: 10px; margin: 10px 0; max-height: 50vh; overflow-y: auto;';
    footerEl = document.createElement('p');
    footerEl.id = 'themed-confirm-footer';
    footerEl.style.display = 'none';
    messageEl.after(detailsEl, footerEl);
  }

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
 *
 * `details`: an optional list shown under the message, one entry per item,
 * each `{ primary, secondary, title }` (a main line, a smaller second line,
 * and a hover text). `footer`: an optional line after the list.
 * `focusConfirm`: the confirm button takes focus, so Enter picks it; only for
 * a question whose yes loses nothing.
 * `destructive`: the confirm button is red, for a yes that deletes, recycles,
 * overwrites or throws work away; every other confirm uses the accent colour
 * (#880).
 */
export function themedConfirm(message, { title, confirmLabel, cancelLabel, details, footer, hideCancel = false, focusConfirm = false, destructive = false } = {}) {
  if (titleEl) titleEl.textContent = title || STRINGS.THEMED_CONFIRM_MODAL.TITLE_DEFAULT;
  if (messageEl) messageEl.textContent = message;
  if (detailsEl) {
    detailsEl.replaceChildren(...(details || []).map((item) => {
      const entry = document.createElement('div');
      entry.className = 'themed-confirm-detail';
      if (item.title) entry.title = item.title;
      const primary = document.createElement('div');
      primary.textContent = item.primary;
      primary.style.cssText = 'font-weight: 600; word-break: break-word;';
      entry.append(primary);
      if (item.secondary) {
        const secondary = document.createElement('div');
        secondary.textContent = item.secondary;
        secondary.style.cssText = 'font-size: 0.85em; color: var(--text-muted, #999); word-break: break-word;';
        entry.append(secondary);
      }
      return entry;
    }));
    detailsEl.style.display = details && details.length ? 'flex' : 'none';
  }
  if (footerEl) {
    footerEl.textContent = footer || '';
    footerEl.style.display = footer ? '' : 'none';
  }
  if (okBtn) {
    okBtn.textContent = confirmLabel || STRINGS.THEMED_CONFIRM_MODAL.CONFIRM_BUTTON;
    okBtn.classList.toggle('danger-btn', !!destructive);
    okBtn.classList.toggle('primary-btn', !destructive);
  }
  if (cancelBtn) {
    cancelBtn.textContent = cancelLabel || STRINGS.THEMED_CONFIRM_MODAL.CANCEL_BUTTON;
    // `hideCancel`: a notice with one button, not a question.
    cancelBtn.style.display = hideCancel ? 'none' : '';
  }
  if (modal) modal.style.display = 'flex';
  if (focusConfirm) okBtn?.focus();
  return new Promise((resolve) => { pendingResolve = resolve; });
}

/** Rewrites the open dialog's message, for a line worked out after it
 *  opened (#937's rescan time). Does nothing once it has closed. */
export function setThemedConfirmMessage(message) {
  if (pendingResolve && messageEl) messageEl.textContent = message;
}
