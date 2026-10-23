// launch_files.js
// Files Windows asks DoD Studio to open (#528): a `.dodstudio` project
// double-clicked in Explorer, or a `.dem` opened with Open With. The backend
// keeps them (open_files.rs) until this asks, once at startup and again each
// time a later double-click hands the running window more.
//
// A project opens through project_session.js's openProject, the same path as
// Load Project (unsaved-changes prompt, missing-demo check, rescan offer).
// A demo goes where Settings says: the Demo Analyzer, or Studio's queue.

import { listen } from '@tauri-apps/api/event';
import { takeOpenFiles } from './ipc_bridge.js';
import { isProjectFile, isDemoFile } from './project_file_type.js';

export const DEM_TARGETS = ['analyzer', 'queue'];

/** `paths` sorted into what to do: the project to open (the first one; a
 *  window holds one project), and the demos. Anything else is dropped. */
export function routeOpenedFiles(paths) {
  const list = Array.isArray(paths) ? paths.filter((p) => typeof p === 'string' && p) : [];
  return {
    project: list.find(isProjectFile) || null,
    demos: list.filter(isDemoFile),
  };
}

/** The saved setting made safe: the Demo Analyzer unless it says `queue`. */
export function normalizeDemTarget(raw) {
  return DEM_TARGETS.includes(raw) ? raw : 'analyzer';
}

/**
 * @param {object} app  What this needs from main.js: openProject(path) from
 *   project_session.js, openDemoInAnalyzer(path), addDemosToQueue(paths)
 *   (queue_scan.js's triggerAutoScan), and getDemTarget(), the setting.
 *   `take` and `listenFor` are swappable for tests.
 */
export function createLaunchFiles({
  openProject,
  openDemoInAnalyzer,
  addDemosToQueue,
  getDemTarget,
  take = takeOpenFiles,
  listenFor = listen,
}) {
  // One batch at a time: a second double-click while the unsaved-changes
  // prompt of the first is open waits for it.
  let chain = Promise.resolve();

  async function openBatch() {
    const { project, demos } = routeOpenedFiles(await take());
    if (project) await openProject(project);
    if (demos.length === 0) return;
    if (normalizeDemTarget(getDemTarget()) === 'queue') {
      await addDemosToQueue(demos);
    } else {
      // The Analyzer shows one demo: the last one opened.
      await openDemoInAnalyzer(demos[demos.length - 1]);
    }
  }

  function drain() {
    chain = chain.then(openBatch).catch((err) => console.error('Opening files from Explorer failed:', err));
    return chain;
  }

  /** Listens for files handed over later, then opens the ones Studio was
   *  started with. Call once the window is ready. */
  async function start() {
    try {
      await listenFor('open_files_pending', () => drain());
    } catch (err) {
      console.error('Could not listen for files opened from Explorer:', err);
    }
    return drain();
  }

  return { start, drain };
}
