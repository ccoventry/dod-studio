import { scanOrphanedPreviews, deleteOrphanedPreviews } from './ipc_bridge.js';
import { themedConfirm } from './themed_confirm.js';
import { showToast } from './toast.js';
import { gigabytes } from './byte_size.js';
import { STRINGS } from './strings.js';
import { formatPreviewSize } from './capture_pane_logic.js';

// ── Clear Previews audit modal ─────────────────────────────────────────────
//
// Audits `<hl>/dod` for orphaned `*_preview.dem` bookmark previews (see the
// block comment above `patch_bookmark_previews` in demo_preview.rs) left
// behind across capture sessions, and lets the user purge them.

let currentPreviewScanResults = [];

function updateClearPreviewsDeleteButtonState() {
  const deleteBtn = document.querySelector('#clear-previews-delete-btn');
  if (!deleteBtn) return;
  const checked = document.querySelectorAll('.clear-previews-row-cb:checked');
  deleteBtn.disabled = checked.length === 0;
  deleteBtn.textContent = checked.length > 0 ? STRINGS.CAPTURE.deleteNSelected(checked.length) : STRINGS.CAPTURE.DELETE_SELECTED_DEFAULT;
}

function renderClearPreviewsResults() {
  const tbody = document.querySelector('#clear-previews-body');
  const footerEl = document.querySelector('#clear-previews-footer');
  if (!tbody) return;

  if (currentPreviewScanResults.length === 0) {
    tbody.innerHTML = `<tr><td colspan="4" class="table-empty">${STRINGS.CAPTURE.NO_ORPHANED_PREVIEWS}</td></tr>`;
    if (footerEl) footerEl.textContent = STRINGS.CLEAR_PREVIEWS_MODAL.FOOTER_DEFAULT;
    updateClearPreviewsDeleteButtonState();
    return;
  }

  tbody.innerHTML = '';
  let totalBytes = 0;

  currentPreviewScanResults.forEach((entry) => {
    totalBytes += entry.size_bytes;

    const tr = document.createElement('tr');

    const tdCb = document.createElement('td');
    const cb = document.createElement('input');
    cb.type = 'checkbox';
    cb.className = 'clear-previews-row-cb';
    cb.dataset.path = entry.demo_path;
    cb.checked = true;
    cb.addEventListener('change', updateClearPreviewsDeleteButtonState);
    tdCb.appendChild(cb);

    const tdFile = document.createElement('td');
    tdFile.textContent = entry.file_name;

    const tdSize = document.createElement('td');
    tdSize.textContent = formatPreviewSize(entry.size_bytes);

    const tdModified = document.createElement('td');
    tdModified.textContent = entry.modified_unix_secs
      ? new Date(entry.modified_unix_secs * 1000).toLocaleString()
      : STRINGS.CAPTURE.EMPTY_DASH;

    tr.appendChild(tdCb);
    tr.appendChild(tdFile);
    tr.appendChild(tdSize);
    tr.appendChild(tdModified);
    tbody.appendChild(tr);
  });

  const totalGb = gigabytes(totalBytes);
  if (footerEl) footerEl.textContent = STRINGS.CLEAR_PREVIEWS_MODAL.foundReclaimable(currentPreviewScanResults.length, totalGb);
  updateClearPreviewsDeleteButtonState();
}

export function initClearPreviewsModal() {
  const openBtn = document.querySelector('#open-clear-previews-btn');
  const modal = document.querySelector('#clear-previews-modal');
  if (!openBtn || !modal) return;

  const statusEl = document.querySelector('#clear-previews-status');
  const closeBtn = document.querySelector('#clear-previews-close-btn');
  const deleteBtn = document.querySelector('#clear-previews-delete-btn');
  const selectAllBtn = document.querySelector('#clear-previews-select-all-btn');
  const tbody = document.querySelector('#clear-previews-body');

  openBtn.addEventListener('click', async () => {
    const gameDir = document.querySelector('#hl-path-input')?.value?.trim() || '';
    if (!gameDir) {
      showToast(STRINGS.CAPTURE.CONFIGURE_HL_PATH_FIRST, 'error');
      return;
    }

    modal.style.display = 'flex';
    currentPreviewScanResults = [];
    if (statusEl) statusEl.textContent = STRINGS.CAPTURE.SCANNING_ORPHANED_PREVIEWS;
    if (tbody) tbody.innerHTML = `<tr><td colspan="4" class="table-empty">${STRINGS.CLEAR_PREVIEWS_MODAL.SCANNING_ROW}</td></tr>`;
    updateClearPreviewsDeleteButtonState();

    try {
      currentPreviewScanResults = await scanOrphanedPreviews(gameDir);
      if (statusEl) statusEl.textContent = STRINGS.CAPTURE.SCAN_COMPLETE;
      renderClearPreviewsResults();
    } catch (e) {
      if (statusEl) statusEl.textContent = STRINGS.CAPTURE.SCAN_FAILED;
      if (tbody) tbody.innerHTML = `<tr><td colspan="4" class="table-empty">${STRINGS.CAPTURE.scanFailedRow(e)}</td></tr>`;
    }
  });

  if (closeBtn) {
    closeBtn.addEventListener('click', () => {
      modal.style.display = 'none';
    });
  }

  if (selectAllBtn) {
    selectAllBtn.addEventListener('click', () => {
      const boxes = document.querySelectorAll('.clear-previews-row-cb');
      const allChecked = boxes.length > 0 && Array.from(boxes).every(cb => cb.checked);
      boxes.forEach(cb => { cb.checked = !allChecked; });
      updateClearPreviewsDeleteButtonState();
    });
  }

  if (deleteBtn) {
    deleteBtn.addEventListener('click', async () => {
      const checked = document.querySelectorAll('.clear-previews-row-cb:checked');
      const pathsToDelete = Array.from(checked).map(cb => cb.dataset.path);
      if (pathsToDelete.length === 0) return;
      if (!(await themedConfirm(STRINGS.CAPTURE.deletePreviewsConfirm(pathsToDelete.length), { destructive: true }))) return;

      deleteBtn.disabled = true;
      try {
        const deletedCount = await deleteOrphanedPreviews(pathsToDelete);
        showToast(STRINGS.CAPTURE.deletedPreviews(deletedCount), 'success');
        currentPreviewScanResults = currentPreviewScanResults.filter(entry => !pathsToDelete.includes(entry.demo_path));
        renderClearPreviewsResults();
      } catch (e) {
        showToast(STRINGS.CAPTURE.deletionFailed(e), 'error');
        updateClearPreviewsDeleteButtonState();
      }
    });
  }
}
