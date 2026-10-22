// queue_split.js
// Splitting a Master Demo Queue demo that recorded more than one map (#217).
// The game's viewdemo stops at a demo's first map change, so nothing past it
// can be captured and its highlights cover one map only. Its row's Split
// button writes every map of a minute or more as a demo of its own (the Demo
// Auditor's Split Maps) and scans them into the queue in its place; Start
// Capture Batch refuses picks in such a demo and offers the same split.
// The highlights' work moves to the pieces (split_carry.js).
// Which demos and which maps: queue_multimap.js and analyzer_multimap.js.

import { listen } from '@tauri-apps/api/event';
import { logFrontendEvent, splitDemoAuto } from './ipc_bridge.js';
import { pickedMultiMapDemos } from './queue_multimap.js';
import { SHORT_MAP_SECONDS } from './split_pane.js';
import { splitProgressView } from './split_progress.js';
import { applyCarriedWork, carryReport, matchSplitHighlights } from './split_carry.js';
import { renameUidsInTakeIndex } from './take_index.js';
import { updateStreakVisuals } from './detail_pane.js';
import { themedConfirm } from './themed_confirm.js';
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
  // The queue's status line (beside the scan spinner) says how the split is
  // going whatever started it; `update(SplitProgress)`, when given, also
  // feeds the row's own bar.
  function showStatus(name, progress) {
    const status = document.querySelector('#scan-status');
    const spinner = document.querySelector('#scan-spinner');
    if (status) status.textContent = progress ? STRINGS.MAIN.queueSplitStatus(name, splitProgressView(progress).text) : '';
    if (spinner) spinner.style.display = progress ? 'inline-block' : 'none';
  }

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
    const report = (p) => {
      showStatus(name, p);
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
      return false;
    } finally {
      unlisten();
      showStatus(name, null);
    }
    removeDemo(demo);
    showToast(STRINGS.MAIN.queueSplitDone(name, written.map((w) => fileNameOf(w.path))), 'success', 6000);
    await scan(written.map((w) => w.path), { pickedFiles: true });
    carryWork(demo, written);
    return true;
  }

  // Start Capture Batch's demo checks: picks in a demo with more than one map
  // first, then missing or changed demos (#21). Resolves true to go ahead,
  // false when a demo is missing (capture_pane.js says so), or 'handled' when
  // this already told the user why not.
  async function captureDemosReady() {
    const multi = pickedMultiMapDemos(getDemos());
    if (multi.length === 0) return pickedDemosPresent();
    const split = await themedConfirm(STRINGS.MAIN.MULTI_MAP_PICKED_MESSAGE, {
      title: STRINGS.MAIN.MULTI_MAP_PICKED_TITLE,
      confirmLabel: STRINGS.MAIN.MULTI_MAP_PICKED_SPLIT,
      cancelLabel: STRINGS.MAIN.RELOCATE_CANCEL_PLAIN,
      details: multi.map((d) => ({ primary: fileNameOf(d.path), secondary: d.signon_maps.join(', '), title: d.path })),
      footer: STRINGS.MAIN.MULTI_MAP_PICKED_QUESTION,
    });
    if (split) {
      for (const d of multi) await splitQueuedDemo(d);
    }
    return 'handled';
  }

  return { splitQueuedDemo, captureDemosReady };
}
