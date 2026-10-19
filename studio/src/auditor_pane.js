import { open } from '@tauri-apps/plugin-dialog';
import { themedConfirm } from './themed_confirm.js';
import { listen } from '@tauri-apps/api/event';
import { runDemoAudit, deleteAuditFiles, cancelAudit, revealInExplorer } from './ipc_bridge.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import { createAuditFolders } from './audit_folders.js';
import {
  shownPath, auditProgressView, deleteProgressView, startsTicked, withoutDeleted,
  auditTotals, wastedSizeText, fileStatus, keepFile, keepPicksIn,
} from './audit_duplicates.js';

/// Demo Auditor › Duplicates (#844). `getDemos` is the project's queue:
/// copies in it are kept first and never start ticked.
export function initAuditorPane({ getDemos = () => [] } = {}) {
  let currentAuditResults = [];
  // Groups default to expanded; this tracks the exceptions. Reset on every
  // new audit run so a fresh result set always starts fully expanded.
  let collapsedGroups = new Set();
  let running = false;
  let deleting = null; // { permanent } while a delete runs

  const targetFolderInput = document.querySelector('#audit-target-folder-input');
  const browseFolderBtn = document.querySelector('#audit-browse-folder-btn');
  const runAuditBtn = document.querySelector('#run-audit-btn');
  const cancelAuditBtn = document.querySelector('#cancel-audit-btn');
  const deleteBtn = document.querySelector('#delete-selected-audits-btn');
  const deletePermanentBtn = document.querySelector('#delete-permanent-audits-btn');
  const preferFolderBtn = document.querySelector('#audit-prefer-folder-btn');
  const selectAllCb = document.querySelector('#audit-select-all');
  const progressEl = document.querySelector('#audit-progress');
  const statusTextEl = document.querySelector('#audit-status-text');
  const currentEl = document.querySelector('#audit-status-current');
  const footerEl = document.querySelector('#audit-list-footer');
  const pinnedFooterEl = document.querySelector('#footer-audit-summary');
  const tbody = document.querySelector('#audit-results-body');

  const folders = createAuditFolders({ onChange: () => refreshStartButtonState() });

  function setAuditFooterSummary(text) {
    if (footerEl) footerEl.textContent = text;
    if (pinnedFooterEl) pinnedFooterEl.textContent = text;
  }

  function refreshStartButtonState() {
    if (!runAuditBtn) return;
    runAuditBtn.disabled = running || !!deleting || !folders.canStart();
  }
  refreshStartButtonState();

  function setStatus(text, current = '') {
    if (statusTextEl) statusTextEl.textContent = text;
    if (currentEl) currentEl.textContent = current;
  }

  function showProgress(view) {
    if (!progressEl) return;
    progressEl.hidden = false;
    const fill = progressEl.querySelector('.progress-bar-fill');
    if (fill) fill.style.width = `${view.pct}%`;
    setStatus(view.text, view.current);
  }

  function hideProgress() {
    if (progressEl) progressEl.hidden = true;
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
          if (targetFolderInput) {
            targetFolderInput.value = path;
            // The other tabs follow the folder too (#624).
            targetFolderInput.dispatchEvent(new Event('input'));
          }
          refreshStartButtonState();
        }
      } catch (err) {
        console.error('Error selecting audit target folder:', err);
      }
    });
  }

  listen('audit_progress', (event) => {
    if (running) showProgress(auditProgressView(event.payload));
  });
  listen('audit_delete_progress', (event) => {
    if (deleting) showProgress(deleteProgressView(event.payload, deleting.permanent));
  });

  if (runAuditBtn) {
    runAuditBtn.addEventListener('click', async () => {
      if (!folders.canStart()) {
        showToast(STRINGS.AUDITOR.CHOOSE_FOLDER_FIRST, 'error');
        return;
      }
      const { paths, allDrives } = folders.getRequest();
      const queued = (getDemos() || []).map((d) => d?.path).filter(Boolean);

      running = true;
      collapsedGroups = new Set();
      currentAuditResults = [];
      refreshStartButtonState();
      folders.setLocked(true);
      if (cancelAuditBtn) cancelAuditBtn.disabled = false;
      setDeleteButtons(0);
      showProgress(auditProgressView(null));
      if (tbody) tbody.innerHTML = `<tr><td colspan="5" class="table-empty">${STRINGS.AUDITOR.AUDITING_IN_PROGRESS_ROW}</td></tr>`;

      try {
        const outcome = await runDemoAudit(paths, allDrives, queued);
        currentAuditResults = outcome?.groups ?? [];
        renderAuditResults({ resetTicks: true });
        setStatus(outcome?.cancelled
          ? STRINGS.AUDITOR.CANCELLED_STATUS
          : STRINGS.AUDITOR.doneStatus(outcome?.demos ?? 0, currentAuditResults.length));
        if (outcome?.cancelled && tbody) {
          tbody.innerHTML = `<tr><td colspan="5" class="table-empty">${STRINGS.AUDITOR.CANCELLED_ROW}</td></tr>`;
        }
      } catch (e) {
        setStatus(STRINGS.AUDITOR.auditFailedRow(e));
        if (tbody) {
          tbody.innerHTML = '';
          const tr = document.createElement('tr');
          const td = document.createElement('td');
          td.colSpan = 5;
          td.className = 'table-empty';
          td.textContent = STRINGS.AUDITOR.auditFailedRow(e);
          tr.appendChild(td);
          tbody.appendChild(tr);
        }
      } finally {
        running = false;
        hideProgress();
        folders.setLocked(false);
        if (cancelAuditBtn) cancelAuditBtn.disabled = true;
        refreshStartButtonState();
        updateDeleteButtonState();
      }
    });
  }

  if (cancelAuditBtn) {
    cancelAuditBtn.addEventListener('click', async () => {
      cancelAuditBtn.disabled = true;
      setStatus(STRINGS.AUDITOR.CANCELLING_STATUS);
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

  // Ticks survive collapsing a group and re-rendering.
  let ticked = new Set();

  function renderAuditResults({ resetTicks = false } = {}) {
    if (!tbody) return;

    if (currentAuditResults.length === 0) {
      tbody.innerHTML = `<tr><td colspan="5" class="table-empty">${STRINGS.AUDITOR.NO_DUPLICATES_ROW}</td></tr>`;
      setDeleteButtons(0);
      setAuditFooterSummary(STRINGS.AUDITOR.FOOTER_DEFAULT);
      return;
    }
    if (resetTicks) {
      ticked = new Set();
      currentAuditResults.forEach((g) => g.files.forEach((f, i) => { if (startsTicked(f, i)) ticked.add(f.path); }));
    }

    tbody.innerHTML = '';
    currentAuditResults.forEach((group, groupIdx) => {
      const sizeMb = (group.size / (1024 * 1024)).toFixed(2);
      const hashStr = group.header_hash.toString(16);
      const isExpanded = !collapsedGroups.has(groupIdx);

      const groupRow = document.createElement('tr');
      groupRow.className = 'audit-group-row';
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
        tr.className = fileIdx === 0 ? 'audit-file-row audit-kept' : 'audit-file-row';

        const tdCb = document.createElement('td');
        const cb = document.createElement('input');
        cb.type = 'checkbox';
        cb.className = 'audit-row-cb';
        cb.dataset.file = file.path;
        if (fileIdx === 0) {
          cb.disabled = true; // The kept copy is never deleted.
          cb.title = STRINGS.AUDITOR.ORIGINAL_FILE_TITLE;
        } else {
          cb.checked = ticked.has(file.path);
          if (file.in_use) cb.title = STRINGS.AUDITOR.IN_USE_TITLE;
        }
        cb.addEventListener('change', () => {
          if (cb.checked) ticked.add(file.path); else ticked.delete(file.path);
          updateDeleteButtonState();
        });
        tdCb.appendChild(cb);

        // Which copy to keep: the audit's pick, or any other one (#844).
        const tdStatus = document.createElement('td');
        tdStatus.className = 'audit-file-status';
        const keepLabel = document.createElement('label');
        keepLabel.className = 'audit-keep';
        keepLabel.title = fileIdx === 0 ? STRINGS.AUDITOR.ORIGINAL_FILE_TITLE : STRINGS.AUDITOR.KEEP_THIS_TITLE;
        const keepRadio = document.createElement('input');
        keepRadio.type = 'radio';
        keepRadio.name = `audit-keep-${groupIdx}`;
        keepRadio.checked = fileIdx === 0;
        keepRadio.addEventListener('change', () => keepInstead(groupIdx, file.path));
        keepLabel.append(keepRadio, document.createTextNode(` ${fileStatus(file, fileIdx).trim()}`));
        tdStatus.appendChild(keepLabel);

        const tdSize = document.createElement('td');
        tdSize.textContent = STRINGS.AUDITOR.megabytesLabel(sizeMb);

        const displayPath = shownPath(file.path);
        const tdPath = document.createElement('td');
        tdPath.className = 'audit-file-path';
        tdPath.textContent = displayPath;
        if (fileIdx > 0) tdPath.style.color = '#ff9999';
        else tdPath.style.fontWeight = 'bold';

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

    const { copies, wastedBytes } = auditTotals(currentAuditResults);
    setAuditFooterSummary(STRINGS.AUDITOR.footerSummary(copies, wastedSizeText(wastedBytes)));

    updateDeleteButtonState();
  }

  /// Keeps `path` instead of group `groupIdx`'s current pick.
  function applyKeep(groupIdx, path) {
    const { groups, untick, tick } = keepFile(currentAuditResults, groupIdx, path);
    currentAuditResults = groups;
    if (untick) ticked.delete(untick);
    if (tick) ticked.add(tick);
    return !!untick;
  }

  function keepInstead(groupIdx, path) {
    applyKeep(groupIdx, path);
    renderAuditResults();
  }

  // Keep the copies in one folder, over every group at once: a long list
  // needs no row-by-row picking (#844).
  preferFolderBtn?.addEventListener('click', async () => {
    let folder;
    try {
      folder = await open({ directory: true, multiple: false, title: STRINGS.AUDITOR.PREFER_FOLDER_DIALOG_TITLE });
    } catch (err) {
      console.error('Error picking the folder to keep:', err);
      return;
    }
    if (!folder) return;
    folder = Array.isArray(folder) ? folder[0] : folder;
    let changed = 0;
    let found = 0;
    keepPicksIn(currentAuditResults, folder).forEach(({ groupIdx, path, already }) => {
      found += 1;
      if (!already && applyKeep(groupIdx, path)) changed += 1;
    });
    renderAuditResults();
    showToast(STRINGS.AUDITOR.preferFolderToast(found, changed, folder), found ? 'success' : 'info');
  });

  /// The ticked copies still in the results (a collapsed group's count too).
  function tickedFiles() {
    const listed = new Set(currentAuditResults.flatMap((g) => g.files.slice(1).map((f) => f.path)));
    return [...ticked].filter((p) => listed.has(p));
  }

  function setDeleteButtons(n) {
    const busy = running || !!deleting;
    if (preferFolderBtn) preferFolderBtn.disabled = busy || currentAuditResults.length === 0;
    if (deleteBtn) {
      deleteBtn.disabled = busy || n === 0;
      deleteBtn.textContent = STRINGS.AUDITOR.recycleNSelectedFiles(n);
    }
    if (deletePermanentBtn) {
      deletePermanentBtn.disabled = busy || n === 0;
      deletePermanentBtn.textContent = STRINGS.AUDITOR.deleteNPermanently(n);
    }
  }

  function updateDeleteButtonState() {
    setDeleteButtons(tickedFiles().length);
  }

  if (selectAllCb) {
    selectAllCb.addEventListener('change', (e) => {
      const isChecked = e.target.checked;
      currentAuditResults.forEach((g) => g.files.slice(1).forEach((f) => {
        if (isChecked) ticked.add(f.path); else ticked.delete(f.path);
      }));
      renderAuditResults();
    });
  }

  async function deleteTicked(permanent) {
    const filesToDelete = tickedFiles();
    if (filesToDelete.length === 0) return;
    const question = permanent
      ? STRINGS.AUDITOR.deletePermanentlyConfirm(filesToDelete.length)
      : STRINGS.AUDITOR.recycleConfirm(filesToDelete.length);
    if (!(await themedConfirm(question))) return;

    deleting = { permanent };
    setDeleteButtons(filesToDelete.length);
    refreshStartButtonState();
    showProgress(deleteProgressView({ done: 0, total: filesToDelete.length }, permanent));
    try {
      const { deleted = [], failed = [] } = (await deleteAuditFiles(filesToDelete, permanent)) ?? {};
      if (deleted.length) {
        showToast(permanent
          ? STRINGS.AUDITOR.deletedFilesToast(deleted.length)
          : STRINGS.AUDITOR.recycledFilesToast(deleted.length), 'success');
      }
      if (failed.length) {
        showToast(STRINGS.AUDITOR.someNotDeletedToast(failed.length, failed[0].error), 'error');
      }
      deleted.forEach((p) => ticked.delete(p));
      currentAuditResults = withoutDeleted(currentAuditResults, deleted);
      setStatus(STRINGS.AUDITOR.deletedStatus(deleted.length, failed.length, permanent));
      if (selectAllCb) selectAllCb.checked = false;
    } catch (e) {
      // ipc_bridge toasts the error.
      setStatus(STRINGS.AUDITOR.deletionFailedToast(e));
    } finally {
      deleting = null;
      if (progressEl) progressEl.hidden = true;
      renderAuditResults();
      refreshStartButtonState();
    }
  }

  deleteBtn?.addEventListener('click', () => deleteTicked(false));
  deletePermanentBtn?.addEventListener('click', () => deleteTicked(true));
}
