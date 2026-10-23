// audit_folders.js — the Duplicates tab's "Folders to check" list (#844):
// folders added with the native picker (never typed), and the All drives
// choice. The page's Target Folder row is hidden on this tab, so the list is
// the one place folders are picked here: it starts from the Target Folder,
// and follows it until a folder is added or removed. auditor_pane.js reads it
// through `getRequest()` when Start Audit is clicked. "Look inside .zip
// files" (#929) is saved with the app settings (settings_sync.js), so
// `onSettingsChange` runs when it is ticked.
import { pickPath } from './pick_path.js';
import { listFixedDrives } from './ipc_bridge.js';
import { STRINGS } from './strings.js';
import { addFolders, allDrivesLabel, canStartAudit } from './audit_duplicates.js';

export function createAuditFolders({ onChange = () => {}, onSettingsChange = () => {} } = {}) {
  const $ = (sel) => document.querySelector(sel);
  const targetInput = $('#audit-target-folder-input');
  const listEl = $('#audit-folder-list');
  const addBtn = $('#audit-add-folder-btn');
  const allDrivesCb = $('#audit-all-drives');
  const allDrivesText = $('#audit-all-drives-label');
  const lookInZipsCb = $('#audit-look-in-zips');
  let folders = [];
  // Set once the list is edited here: from then on it no longer follows the
  // Target Folder the other tabs use.
  let edited = false;
  let locked = false;

  const allDrives = () => !!allDrivesCb?.checked;

  function followTarget() {
    if (edited) return;
    folders = addFolders([], targetInput?.value?.trim() ? [targetInput.value.trim()] : []);
  }

  function row(path) {
    const li = document.createElement('li');
    const name = document.createElement('span');
    name.className = 'audit-folder-path';
    name.textContent = path;
    const remove = document.createElement('button');
    remove.type = 'button';
    remove.className = 'audit-folder-remove';
    remove.textContent = STRINGS.AUDITOR.REMOVE_FOLDER_BUTTON;
    remove.disabled = locked || allDrives();
    remove.addEventListener('click', () => {
      folders = folders.filter((f) => f !== path);
      // Emptied: follow the Target Folder again, as at the start.
      edited = folders.length > 0;
      render();
      onChange();
    });
    li.append(name, remove);
    return li;
  }

  function render() {
    if (!listEl) return;
    listEl.innerHTML = '';
    folders.forEach((f) => listEl.appendChild(row(f)));
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
    if (lookInZipsCb) lookInZipsCb.disabled = locked;
  }

  addBtn?.addEventListener('click', async () => {
    try {
      const picked = await pickPath({ field: 'auditor.add_folder', directory: true, multiple: true, title: STRINGS.AUDITOR.ADD_FOLDER_DIALOG_TITLE });
      if (!picked) return;
      edited = true;
      folders = addFolders(folders, picked);
      render();
      onChange();
    } catch (err) {
      console.error('Error picking a folder to audit:', err);
    }
  });
  allDrivesCb?.addEventListener('change', () => { render(); onChange(); });
  lookInZipsCb?.addEventListener('change', () => onSettingsChange());
  targetInput?.addEventListener('input', () => { followTarget(); render(); onChange(); });

  listFixedDrives()
    .then((roots) => { if (allDrivesText && Array.isArray(roots)) allDrivesText.textContent = allDrivesLabel(roots); })
    .catch(() => {});

  followTarget();
  render();

  return {
    /// What Start Audit sends: the folders, and whether to add every drive.
    getRequest: () => ({ paths: [...folders], allDrives: allDrives(), lookInZips: !!lookInZipsCb?.checked }),
    canStart: () => canStartAudit('', folders, allDrives()),
    /// Locks the list while an audit runs.
    setLocked: (value) => { locked = value; render(); },
  };
}
