// queue_split.js
// Splitting a Master Demo Queue demo that recorded more than one map (#217).
// Until it's split, such a demo is used for its first map only: its
// highlights come from that map and a capture plays it like any other demo.
// Its row's Split button writes every map of a minute or more as a demo of
// its own (the Demo Auditor's Split Maps) and scans them into the queue in
// its place, so the later maps can be used too. The highlights' work moves to
// the pieces (split_carry.js). Start Capture Batch never refuses such a demo;
// it only says in the activity log that the first map is used.
// Which demos and which maps: queue_multimap.js and analyzer_multimap.js.

import { listen } from '@tauri-apps/api/event';
import { logFrontendEvent, splitDemoAuto } from './ipc_bridge.js';
import { pickedMultiMapDemos } from './queue_multimap.js';
import { SHORT_MAP_SECONDS } from './split_pane.js';
import { splitProgressView } from './split_progress.js';
import { SPLIT_SHARE, nest, placed, queueProgress } from './queue_progress.js';
import { durationTextMs } from './time_format.js';
import { applyCarriedWork, carryReport, matchSplitHighlights } from './split_carry.js';
import { renameUidsInTakeIndex } from './take_index.js';
import { updateStreakVisuals } from './detail_pane.js';
import { showToast } from './toast.js';
import { fileNameOf, samePath } from './path_display.js';
import { STRINGS } from './strings.js';

/**
 * @param {object} app  What this needs from main.js:
 *   getDemos() the queue, removeDemo(demo) take one row out of it,
 *   scan(paths, opts) main.js's triggerAutoScan, getTakeIndex() the
 *   project's take index, refreshQueue() redraw the queue and the selected
 *   demo and mark the project changed, and pickedDemosPresent()
 *   project_demos.js's missing/changed check.
 */
export function createQueueSplit({ getDemos, removeDemo, scan, getTakeIndex, refreshQueue, pickedDemosPresent }) {
  // The queue's line and bar (queue_progress.js) say how the split is going,
  // the scan of the maps it writes included (#951); `update(SplitProgress)`,
  // when given, also feeds the row's own bar.
  const progress = queueProgress();

  // The pieces are in the queue now: put the original's highlight work on
  // them, and say what didn't carry, in the activity log.
  function carryWork(original, written) {
    const pieces = written
      .map((w) => {
        const demo = getDemos().find((d) => samePath(d.path, w.path));
        return demo && { path: demo.path, map_name: demo.map_name, streaks: demo.streaks, splitIndex: w.index };
      })
      .filter(Boolean);
    const result = matchSplitHighlights(original, pieces);
    const renames = applyCarriedWork(original, result.carried);
    result.carried.forEach((c) => updateStreakVisuals(c.to));
    renameUidsInTakeIndex(getTakeIndex(), renames);
    const report = carryReport(original, result, written.map((w) => w.path));
    report.log.forEach((line) => logFrontendEvent(line));
    if (report.toast) showToast(report.toast.text, report.toast.type, 10000);
    refreshQueue();
  }

  // The file is kept; its row goes, and its highlights' work moves to the
  // pieces. Resolves whether it split.
  async function splitQueuedDemo(demo, { update } = {}) {
    const name = fileNameOf(demo.path);
    const line = STRINGS.MAIN.queueSplitHead(name);
    const job = progress.begin({ text: line });
    const place = { job, range: [0, 100] };
    const splitPlace = nest(place, [0, SPLIT_SHARE]);
    const report = (p) => {
      const view = splitProgressView(p);
      progress.show(job, placed(splitPlace, { pct: view.pct, text: `${line}: ${view.text}`, indeterminate: false }));
      update?.(p);
    };
    report({ fraction: 0, stage: 'reading' });
    const unlisten = await listen('split_progress', (event) => {
      if (event.payload?.path === demo.path) report(event.payload.progress);
    }).catch(() => () => {});
    let written;
    try {
      written = await splitDemoAuto(demo.path, SHORT_MAP_SECONDS);
    } catch (err) {
      showToast(STRINGS.MAIN.queueSplitFailed(name, err), 'error', 8000);
      progress.end(job, STRINGS.MAIN.queueSplitFailed(name, err));
      return false;
    } finally {
      unlisten();
    }
    removeDemo(demo);
    showToast(STRINGS.MAIN.queueSplitDone(name, written.map((w) => fileNameOf(w.path))), 'success', 6000);
    try {
      await scan(written.map((w) => w.path), {
        pickedFiles: true,
        within: nest(place, [SPLIT_SHARE, 100], `${line}: ${STRINGS.MAIN.QUEUE_SPLIT_ADDING}`),
      });
      carryWork(demo, written);
    } finally {
      progress.end(job, STRINGS.MAIN.queueSplitDoneLine(durationTextMs(progress.ms(job))));
    }
    return true;
  }

  // Start Capture Batch's demo check: missing or changed demos (#21), as
  // project_demos.js answers it. A picked demo with more than one map goes
  // ahead like any other, on its first map; the activity log says how many
  // there are once the batch is cleared to start. `options.onProgress`
  // counts the changed-demo check (#953).
  async function captureDemosReady(options = {}) {
    const ready = await pickedDemosPresent(options);
    const multi = pickedMultiMapDemos(getDemos());
    if (ready === true && multi.length > 0) logFrontendEvent(STRINGS.MAIN.captureFirstMapOnly(multi.length));
    return ready;
  }

  return { splitQueuedDemo, captureDemosReady };
}
