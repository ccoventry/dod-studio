// crosshair_import_ui.js
// Players tab, below the crosshair fallback (#842): "Make your own
// crosshair…" opens Vertex's crosshair creator in the browser, and "Add a
// crosshair file…" takes the file it downloads (customxhair.spr, or a ZIP
// with it inside) and copies it into the install's addon sprites folder
// under a name the user gives, so it shows in the crosshair lists. The copy
// itself is native/src/crosshair_import.rs.

import { open } from '@tauri-apps/plugin-dialog';
import { openUrl } from '@tauri-apps/plugin-opener';
import { addCrosshairFile } from './ipc_bridge.js';
import { STRINGS } from './strings.js';
import { CREATOR_URL, crosshairFileName, defaultCrosshairName, crosshairTaken } from './crosshair_file.js';

const $ = (sel) => document.querySelector(sel);
const S = () => STRINGS.PLAYERS;

/**
 * Wires the two buttons and the name row.
 * @param {object} app  getPaths() `{ hlPath, gameFolder }`, getSprites() the
 *   sprites the crosshair lists show now, onAdded(file) re-lists them.
 */
export function createCrosshairImport({ getPaths, getSprites, onAdded }) {
  const addBtn = $('#players-add-crosshair-btn');
  const form = $('#players-add-crosshair-form');
  const nameInput = $('#players-add-crosshair-name');
  const fileLabel = $('#players-add-crosshair-file');
  const saveBtn = $('#players-add-crosshair-save');
  const cancelBtn = $('#players-add-crosshair-cancel');
  const status = $('#players-add-crosshair-status');
  // The file picked, while the name row is open.
  let source = '';
  let busy = false;

  function say(text, isError = false) {
    if (!status) return;
    status.textContent = text || '';
    status.classList.toggle('path-warning', isError);
    status.style.display = text ? '' : 'none';
  }

  function showForm(on) {
    if (form) form.style.display = on ? '' : 'none';
    if (!on) source = '';
  }

  /** The name row's file name line and whether Add can go. */
  function refreshName() {
    const file = crosshairFileName(nameInput?.value);
    const taken = crosshairTaken(file, getSprites());
    if (fileLabel) {
      fileLabel.textContent = !file ? S().CROSSHAIR_NAME_EMPTY
        : taken ? S().crosshairNameTaken(file) : S().crosshairSavedAs(file);
      fileLabel.classList.toggle('path-warning', !file || taken);
    }
    if (saveBtn) saveBtn.disabled = busy || !file || taken;
    if (addBtn) addBtn.disabled = busy;
  }

  async function pick() {
    const { hlPath } = getPaths();
    if (!hlPath) {
      say(S().CROSSHAIR_NEEDS_HL, true);
      return;
    }
    busy = true;
    refreshName();
    say('');
    const picked = await open({
      multiple: false,
      directory: false,
      title: S().CROSSHAIR_PICK_TITLE,
      filters: [{ name: S().CROSSHAIR_FILTER, extensions: ['spr', 'zip'] }],
    }).catch((err) => {
      console.error('Error opening the crosshair file picker:', err);
      return null;
    });
    busy = false;
    const path = Array.isArray(picked) ? picked[0] : picked;
    if (path) {
      source = path;
      if (nameInput) nameInput.value = defaultCrosshairName(path);
      showForm(true);
      nameInput?.focus();
      nameInput?.select();
    }
    refreshName();
  }

  async function save() {
    const file = crosshairFileName(nameInput?.value);
    if (!source || !file || busy) return;
    const { hlPath, gameFolder } = getPaths();
    busy = true;
    refreshName();
    say(S().CROSSHAIR_ADDING);
    const result = await addCrosshairFile(hlPath, gameFolder, source, nameInput?.value ?? '');
    busy = false;
    if (result?.file) {
      showForm(false);
      say(S().crosshairAdded(result.file));
      await onAdded(result.file);
    } else {
      say(S().crosshairAddFailed(result?.error || ''), true);
    }
    refreshName();
  }

  $('#players-make-crosshair-btn')?.addEventListener('click', () => {
    openUrl(CREATOR_URL).catch((err) => console.error('Error opening the crosshair creator:', err));
  });
  addBtn?.addEventListener('click', () => { pick(); });
  nameInput?.addEventListener('input', refreshName);
  nameInput?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && saveBtn && !saveBtn.disabled) save();
    if (e.key === 'Escape') { showForm(false); refreshName(); }
  });
  saveBtn?.addEventListener('click', () => { save(); });
  cancelBtn?.addEventListener('click', () => { showForm(false); say(''); refreshName(); });
  showForm(false);
  say('');

  // The crosshair lists changed (a Refresh): a name may be taken now.
  return { refresh: refreshName };
}
