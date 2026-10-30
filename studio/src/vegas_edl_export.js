// vegas_edl_export.js
// The "Export VEGAS EDL" button on the Render tab (#110): saves the finished
// renders as an EDL (`vegas_edl.js`) that VEGAS Pro imports.

import { save } from '@tauri-apps/plugin-dialog';
import { writeTextFile } from './ipc_bridge.js';
import { whileDialogOpens } from './dialog_wait.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import { JOB_STATUS } from './render_jobs.js';
import { edlClips, vegasEdl } from './vegas_edl.js';

/** Wires the button; `getJobs` returns the Render tab's latest job list. */
export function startVegasEdlExport({ getJobs }) {
  const button = document.querySelector('#export-vegas-edl-btn');
  button?.addEventListener('click', async () => {
    const { clips, skipped } = edlClips(getJobs(), JOB_STATUS.FINISHED);
    if (clips.length === 0) {
      showToast(STRINGS.RENDER.exportEdlNone(skipped), 'info');
      return;
    }
    const path = await whileDialogOpens(() => save({
      defaultPath: 'dod_highlights.edl',
      filters: [{ name: 'EDL', extensions: ['edl'] }],
    })).catch((err) => {
      console.error('Save dialog failed:', err);
      return null;
    });
    if (!path) return;
    try {
      await writeTextFile(path, vegasEdl(clips));
      showToast(STRINGS.RENDER.exportEdlDone(clips.length, skipped), 'success');
    } catch (err) {
      showToast(STRINGS.RENDER.exportEdlFailed(err), 'error');
    }
  });
}
