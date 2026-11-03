// project_vegas_export.js
// The "Export Project to VEGAS" button on the Render tab (#1264): an EDL of
// every video rendered for the loaded project that is still on disk, taken
// from the project's own record (take_renders.js), so a take that was deleted
// still has its videos in the movie. Moved videos are found again by name in
// the render export folders; missing ones are counted in the toast.

import { save } from '@tauri-apps/plugin-dialog';
import { writeTextFile, logFrontendEvent, locateRenderedVideos } from './ipc_bridge.js';
import { whileDialogOpens } from './dialog_wait.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import { projectRenderedVideos, existingVideos } from './take_renders.js';
import { videosToExport, clipsOfVideos } from './project_vegas_logic.js';
import { vegasEdl, edlDefaultPath, EDL_SAVE_FILTER } from './vegas_edl.js';
import { probeLengths, saveManifest } from './vegas_edl_export.js';
import { nameplateCount } from './clip_manifest.js';
import { projectEdlStartLine, projectEdlExportedLine } from './project_log.js';

const defaultIo = {
  locate: locateRenderedVideos,
  probe: probeLengths,
  pickPath: (clips) => whileDialogOpens(() => save({
    defaultPath: edlDefaultPath(clips),
    filters: [EDL_SAVE_FILTER, { name: 'All files', extensions: ['*'] }],
  })).catch((err) => {
    console.error('Save dialog failed:', err);
    return null;
  }),
  write: writeTextFile,
  saveManifest,
  log: logFrontendEvent,
  toast: showToast,
  now: () => Date.now(),
};

/**
 * @param {object} app  `getTakeIndex()`, `getAllDemos()` the project's;
 *   `ffmpegPath()` the FFmpeg override, if any. `io` swaps the disk, dialog,
 *   log and toasts out in tests.
 * @returns {{ run: () => Promise<boolean> }}  `run` resolves to whether an
 *   EDL was saved.
 */
export function createProjectVegasExport({
  getTakeIndex, getAllDemos, ffmpegPath = () => null, io = defaultIo,
}) {
  async function run() {
    const takeIndex = getTakeIndex() || {};
    const demos = getAllDemos() || [];
    const recorded = projectRenderedVideos(takeIndex);
    if (recorded.length === 0) {
      io.toast(STRINGS.RENDER.exportProjectNone(0), 'info');
      return false;
    }
    const started = io.now();
    const { videos: chosen, scope } = videosToExport(recorded, demos);
    io.log(projectEdlStartLine(recorded.length, chosen.length, scope));
    io.toast(STRINGS.RENDER.exportProjectLooking(chosen.length), 'info');
    const located = await io.locate(chosen.map((video) => video.output_path));
    if (!located) {
      io.toast(STRINGS.RENDER.exportProjectLookFailed, 'error');
      io.log('[project] VEGAS export: the lookup for the rendered videos failed');
      return false;
    }
    const found = existingVideos(chosen, located);
    const missing = chosen.length - found.length;
    if (found.length === 0) {
      io.toast(STRINGS.RENDER.exportProjectNone(chosen.length), 'info');
      io.log(projectEdlExportedLine(0, scope, missing, 0, io.now() - started));
      return false;
    }
    io.toast(STRINGS.RENDER.exportEdlProbing(found.length), 'info');
    const lengths = await io.probe(found.map((video) => video.output_path), ffmpegPath());
    const { clips, skipped } = clipsOfVideos(found, takeIndex, lengths);
    if (clips.length === 0) {
      io.toast(STRINGS.RENDER.exportEdlNone(skipped), 'info');
      return false;
    }
    const path = await io.pickPath(clips);
    if (!path) return false;
    try {
      await io.write(path, vegasEdl(clips));
    } catch (err) {
      io.toast(STRINGS.RENDER.exportEdlFailed(err), 'error');
      io.log(`[project] VEGAS export could not write ${path}: ${err}`);
      return false;
    }
    const manifest = await io.saveManifest(path, clips, { takeIndex, demos });
    io.log(projectEdlExportedLine(clips.length, scope, missing, skipped, io.now() - started));
    io.toast(STRINGS.RENDER.exportProjectDone(clips.length, scope, missing, skipped, nameplateCount(manifest)), 'success');
    return true;
  }
  return { run };
}

/** Wires the button; the factory's options are `createProjectVegasExport`'s. */
export function startProjectVegasExport(options) {
  const button = document.querySelector('#export-project-edl-btn');
  const exporter = createProjectVegasExport(options);
  button?.addEventListener('click', async () => {
    button.disabled = true;
    try {
      await exporter.run();
    } catch (err) {
      console.error('Project VEGAS export failed:', err);
      showToast(STRINGS.RENDER.exportEdlFailed(err), 'error');
    } finally {
      button.disabled = false;
    }
  });
}
