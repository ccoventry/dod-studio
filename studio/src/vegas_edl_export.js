// vegas_edl_export.js
// The "Export VEGAS EDL" button on the Render tab (#110): saves the finished
// renders as an EDL (`vegas_edl.js`) that VEGAS Pro imports, and beside it
// the clip manifest (`clip_manifest.js`, #429) the nameplate script reads.

import { save } from '@tauri-apps/plugin-dialog';
import { combinePlan, writeTextFile, logFrontendEvent } from './ipc_bridge.js';
import { whileDialogOpens } from './dialog_wait.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import { JOB_STATUS } from './render_jobs.js';
import { edlClips, pathsToProbe, partPaths, planEdlParts, vegasEdl } from './vegas_edl.js';
import { edlDefaultPath, EDL_SAVE_FILTER } from './vegas_edl.js';
import { clipManifest, manifestPathFor, nameplateCount } from './clip_manifest.js';
import { useAsName, nameplatePath } from './player_alias.js';
import { fileNameOf } from './path_display.js';

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
 *  `ffmpegPath()` the FFmpeg override, if any, `getTakeIndex()` and
 *  `getAllDemos()` the project's, for whose highlight each clip is. */
export function startVegasEdlExport({
  getJobs, ffmpegPath = () => null, getTakeIndex = () => ({}), getAllDemos = () => [],
}) {
  const button = document.querySelector('#export-vegas-edl-btn');
  button?.addEventListener('click', async () => {
    button.disabled = true;
    try {
      await exportEdl(getJobs(), ffmpegPath(), { takeIndex: getTakeIndex() || {}, demos: getAllDemos() || [] });
    } finally {
      button.disabled = false;
    }
  });
}

async function exportEdl(jobs, ffmpegPath, project) {
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
    defaultPath: edlDefaultPath(clips),
    filters: [EDL_SAVE_FILTER, { name: 'All files', extensions: ['*'] }],
  })).catch((err) => {
    console.error('Save dialog failed:', err);
    return null;
  });
  if (!path) return;
  try {
    const parts = planEdlParts(clips);
    const paths = partPaths(path, parts.length);
    for (let i = 0; i < parts.length; i += 1) {
      await writeTextFile(paths[i], vegasEdl(parts[i]));
    }
    await finishExport({ parts, paths, clips, skipped, project });
  } catch (err) {
    showToast(STRINGS.RENDER.exportEdlFailed(err), 'error');
  }
}

/**
 * The clip manifest beside each EDL file (`name_part1.dodstudio.json` for
 * `name_part1.txt`): the nameplate script reads every `*.dodstudio.json` near
 * a clip, so several part manifests need no merging. A manifest that can't be
 * saved doesn't undo the EDL.
 */
async function finishExport({ parts, paths, clips, skipped, project }) {
  let nameplates = 0;
  let manifestError = null;
  for (let i = 0; i < parts.length; i += 1) {
    const manifest = clipManifest(parts[i], {
      edlName: fileNameOf(paths[i]), ...project, useAsName, nameplatePath,
    });
    nameplates += nameplateCount(manifest);
    try {
      await writeTextFile(manifestPathFor(paths[i]), `${JSON.stringify(manifest, null, 2)}\n`);
    } catch (err) {
      manifestError = err;
    }
  }
  if (manifestError) showToast(STRINGS.RENDER.exportManifestFailed(manifestError), 'error');
  logFrontendEvent(`Exported VEGAS EDL: ${clips.length} clips in ${parts.length} file(s), ${skipped} left out`);
  showToast(STRINGS.RENDER.exportEdlDone(clips.length, skipped, nameplates, parts.length), 'success');
}
