// vegas_edl_export.js
// The "Export VEGAS EDL" button on the Render tab (#110): saves the finished
// renders as an EDL (`vegas_edl.js`) that VEGAS Pro imports.

import { save } from '@tauri-apps/plugin-dialog';
import { combinePlan, writeTextFile } from './ipc_bridge.js';
import { whileDialogOpens } from './dialog_wait.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import { JOB_STATUS } from './render_jobs.js';
import { edlClips, pathsToProbe, vegasEdl } from './vegas_edl.js';

/**
 * The length of each file in `paths` as FFmpeg reads it (the Combine Clips
 * probe), one file at a time so one unreadable file leaves only itself out.
 * @returns {Promise<Map<string, number>>} path to seconds, readable files only
 */
async function probeLengths(paths, ffmpegPath) {
  const lengths = new Map();
  for (const path of paths) {
    try {
      const plan = await combinePlan([path], ffmpegPath);
      const seconds = plan?.clips?.[0]?.duration_secs;
      if (seconds > 0) lengths.set(path, seconds);
    } catch {
      // Left out of the EDL and counted in the toast's "left out".
    }
  }
  return lengths;
}

/** Wires the button; `getJobs` returns the Render tab's latest job list,
 *  `ffmpegPath()` the FFmpeg override, if any. */
export function startVegasEdlExport({ getJobs, ffmpegPath = () => null }) {
  const button = document.querySelector('#export-vegas-edl-btn');
  button?.addEventListener('click', async () => {
    button.disabled = true;
    try {
      await exportEdl(getJobs(), ffmpegPath());
    } finally {
      button.disabled = false;
    }
  });
}

async function exportEdl(jobs, ffmpegPath) {
  // A copied or captured video's length isn't in its settings line: its file
  // says it (#110).
  const toProbe = pathsToProbe(jobs, JOB_STATUS.FINISHED);
  if (toProbe.length > 0) showToast(STRINGS.RENDER.exportEdlProbing(toProbe.length), 'info');
  const probed = await probeLengths(toProbe, ffmpegPath);
  const { clips, skipped } = edlClips(jobs, JOB_STATUS.FINISHED, probed);
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
}
