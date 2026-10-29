import { logFrontendEvent } from './ipc_bridge.js';

/** The activity-log line for a toast (#873): its kind and its text on one
 *  line, so a log shows what the user was told. */
export function toastLogLine(message, type = 'info') {
  return `[toast ${type}] ${String(message).replace(/\s*\n\s*/g, ' ').trim()}`;
}

/**
 * `action` ({ label, onClick }) adds a button to the toast, e.g. Undo.
 * Clicking it runs onClick and dismisses the toast at once.
 * Every toast is also written to the activity log (#873).
 * The same text and kind already on screen isn't shown a second time (#1097).
 */
export function showToast(message, type = 'info', duration = 3000, { action } = {}) {
  let container = document.querySelector('#toast-container');
  const className = `toast toast-${type}`;
  if (!action && container && [...container.children].some(
    (t) => t.className === className && t.textContent === String(message),
  )) return;

  logFrontendEvent(toastLogLine(message, type));

  if (!container) {
    container = document.createElement('div');
    container.id = 'toast-container';
    container.style.cssText = 'position: fixed; bottom: 20px; right: 20px; z-index: 9999; display: flex; flex-direction: column; gap: 8px;';
    document.body.appendChild(container);
  }

  const toast = document.createElement('div');
  toast.className = className;
  toast.textContent = message;

  let bg = '#2196f3';
  if (type === 'success') bg = '#4caf50';
  if (type === 'error') bg = '#f44336';
  if (type === 'warning') bg = '#ff9800';

  toast.style.cssText = `background: ${bg}; color: #fff; padding: 10px 16px; border-radius: 4px; font-family: sans-serif; font-size: 14px; box-shadow: 0 4px 12px rgba(0,0,0,0.3); transition: all 0.3s ease; opacity: 1;`;

  if (action) {
    const button = document.createElement('button');
    button.type = 'button';
    button.textContent = action.label;
    button.style.cssText = 'margin-left: 12px; background: transparent; color: #fff; border: 1px solid rgba(255,255,255,0.7); border-radius: 3px; padding: 2px 8px; cursor: pointer; font-size: 13px;';
    button.addEventListener('click', () => {
      action.onClick();
      if (toast.parentNode === container) container.removeChild(toast);
    });
    toast.appendChild(button);
  }

  container.appendChild(toast);

  const fadeOutDuration = 500;
  const displayDuration = Math.max(0, duration - fadeOutDuration);

  setTimeout(() => {
    toast.classList.add('toast-fade-out');
    setTimeout(() => {
      if (toast.parentNode === container) {
        container.removeChild(toast);
      }
    }, fadeOutDuration);
  }, displayDuration);
  return toast;
}
