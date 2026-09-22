import { open } from '@tauri-apps/plugin-dialog';
import { themedConfirm } from './themed_confirm.js';
import { listen } from '@tauri-apps/api/event';
import { runDemoAudit, deleteAuditFiles, cancelAudit, revealInExplorer } from './ipc_bridge.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

export function initAuditorPane() {
  let currentAuditResults = [];
  // Groups default to expanded; this tracks the exceptions. Reset on every
  // new audit run so a fresh result set always starts fully expanded.
  let collapsedGroups = new Set();

  const targetFolderInput = document.querySelector('#audit-target-folder-input');
  const browseFolderBtn = document.querySelector('#audit-browse-folder-btn');
  const runAuditBtn = document.querySelector('#run-audit-btn');
  const cancelAuditBtn = document.querySelector('#cancel-audit-btn');
  const deleteBtn = document.querySelector('#delete-selected-audits-btn');
  const selectAllCb = document.querySelector('#audit-select-all');
  const spinnerEl = document.querySelector('#audit-spinner');
  const statusTextEl = document.querySelector('#audit-status-text');
  const footerEl = document.querySelector('#audit-list-footer');
  const pinnedFooterEl = document.querySelector('#footer-audit-summary');
  const tbody = document.querySelector('#audit-results-body');

  function setAuditFooterSummary(text) {
    if (footerEl) footerEl.textContent = text;
    if (pinnedFooterEl) pinnedFooterEl.textContent = text;
  }

  function refreshStartButtonState() {
    if (!runAuditBtn) return;
    runAuditBtn.disabled = !targetFolderInput?.value?.trim();
  }
  refreshStartButtonState();

  if (targetFolderInput) {
    targetFolderInput.addEventListener('input', refreshStartButtonState);
  }

  if (browseFolderBtn) {
    browseFolderBtn.addEventListener('click', async () => {
      try {
        const selected = await open({
          directory: true,
          multiple: false,
          title: STRINGS.AUDITOR.SELECT_FOLDER_DIALOG_TITLE,
        });
        if (selected) {
          const path = Array.isArray(selected) ? selected[0] : selected;
          if (targetFolderInput) targetFolderInput.value = path;
          refreshStartButtonState();
        }
      } catch (err) {
        console.error('Error selecting audit target folder:', err);
      }
    });
  }

  let filesCheckedSoFar = 0;

  listen('audit_progress', (event) => {
    const p = event.payload;
    filesCheckedSoFar = p.files_checked || 0;
    if (statusTextEl) {
      statusTextEl.innerHTML =
        STRINGS.AUDITOR.foundSoFarHtml(filesCheckedSoFar) +
        STRINGS.AUDITOR.statusLineHtml(p.status);
    }
  });

  if (runAuditBtn) {
    runAuditBtn.addEventListener('click', async () => {
      const targetFolder = targetFolderInput?.value?.trim();
      if (!targetFolder) {
        showToast(STRINGS.AUDITOR.CHOOSE_FOLDER_FIRST, 'error');
        return;
      }

      filesCheckedSoFar = 0;
      collapsedGroups = new Set();
      runAuditBtn.disabled = true;
      if (cancelAuditBtn) cancelAuditBtn.disabled = false;
      if (deleteBtn) deleteBtn.disabled = true;
      if (spinnerEl) spinnerEl.style.display = 'inline-block';
      if (statusTextEl) statusTextEl.innerHTML = STRINGS.AUDITOR.INITIALIZING_HTML;
      if (tbody) tbody.innerHTML = `<tr><td colspan="5" class="table-empty">${STRINGS.AUDITOR.AUDITING_IN_PROGRESS_ROW}</td></tr>`;

      try {
        currentAuditResults = await runDemoAudit([targetFolder]);
        renderAuditResults();
      } catch (e) {
        if (tbody) tbody.innerHTML = `<tr><td colspan="5" class="table-empty">${STRINGS.AUDITOR.auditFailedRow(e)}</td></tr>`;
      } finally {
        if (spinnerEl) spinnerEl.style.display = 'none';
        if (cancelAuditBtn) cancelAuditBtn.disabled = true;
        refreshStartButtonState();
      }
    });
  }

  if (cancelAuditBtn) {
    cancelAuditBtn.addEventListener('click', async () => {
      cancelAuditBtn.disabled = true;
      if (statusTextEl) statusTextEl.innerHTML = STRINGS.AUDITOR.CANCELLING_HTML;
      try {
        await cancelAudit();
      } catch (e) {
        // ipc_bridge already toasts; leave the in-flight audit to resolve
        // on its own and re-enable buttons via the runAuditBtn handler's
        // finally block.
      }
    });
  }

  function toggleGroup(groupIdx) {
    if (collapsedGroups.has(groupIdx)) {
      collapsedGroups.delete(groupIdx);
    } else {
      collapsedGroups.add(groupIdx);
    }
    renderAuditResults();
  }

  function renderAuditResults() {
    if (!tbody) return;

    if (currentAuditResults.length === 0) {
      tbody.innerHTML = `<tr><td colspan="5" class="table-empty">${STRINGS.AUDITOR.NO_DUPLICATES_ROW}</td></tr>`;
      if (deleteBtn) deleteBtn.disabled = true;
      setAuditFooterSummary(STRINGS.AUDITOR.FOOTER_DEFAULT);
      return;
    }

    tbody.innerHTML = '';
    let totalWastedBytes = 0;
    let duplicateCount = 0;

    currentAuditResults.forEach((group, groupIdx) => {
      const sizeMb = (group.size / (1024 * 1024)).toFixed(2);
      const hashStr = group.header_hash.toString(16);

      const wastedBytes = group.size * (group.files.length - 1);
      totalWastedBytes += wastedBytes;
      duplicateCount += (group.files.length - 1);

      const isExpanded = !collapsedGroups.has(groupIdx);

      const groupRow = document.createElement('tr');
      groupRow.innerHTML = `
        <td></td>
        <td><button class="group-toggle-btn" type="button">${STRINGS.AUDITOR.groupToggleLabel(isExpanded, group.files.length)}</button></td>
        <td>-</td>
        <td class="text-muted">${STRINGS.AUDITOR.identicalHash(hashStr)}</td>
        <td></td>
      `;
      groupRow.querySelector('.group-toggle-btn').addEventListener('click', () => toggleGroup(groupIdx));
      tbody.appendChild(groupRow);

      if (!isExpanded) return;

      group.files.forEach((file, fileIdx) => {
        const tr = document.createElement('tr');

        const tdCb = document.createElement('td');
        const cb = document.createElement('input');
        cb.type = 'checkbox';
        cb.className = 'audit-row-cb';
        cb.dataset.file = file;

        // Auto-select duplicates (all but the first file in the group)
        if (fileIdx > 0) {
          cb.checked = true;
        } else {
          cb.disabled = true; // Prevent deleting the original
          cb.title = STRINGS.AUDITOR.ORIGINAL_FILE_TITLE;
        }

        cb.addEventListener('change', updateDeleteButtonState);
        tdCb.appendChild(cb);

        const tdStatus = document.createElement('td');
        tdStatus.textContent = STRINGS.AUDITOR.FILE_ROW_LABEL;

        const tdSize = document.createElement('td');
        tdSize.textContent = STRINGS.AUDITOR.megabytesLabel(sizeMb);

        let displayPath = file;
        if (displayPath.startsWith('\\\\?\\')) {
          displayPath = displayPath.slice(4);
        }

        const tdPath = document.createElement('td');
        tdPath.textContent = displayPath;
        tdPath.style.color = fileIdx > 0 ? '#ff9999' : undefined;
        if (fileIdx === 0) tdPath.style.fontWeight = 'bold';

        const tdAction = document.createElement('td');
        const copyBtn = document.createElement('button');
        copyBtn.type = 'button';
        copyBtn.textContent = STRINGS.AUDITOR.COPY_PATH_BUTTON;
        copyBtn.addEventListener('click', async () => {
          try {
            await navigator.clipboard.writeText(displayPath);
            showToast(STRINGS.AUDITOR.PATH_COPIED_TOAST, 'info');
          } catch (err) {
            console.error('Clipboard write failed:', err);
            showToast(STRINGS.AUDITOR.COPY_PATH_FAILED_TOAST, 'error');
          }
        });
        const openBtn = document.createElement('button');
        openBtn.type = 'button';
        openBtn.textContent = STRINGS.AUDITOR.OPEN_FOLDER_BUTTON;
        openBtn.addEventListener('click', () => {
          revealInExplorer(displayPath).catch(() => {});
        });
        tdAction.appendChild(copyBtn);
        tdAction.appendChild(openBtn);

        tr.appendChild(tdCb);
        tr.appendChild(tdStatus);
        tr.appendChild(tdSize);
        tr.appendChild(tdPath);
        tr.appendChild(tdAction);
        tbody.appendChild(tr);
      });
    });

    const wastedGb = (totalWastedBytes / (1024 * 1024 * 1024)).toFixed(2);
    setAuditFooterSummary(STRINGS.AUDITOR.footerSummary(duplicateCount, wastedGb));

    updateDeleteButtonState();
  }

  function updateDeleteButtonState() {
    if (!deleteBtn) return;
    const checked = document.querySelectorAll('.audit-row-cb:checked');
    deleteBtn.disabled = checked.length === 0;
    deleteBtn.textContent = STRINGS.AUDITOR.deleteNSelectedFiles(checked.length);
  }

  if (selectAllCb) {
    selectAllCb.addEventListener('change', (e) => {
      const isChecked = e.target.checked;
      document.querySelectorAll('.audit-row-cb:not([disabled])').forEach(cb => {
        cb.checked = isChecked;
      });
      updateDeleteButtonState();
    });
  }

  if (deleteBtn) {
    deleteBtn.addEventListener('click', async () => {
      const checked = document.querySelectorAll('.audit-row-cb:checked');
      const filesToDelete = Array.from(checked).map(cb => cb.dataset.file);

      if (filesToDelete.length === 0) return;
      if (!(await themedConfirm(STRINGS.AUDITOR.deleteConfirm(filesToDelete.length)))) return;

      deleteBtn.disabled = true;
      try {
        await deleteAuditFiles(filesToDelete);
        showToast(STRINGS.AUDITOR.deletedFilesToast(filesToDelete.length), 'success');

        // Remove deleted files from current state to avoid re-auditing immediately
        currentAuditResults = currentAuditResults.map(g => {
          return {
            ...g,
            files: g.files.filter(f => !filesToDelete.includes(f))
          };
        }).filter(g => g.files.length > 1); // Remove group if no duplicates remain

        renderAuditResults();
        if (selectAllCb) selectAllCb.checked = false;
      } catch (e) {
        showToast(STRINGS.AUDITOR.deletionFailedToast(e), 'error');
        updateDeleteButtonState();
      }
    });
  }
}
