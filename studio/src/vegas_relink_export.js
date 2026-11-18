// vegas_relink_export.js
// The Render tab's "Export VEGAS Relink List…" and "From a Project Backup…"
// buttons (#1514): saves where each of the loaded project's rendered videos
// used to be and where it is now (vegas_relink_map.js), for the VEGAS script
// `DoD Studio Relink Clips.cs`, which swaps them on an existing VEGAS project
// and keeps the edit. The first goes by the paths Find Moved Videos kept on
// each render; the second also compares with an older copy of the project
// file, which is only ever read. The bar and line under the buttons show each
// step from the click to the end.

import { save } from '@tauri-apps/plugin-dialog';
import { writeTextFile, readProjectFile, filesExist, logFrontendEvent } from './ipc_bridge.js';
import { whileDialogOpens } from './dialog_wait.js';
import { pickPath } from './pick_path.js';
import { showToast } from './toast.js';
import { showBar, hideBar } from './progress_view.js';
import { STRINGS } from './strings.js';
import { PROJECT_EXTENSION } from './project_file_type.js';
import {
  movesFromRecord, movesFromBackup, mergeMoves,
  relinkMap, moveCounts, relinkDefaultPath, RELINK_SAVE_FILTER,
} from './vegas_relink_map.js';
import { fileNameOf } from './path_display.js';

/** The bar's view for a step that can't be counted. */
const stepView = (text) => ({ pct: 0, indeterminate: true, text });

/** Draws on the bar and line under the buttons; nothing on a page without them. */
function createRelinkProgress(doc = globalThis.document) {
  const bar = () => doc?.querySelector('#vegas-relink-progress');
  const line = () => doc?.querySelector('#vegas-relink-status');
  return {
    show(view) {
      showBar(bar(), view);
      const text = line();
      if (text) text.textContent = view.text;
    },
    hide() {
      hideBar(bar());
      const text = line();
      if (text) text.textContent = '';
    },
  };
}

const defaultIo = {
  pickBackup: () => pickPath({
    field: 'render.relink_backup',
    multiple: false,
    directory: false,
    title: STRINGS.RENDER.RELINK_BACKUP_PICK_TITLE,
    // A backup's name rarely ends in .dodstudio (it is usually the project's
    // name with something added), so every file shows first.
    filters: [{ name: 'All files', extensions: ['*'] }, { name: 'DoD Studio project', extensions: [PROJECT_EXTENSION] }],
  }).catch((err) => {
    console.error('Open dialog failed:', err);
    return null;
  }),
  read: readProjectFile,
  exists: filesExist,
  pickSave: (defaultPath) => whileDialogOpens(() => save({
    defaultPath,
    filters: [RELINK_SAVE_FILTER, { name: 'All files', extensions: ['*'] }],
  })).catch((err) => {
    console.error('Save dialog failed:', err);
    return null;
  }),
  write: writeTextFile,
  log: logFrontendEvent,
  toast: showToast,
  progress: createRelinkProgress(),
  now: () => Date.now(),
};

/**
 * @param {object} app  `getTakeIndex()` the open project's take index;
 *   `io` swaps the disk, dialogs, log, toasts and bar out in tests.
 * @returns {{ run: ({ fromBackup }?) => Promise<boolean> }}  resolves to
 *   whether a list was saved.
 */
export function createVegasRelinkExport({ getTakeIndex, io = defaultIo }) {
  async function run({ fromBackup = false } = {}) {
    try {
      return await exportList(fromBackup);
    } finally {
      io.progress?.hide();
    }
  }

  async function exportList(fromBackup) {
    const R = STRINGS.RENDER;
    const takeIndex = getTakeIndex() || {};
    const started = io.now();
    const source = fromBackup ? 'backup' : 'record';
    io.log(`[vegas-relink] export started (from the ${source === 'backup' ? 'project and a backup' : 'project'})`);
    const end = (why) => io.log(`[vegas-relink] no list saved: ${why} (${io.now() - started} ms)`);

    let candidates = movesFromRecord(takeIndex);
    let backupCounts = null;
    if (fromBackup) {
      io.progress?.show(stepView(R.RELINK_PICKING_BACKUP));
      const backupPath = await io.pickBackup();
      if (!backupPath || Array.isArray(backupPath)) {
        end('the backup pick was cancelled');
        return false;
      }
      io.progress?.show(stepView(R.relinkReadingBackup(fileNameOf(backupPath))));
      try {
        const text = await io.read(backupPath);
        const fromCopy = movesFromBackup(takeIndex, text);
        candidates = [...candidates, ...fromCopy.moves];
        backupCounts = { matched: fromCopy.matched, unmatched: fromCopy.unmatched };
      } catch (err) {
        io.toast(R.relinkBackupFailed(backupPath, err?.message || err), 'error');
        end(`the backup ${backupPath} could not be read: ${err?.message || err}`);
        return false;
      }
      io.log(`[vegas-relink] backup read: ${backupCounts.matched} render(s) matched, ${backupCounts.unmatched} not in this project`);
    }
    const moves = mergeMoves(takeIndex, candidates);
    if (moves.length === 0) {
      io.toast(fromBackup ? R.RELINK_NONE_BACKUP : R.RELINK_NONE, 'info');
      end('no video has moved');
      return false;
    }

    io.progress?.show(stepView(R.relinkChecking(moves.length)));
    const targets = [...new Set(moves.map((m) => m.to))];
    const there = await io.exists(targets);
    const missingTargets = Array.isArray(there) ? targets.filter((_, i) => there[i] === false).length : 0;

    io.progress?.show(stepView(R.RELINK_PICKING_SAVE));
    const path = await io.pickSave(relinkDefaultPath(takeIndex));
    if (!path) {
      end('the save dialog was cancelled');
      return false;
    }
    io.progress?.show(stepView(R.RELINK_WRITING));
    try {
      await io.write(path, `${JSON.stringify(relinkMap(moves), null, 2)}\n`);
    } catch (err) {
      io.toast(R.relinkWriteFailed(err), 'error');
      end(`could not write ${path}: ${err}`);
      return false;
    }
    const counts = moveCounts(moves);
    io.log(`[vegas-relink] saved ${path}: ${counts.moved} moved, ${missingTargets} new file(s) not there, in ${io.now() - started} ms`);
    io.toast(R.relinkDone({ ...counts, missingTargets }), missingTargets > 0 ? 'info' : 'success');
    return true;
  }

  return { run };
}

/** Wires the two buttons; the options are `createVegasRelinkExport`'s. */
export function startVegasRelinkExport(options) {
  const button = document.querySelector('#vegas-relink-export-btn');
  const backupButton = document.querySelector('#vegas-relink-backup-btn');
  const exporter = createVegasRelinkExport(options);
  const wire = (el, fromBackup) => el?.addEventListener('click', async () => {
    if (button) button.disabled = true;
    if (backupButton) backupButton.disabled = true;
    try {
      await exporter.run({ fromBackup });
    } catch (err) {
      console.error('VEGAS relink list export failed:', err);
      showToast(STRINGS.RENDER.relinkWriteFailed(err), 'error');
    } finally {
      if (button) button.disabled = false;
      if (backupButton) backupButton.disabled = false;
    }
  });
  wire(button, false);
  wire(backupButton, true);
}
