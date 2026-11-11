// queue_repair.js
// Repairing a Master Demo Queue demo damaged partway (#15). Such a demo plays,
// and its scan covers it, up to its first gap; its row's buttons
// (queue_repair_hint.js) run these. Repair saves <name>_repaired.dem next to it
// (demo_repair_manager.rs), swaps the repaired demo into the queue in its
// place, scans it, and carries the highlights' work over by their kills, as a
// split does (split_carry.js): the repaired demo holds every highlight the
// original's scan listed, at the same times, and the ones after the gap too.

import { listen } from '@tauri-apps/api/event';
import { logFrontendEvent, repairDemo } from './ipc_bridge.js';
import { repairProgressView, repairResultText } from './repair_view.js';
import { SPLIT_SHARE, nest, placed, queueProgress } from './queue_progress.js';
import { applyCarriedWork, carryReport, matchSplitHighlights } from './split_carry.js';
import { repairThenSplit } from './repair_split.js';
import { noteRepairResult } from './repair_status.js';
import { repairCarryReport } from './repair_carry.js';
import { renameUidsInTakeIndex } from './take_index.js';
import { updateStreakVisuals } from './detail_pane.js';
import { showToast } from './toast.js';
import { durationTextMs } from './time_format.js';
import { fileNameOf, samePath } from './path_display.js';
import { STRINGS } from './strings.js';

const R = STRINGS.REPAIR;

/**
 * @param {object} app  What this needs from main.js: getDemos() the queue,
 *   removeDemo(demo) take one row out of it, scan(paths, opts) main.js's
 *   triggerAutoScan, getTakeIndex() the project's take index, and
 *   refreshQueue() redraw the queue and mark the project changed, and
 *   renderQueue() redraw it alone (a repair that found nothing to cut out
 *   takes the row's Repair away and gives its Split back).
 */
export function createQueueRepair({ getDemos, removeDemo, scan, getTakeIndex, refreshQueue, renderQueue }) {
  const progress = queueProgress();

  // The new demos are in the queue now: put the original's highlight work on
  // them, and say what didn't carry, in the activity log. `written` is
  // `[{ path, index }]` (a split's Written list; a repair alone is index 0),
  // `describe(result)` the toast and log lines for what carried.
  function carryWork(original, written, describe) {
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
    const report = describe(result);
    report.log.forEach((line) => logFrontendEvent(line));
    if (report.toast) showToast(report.toast.text, report.toast.type, 10000);
    refreshQueue();
  }

  // A repair that saved nothing says why; one that found nothing to cut out
  // redraws the queue, whose row then offers no Repair (repair_status.js).
  function notSaved(name, job, result) {
    const why = repairResultText(result);
    showToast(R.queueNotSaved(name, why), 'warning', 8000);
    progress.end(job, R.queueNotSaved(name, why));
    if (result?.verdict === 'not_damaged') renderQueue();
  }

  // The original file is kept; its row goes, and its highlights' work moves
  // to the repaired demo. Resolves whether it was repaired.
  async function repairQueuedDemo(demo, { update } = {}) {
    const name = fileNameOf(demo.path);
    const line = R.queueHead(name);
    const job = progress.begin({ text: line });
    const place = { job, range: [0, 100] };
    const repairPlace = nest(place, [0, SPLIT_SHARE]);
    const report = (p) => {
      const view = repairProgressView(p);
      progress.show(job, placed(repairPlace, { pct: view.pct, text: `${line}: ${view.text}`, indeterminate: false }));
      update?.(p);
    };
    report(null);
    logFrontendEvent(R.queueStartLog(name));
    const unlisten = await listen('repair_progress', (event) => {
      if (samePath(event.payload?.path || '', demo.path)) report(event.payload.progress);
    }).catch(() => () => {});
    let result;
    try {
      result = await repairDemo(demo.path);
    } catch (err) {
      showToast(R.queueFailed(name, err), 'error', 8000);
      progress.end(job, R.queueFailed(name, err));
      return false;
    } finally {
      unlisten();
    }
    noteRepairResult(demo.path, result);
    if (result.verdict !== 'repaired') {
      notSaved(name, job, result);
      return false;
    }
    removeDemo(demo);
    showToast(repairResultText(result), 'success', 6000);
    try {
      await scan([result.written], {
        pickedFiles: true,
        within: nest(place, [SPLIT_SHARE, 100], `${line}: ${R.QUEUE_ADDING}`),
      });
      carryWork(demo, [{ path: result.written, index: 0 }], (r) => repairCarryReport(demo, r, result.written));
    } finally {
      const elapsed = durationTextMs(progress.ms(job));
      progress.end(job, R.queueDoneLine(elapsed));
      logFrontendEvent(R.queueEndLog(name, elapsed));
    }
    return true;
  }

  // Repair & Split for a demo both damaged and multi-map: the repair, then a
  // split of it, on one bar; the original's row goes and its work moves to
  // the first map's piece, as a split's does. The _repaired.dem is kept.
  async function repairAndSplitQueuedDemo(demo, { update } = {}) {
    const name = fileNameOf(demo.path);
    const line = R.queueRepairSplitHead(name);
    const job = progress.begin({ text: line });
    const place = { job, range: [0, 100] };
    const jobPlace = nest(place, [0, SPLIT_SHARE]);
    const report = (view) => {
      progress.show(job, placed(jobPlace, { pct: view.pct, text: `${line}: ${view.text}`, indeterminate: false }));
      update?.(view);
    };
    logFrontendEvent(R.queueStartLog(name));
    let outcome;
    try {
      outcome = await repairThenSplit(demo.path, report);
    } catch (err) {
      showToast(R.queueFailed(name, err), 'error', 8000);
      progress.end(job, R.queueFailed(name, err));
      return false;
    }
    const { result, written } = outcome;
    if (!written) {
      notSaved(name, job, result);
      return false;
    }
    removeDemo(demo);
    showToast(R.repairSplitDone(result.holes.length, written.map((w) => fileNameOf(w.path))), 'success', 6000);
    try {
      await scan(written.map((w) => w.path), {
        pickedFiles: true,
        within: nest(place, [SPLIT_SHARE, 100], `${line}: ${R.QUEUE_REPAIR_SPLIT_ADDING}`),
      });
      carryWork(demo, written, (r) => carryReport(demo, r, written.map((w) => w.path)));
    } finally {
      const elapsed = durationTextMs(progress.ms(job));
      progress.end(job, R.queueRepairSplitDoneLine(elapsed, written.length));
      logFrontendEvent(R.queueEndLog(name, elapsed));
    }
    return true;
  }

  return { repairQueuedDemo, repairAndSplitQueuedDemo };
}
