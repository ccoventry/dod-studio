// clip_relink.js
//
// The Render tab's "Find Moved Videos" button (#1442): finds the loaded
// project's rendered videos by the DoD Studio tag on each (in the file, its
// NTFS stream, or size + fingerprint) in the render export folders, and
// points the project at their new place. When some are still not found, the
// toast offers a folder to look in. The scan and its activity-log lines are
// the backend's (`relink_rendered_clips`); this runs it, shows its progress
// and writes what it found into the project's record.

import { listen } from '@tauri-apps/api/event';
import { relinkRenderedClips, cancelRelinkRenderedClips, logFrontendEvent } from './ipc_bridge.js';
import { pickPath } from './pick_path.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import { showBar, hideBar } from './progress_view.js';
import { projectRenderedVideos } from './take_renders.js';
import {
  clipsToFind, applyRelinkReport, relinkProgressView, relinkAppliedLine, relinkResultSummary,
} from './clip_relink_logic.js';

/** How long the result toast stays when it offers a folder to look in. */
const PICK_TOAST_MS = 10000;

/** Draws the scan's bar and line; a page without them (a test) shows nothing. */
function createRelinkProgress(doc = globalThis.document) {
  const bar = () => doc?.querySelector('#find-moved-progress');
  const line = () => doc?.querySelector('#find-moved-status');
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
  listen,
  relink: relinkRenderedClips,
  cancel: cancelRelinkRenderedClips,
  pickFolder: () => pickPath({
    field: 'render.find_moved_videos',
    directory: true,
    multiple: false,
    title: STRINGS.RENDER.FIND_MOVED_PICK_TITLE,
  }).catch((err) => {
    console.error('Folder dialog failed:', err);
    return null;
  }),
  log: logFrontendEvent,
  toast: showToast,
  progress: createRelinkProgress(),
};

/**
 * @param {object} app  `getTakeIndex()` the open project's take index,
 *   `markDirty()` to mark the project unsaved, `onBusy(busy)` told when a
 *   scan starts and ends (the button greys out for a scan the toast started
 *   too). `io` swaps the backend, the dialog, the log, the toasts and the bar
 *   out in tests.
 * @returns {{ run: (extraFolder?: string|null) => Promise<object|null>,
 *   pickAndRun: () => Promise<object|null> }}  each resolves to
 *   `applyRelinkReport`'s counts, or null when nothing ran.
 */
export function createClipRelink({ getTakeIndex, markDirty, onBusy = () => {}, io = defaultIo }) {
  let busy = false;

  /** Scans the export folders and `extraFolder`, and updates the project. */
  async function run(extraFolder = null) {
    if (busy) return null;
    busy = true;
    onBusy(true);
    let unlisten = null;
    try {
      const takeIndex = getTakeIndex() || {};
      const videos = projectRenderedVideos(takeIndex);
      if (videos.length === 0) {
        io.toast(STRINGS.RENDER.findMovedNone, 'info');
        return null;
      }
      io.progress?.show(relinkProgressView({ step: 'tagging', done: 0, total: videos.length }));
      unlisten = await Promise.resolve(io.listen('clip_relink_progress', (event) => {
        io.progress?.show(relinkProgressView(event.payload));
      })).catch((err) => {
        console.error('Failed to register clip_relink_progress listener:', err);
        return null;
      });
      let report;
      try {
        report = await io.relink(clipsToFind(videos), extraFolder);
      } catch (err) {
        io.toast(STRINGS.RENDER.findMovedFailed(err), 'error');
        return null;
      }
      // Another project opened meanwhile: this answer is not its.
      if (getTakeIndex() !== takeIndex) return null;
      const counts = applyRelinkReport(takeIndex, videos, report);
      io.log(relinkAppliedLine(counts, videos.length));
      if (counts.changed > 0) markDirty();
      showResult(relinkResultSummary(counts, videos.length, report));
      return counts;
    } finally {
      if (typeof unlisten === 'function') unlisten();
      io.progress?.hide();
      busy = false;
      onBusy(false);
    }
  }

  /** The result toast; with videos still not found, it offers a folder. */
  function showResult(summary) {
    const text = STRINGS.RENDER.findMovedDone(summary);
    if (summary.cancelled || summary.notFound === 0) {
      io.toast(text, summary.cancelled ? 'info' : 'success');
      return;
    }
    io.toast(text, 'info', PICK_TOAST_MS, {
      action: { label: STRINGS.RENDER.FIND_MOVED_PICK_ACTION, onClick: () => { pickAndRun(); } },
    });
  }

  /** Asks for a folder, then scans it with the export folders. */
  async function pickAndRun() {
    const folder = await io.pickFolder();
    if (!folder || Array.isArray(folder)) return null;
    return run(folder);
  }

  /** Stops the running scan at its next clip or file; it still reports
   *  what it found. */
  function cancel() {
    if (busy) Promise.resolve(io.cancel()).catch((err) => console.error('Cancel failed:', err));
  }

  return { run, pickAndRun, cancel };
}

/** Wires the button; the options are `createClipRelink`'s. */
export function startClipRelink(options) {
  const button = document.querySelector('#find-moved-videos-btn');
  const cancelButton = document.querySelector('#find-moved-cancel-btn');
  const relink = createClipRelink({
    ...options,
    onBusy: (busy) => {
      if (button) button.disabled = busy;
      if (cancelButton) cancelButton.hidden = !busy;
    },
  });
  cancelButton?.addEventListener('click', () => relink.cancel());
  button?.addEventListener('click', () => {
    relink.run().catch((err) => {
      console.error('Find Moved Videos failed:', err);
      showToast(STRINGS.RENDER.findMovedFailed(err), 'error');
    });
  });
  return relink;
}
