// folder_pools.js
// Configuration's two folder pools: Capture Locations (doubles as Render
// Studio's scan input) and Render Studio's Export Drives, with their editable
// lists (list_editor.js), Add/Browse buttons and the capture pool's free-space
// line. Kept out of main.js, which only wires (#683).

import { pickPath } from './pick_path.js';
import { calculateExportPoolSpace } from './ipc_bridge.js';
import { createListEditor } from './list_editor.js';
import { refreshLaunchGuard } from './capture_pane.js';
import { refreshRenderButtons } from './render_pane.js';
import { refreshSameDriveWarning } from './config_wiring.js';
import { STRINGS } from './strings.js';

/**
 * @param {object} app  What this needs from main.js: getDemos() the queue,
 *   persist() saves the settings file, refreshSetup() redraws the Setup
 *   checklist (setup_wiring.js), which exists only late in startup.
 */
export function createFolderPools({ getDemos, persist, refreshSetup }) {
  let targetDrives = [];
  let renderExportDirs = [];

  async function pickTargetDrive() {
    try {
      return await pickPath({ field: 'capture.output_folder', directory: true, multiple: false, title: STRINGS.MAIN.SELECT_CAPTURE_OUTPUT_DIR_TITLE });
    } catch (err) {
      console.error("Error opening capture output directory dialog:", err);
      return null;
    }
  }

  async function pickRenderExportDir() {
    try {
      return await pickPath({ field: 'render.export_folder', directory: true, multiple: false, title: STRINGS.MAIN.SELECT_RENDER_EXPORT_DIR_TITLE });
    } catch (err) {
      console.error("Error opening render export directory dialog:", err);
      return null;
    }
  }

  async function updateExportPoolIndicator() {
    const indicator = document.querySelector('#export-pool-free-indicator');
    refreshSameDriveWarning(targetDrives);
    if (!indicator) return;
    if (targetDrives.length === 0) {
      indicator.textContent = STRINGS.MAIN.EXPORT_POOL_FREE_DEFAULT;
      return;
    }
    try {
      const bytes = await calculateExportPoolSpace(targetDrives);
      const gb = bytes / (1024 * 1024 * 1024);
      indicator.textContent = STRINGS.MAIN.exportPoolFree(gb.toFixed(1));
    } catch (err) {
      console.error("Error calculating export pool space:", err);
      indicator.textContent = STRINGS.MAIN.EXPORT_POOL_ERROR;
    }
  }

  const captureEditor = createListEditor({
    container: document.querySelector('#target-drive-list'),
    getItems: () => targetDrives,
    fields: [{ key: 'value', type: 'text', primitive: true, placeholder: STRINGS.CAPTURE_CONFIG.OUTPUT_DIR_PLACEHOLDER }],
    unique: true,
    browse: pickTargetDrive,
    onChange: () => {
      updateExportPoolIndicator();
      persist();
      refreshLaunchGuard({ targetDrives, currentScannedDemos: getDemos() });
      refreshSetup();
    },
  });

  const renderEditor = createListEditor({
    container: document.querySelector('#render-export-dir-list'),
    getItems: () => renderExportDirs,
    fields: [{ key: 'value', type: 'text', primitive: true, placeholder: STRINGS.RENDER.EXPORT_DIR_ROW_PLACEHOLDER }],
    unique: true,
    browse: pickRenderExportDir,
    onChange: () => { persist(); refreshSetup(); refreshSameDriveWarning(renderExportDirs, 'clips'); refreshRenderButtons(); },
  });

  // A typed path is added on Add; Browse adds the picked folder.
  function wireAddAndBrowse(addSel, inputSel, browseSel, editor, pick) {
    document.querySelector(addSel)?.addEventListener('click', () => {
      const inputEl = document.querySelector(inputSel);
      const path = inputEl?.value?.trim();
      if (path && editor.addItem(path) && inputEl) inputEl.value = '';
    });
    document.querySelector(browseSel)?.addEventListener('click', async () => {
      const selected = await pick();
      if (selected) editor.addItem(selected);
    });
  }
  wireAddAndBrowse('#add-drive-btn', '#drive-path-input', '#browse-drive-btn', captureEditor, pickTargetDrive);
  wireAddAndBrowse('#add-render-export-dir-btn', '#render-export-dir-input', '#browse-render-export-btn', renderEditor, pickRenderExportDir);

  /** Fills both pools from a loaded settings file. */
  function load(settings) {
    if (Array.isArray(settings.target_drives) && settings.target_drives.length > 0) {
      targetDrives = [...settings.target_drives];
      captureEditor.render();
      updateExportPoolIndicator();
    }
    if (Array.isArray(settings.render_export_dirs) && settings.render_export_dirs.length > 0) {
      renderExportDirs = [...settings.render_export_dirs];
      renderEditor.render();
      refreshSameDriveWarning(renderExportDirs, 'clips');
      refreshRenderButtons();
    }
  }

  return {
    targetDrives: () => targetDrives,
    renderExportDirs: () => renderExportDirs,
    updateExportPoolIndicator,
    load,
    captureFolders: { add: (p) => captureEditor.addItem(p), remove: (p) => captureEditor.removeItem(p) },
    renderFolders: { add: (p) => renderEditor.addItem(p), remove: (p) => renderEditor.removeItem(p) },
  };
}
