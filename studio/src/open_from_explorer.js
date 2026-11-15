// open_from_explorer.js
// Wires the files Windows hands Studio (#528) once startup is done: the
// Settings → Pipeline controls for `.dem` Open With, then launch_files.js,
// which opens a double-clicked project or demo. Kept out of main.js, which
// only wires (#683).

import { openAnalyzerDemo } from './analyzer_pane.js';
import { createLaunchFiles } from './launch_files.js';
import { initFileTypeSettings, getDemOpenTarget } from './file_types_settings.js';

/**
 * @param {object} app  openProject(path) from project_session.js,
 *   addDemosToQueue(paths) and isScanning() from queue_scan.js, persist()
 *   saves the settings file.
 */
export function startOpenFromExplorer({ openProject, addDemosToQueue, isScanning, persist }) {
  initFileTypeSettings({ onSettingsChange: persist });
  return createLaunchFiles({
    openProject,
    openDemoInAnalyzer: openAnalyzerDemo,
    addDemosToQueue,
    getDemTarget: getDemOpenTarget,
    isScanning,
  }).start();
}
