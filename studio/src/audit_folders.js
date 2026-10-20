// audit_folders.js — the Duplicates tab's "Folders to check" list (#844):
// the Target Folder above, folders added with the native picker (never
// typed), and the All drives choice. auditor_pane.js reads it through
// `getRequest()` when Start Audit is clicked.
import { open } from '@tauri-apps/plugin-dialog';
import { listFixedDrives } from './ipc_bridge.js';
import { STRINGS } from './strings.js';
import { addFolders, auditFolders, allDrivesLabel, canStartAudit } from './audit_duplicates.js';

export function createAuditFolders({ onChange = () => {} } = {}) {
  const $ = (sel) => document.querySelector(sel);
  const targetInput = $('#audit-target-folder-input');
  const listEl = $('#audit-folder-list');
  const addBtn = $('#audit-add-folder-btn');
  const allDrivesCb = $('#audit-all-drives');
  const allDrivesText = $('#audit-all-drives-label');
  let extra = [];
  let locked = false;

  const target = () => targetInput?.value ?? '';
  const allDrives = () => !!allDrivesCb?.checked;

  function row(path, { isTarget }) {
    const li = document.createElement('li');
    const name = document.createElement('span');
    name.className = 'audit-folder-path';
    name.textContent = path;
    li.appendChild(name);
    if (isTarget) {
      const tag = document.createElement('span');
      tag.className = 'text-muted';
      tag.textContent = STRINGS.AUDITOR.TARGET_FOLDER_TAG;
      li.appendChild(tag);
    } else {
      const remove = document.createElement('button');
      remove.type = 'button';
      remove.className = 'audit-folder-remove';
      remove.textContent = STRINGS.AUDITOR.REMOVE_FOLDER_BUTTON;
      remove.disabled = locked || allDrives();
      remove.addEventListener('click', () => {
        extra = extra.filter((f) => f !== path);
        render();
        onChange();
      });
      li.appendChild(remove);
    }
    return li;
  }

  function render() {
    if (!listEl) return;
    listEl.innerHTML = '';
    const t = target().trim();
    auditFolders(t, extra).forEach((f, i) => listEl.appendChild(row(f, { isTarget: !!t && i === 0 })));
    if (!listEl.children.length) {
      const li = document.createElement('li');
      li.className = 'text-muted';
      li.textContent = STRINGS.AUDITOR.NO_FOLDERS_YET;
      listEl.appendChild(li);
    }
    // All drives walks every fixed drive, so the list doesn't apply.
    listEl.classList.toggle('dimmed', allDrives());
    if (addBtn) addBtn.disabled = locked || allDrives();
    if (allDrivesCb) allDrivesCb.disabled = locked;
  }

  addBtn?.addEventListener('click', async () => {
    try {
      const picked = await open({ directory: true, multiple: true, title: STRINGS.AUDITOR.ADD_FOLDER_DIALOG_TITLE });
      if (!picked) return;
      extra = addFolders(extra, picked);
      render();
      onChange();
    } catch (err) {
      console.error('Error picking a folder to audit:', err);
    }
  });
  allDrivesCb?.addEventListener('change', () => { render(); onChange(); });
  targetInput?.addEventListener('input', () => { render(); onChange(); });

  listFixedDrives()
    .then((roots) => { if (allDrivesText && Array.isArray(roots)) allDrivesText.textContent = allDrivesLabel(roots); })
    .catch(() => {});

  render();

  return {
    /// What Start Audit sends: the folders, and whether to add every drive.
    getRequest: () => ({ paths: auditFolders(target(), extra), allDrives: allDrives() }),
    canStart: () => canStartAudit(target(), extra, allDrives()),
    /// Locks the list while an audit runs.
    setLocked: (value) => { locked = value; render(); },
  };
}
