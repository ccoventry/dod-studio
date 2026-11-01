// project_renders.js
//
// Keeps the project's record of its rendered videos (#1264, take_renders.js)
// up to date: a render that finishes is recorded at once, and opening a
// project picks up what the take folders' own histories (#438) hold for the
// takes it knows, so renders made while no project was open, or by a build
// from before the record, join it. There is no temporary session: the take
// folder's file already is the record while no project is loaded.

import { listen } from '@tauri-apps/api/event';
import { takeCaptureStates, logFrontendEvent } from './ipc_bridge.js';
import { captureOf } from './take_record.js';
import { recordRender, recordTakeFolderRenders } from './take_renders.js';
import { renderRecordedLine, rendersPickedUpLine, rendersPickUpFailedLine } from './project_log.js';

const defaultIo = { listen, fetchStates: takeCaptureStates, log: logFrontendEvent, now: () => Date.now() };

/** The takes of `takeIndex` whose folder the project recorded, as
 *  `{ takeKey, folder }`. */
export function knownTakeFolders(takeIndex) {
  return Object.keys(takeIndex || {})
    .map((takeKey) => ({ takeKey, folder: captureOf(takeIndex, takeKey)?.take_folder }))
    .filter((take) => !!take.folder);
}

/**
 * @param {object} app  `getTakeIndex()` the open project's take index,
 *   `markDirty()` to mark the project unsaved. `io` swaps the event
 *   listener, the disk check and the activity log out in tests.
 */
export function createProjectRenders({ getTakeIndex, markDirty, io = defaultIo }) {
  /** Records the render `render_take_finished` reports, for a take the
   *  project knows. */
  function noteFinished(payload) {
    const { take_key: takeKey, render } = payload || {};
    if (!recordRender(getTakeIndex(), takeKey, render)) return;
    io.log(renderRecordedLine(takeKey, render.output_path));
    markDirty();
  }

  /** Starts recording every render that finishes. */
  function startRecording() {
    Promise.resolve(io.listen('render_take_finished', (event) => noteFinished(event.payload)))
      .catch((err) => console.error('Failed to register render_take_finished listener (project renders):', err));
  }

  /**
   * Reads the render history of every take folder the open project knows and
   * records the finished renders it doesn't have yet. Runs after a project
   * opens, in the background: the project is usable while it reads, and its
   * answer is dropped if another project opened meanwhile. Resolves to how
   * many were added.
   */
  async function pickUpTakeFolderRenders() {
    const takeIndex = getTakeIndex();
    const takes = knownTakeFolders(takeIndex);
    if (takes.length === 0) return 0;
    const started = io.now();
    const states = await io.fetchStates(takes.map((take) => take.folder));
    if (!states) {
      io.log(rendersPickUpFailedLine(takes.length));
      return 0;
    }
    if (getTakeIndex() !== takeIndex) return 0;
    const added = takes.reduce((sum, take, i) =>
      sum + recordTakeFolderRenders(takeIndex, take.takeKey, states[i]?.renders), 0);
    io.log(rendersPickedUpLine(takes.length, added, io.now() - started));
    // The project now holds more than its file: saving keeps it.
    if (added > 0) markDirty();
    return added;
  }

  return { startRecording, noteFinished, pickUpTakeFolderRenders };
}
