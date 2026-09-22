export function showToast(message, type = 'info', duration = 3000) {
  let container = document.querySelector('#toast-container');
  if (!container) {
    container = document.createElement('div');
    container.id = 'toast-container';
    container.style.cssText = 'position: fixed; bottom: 20px; right: 20px; z-index: 9999; display: flex; flex-direction: column; gap: 8px;';
    document.body.appendChild(container);
  }

  const toast = document.createElement('div');
  toast.className = `toast toast-${type}`;
  toast.textContent = message;

  let bg = '#2196f3';
  if (type === 'success') bg = '#4caf50';
  if (type === 'error') bg = '#f44336';
  if (type === 'warning') bg = '#ff9800';

  toast.style.cssText = `background: ${bg}; color: #fff; padding: 10px 16px; border-radius: 4px; font-family: sans-serif; font-size: 14px; box-shadow: 0 4px 12px rgba(0,0,0,0.3); transition: all 0.3s ease; opacity: 1;`;

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
}
