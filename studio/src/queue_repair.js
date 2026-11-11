// queue_repair.js
// Repairing a Master Demo Queue demo damaged partway (#15). Such a demo plays,
// and its scan covers it, up to its first gap; its row gets a "Damaged ·
// Repair" button. Repair saves <name>_repaired.dem next to it
// (demo_repair_manager.rs), swaps the repaired demo into the queue in its
// place, scans it, and carries the highlights' work over by their kills, as a
// split does (split_carry.js): the repaired demo holds every highlight the
// original's scan listed, at the same times, and the ones after the gap too.

import { listen } from '@tauri-apps/api/event';
import { logFrontendEvent, repairDemo } from './ipc_bridge.js';
import { isDamagedDemo, repairProgressBar, repairProgressView, repairResultText } from './repair_view.js';
import { SPLIT_SHARE, nest, placed, queueProgress } from './queue_progress.js';
import { applyCarriedWork, matchSplitHighlights } from './split_carry.js';
import { repairCarryReport } from './repair_carry.js';
import { renameUidsInTakeIndex } from './take_index.js';
import { updateStreakVisuals } from './detail_pane.js';
import { showToast } from './toast.js';
import { durationTextMs, clockFloor } from './time_format.js';
import { fileNameOf, samePath } from './path_display.js';
import { STRINGS } from './strings.js';

const R = STRINGS.REPAIR;

/** The row's "Damaged · Repair" button, or null for a whole demo. Clicking it
 *  runs `onRepair(demo, update)` and doesn't select the row; the first
 *  `update(RepairProgress)` swaps the button for a progress bar until it ends. */
export function damagedRepairButton(demo, onRepair) {
  if (!isDamagedDemo(demo) || !onRepair) return null;
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = 'damaged-repair-btn';
  btn.textContent = R.QUEUE_BUTTON;
  btn.title = R.queueButtonTitle(clockFloor(demo.damage.read_up_to_secs));
  btn.addEventListener('click', async (e) => {
    e.stopPropagation(); // do not select the row
    btn.disabled = true;
    let bar = null;
    const update = (p) => {
      if (!bar) {
        bar = repairProgressBar();
        bar.el.classList.add('multimap-split-progress');
        btn.after(bar.el);
        btn.hidden = true;
      }
      bar.update(p);
    };
    try {
      await onRepair(demo, update);
    } finally {
      bar?.el.remove();
      btn.hidden = false;
      btn.disabled = false;
    }
  });
  return btn;
}

/**
 * @param {object} app  What this needs from main.js: getDemos() the queue,
 *   removeDemo(demo) take one row out of it, scan(paths, opts) main.js's
 *   triggerAutoScan, getTakeIndex() the project's take index, and
 *   refreshQueue() redraw the queue and mark the project changed.
 */
export function createQueueRepair({ getDemos, removeDemo, scan, getTakeIndex, refreshQueue }) {
  const progress = queueProgress();

  // The repaired demo is in the queue now: put the original's highlight work
  // on it, and say what didn't carry, in the activity log.
  function carryWork(original, repairedPath) {
    const demo = getDemos().find((d) => samePath(d.path, repairedPath));
    const pieces = demo ? [{ path: demo.path, map_name: demo.map_name, streaks: demo.streaks, splitIndex: 0 }] : [];
    const result = matchSplitHighlights(original, pieces);
    const renames = applyCarriedWork(original, result.carried);
    result.carried.forEach((c) => updateStreakVisuals(c.to));
    renameUidsInTakeIndex(getTakeIndex(), renames);
    const report = repairCarryReport(original, result, repairedPath);
    report.log.forEach((line) => logFrontendEvent(line));
    if (report.toast) showToast(report.toast.text, report.toast.type, 10000);
    refreshQueue();
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
    if (result.verdict !== 'repaired') {
      const why = repairResultText(result);
      showToast(R.queueNotSaved(name, why), 'warning', 8000);
      progress.end(job, R.queueNotSaved(name, why));
      return false;
    }
    removeDemo(demo);
    showToast(repairResultText(result), 'success', 6000);
    try {
      await scan([result.written], {
        pickedFiles: true,
        within: nest(place, [SPLIT_SHARE, 100], `${line}: ${R.QUEUE_ADDING}`),
      });
      carryWork(demo, result.written);
    } finally {
      const elapsed = durationTextMs(progress.ms(job));
      progress.end(job, R.queueDoneLine(elapsed));
      logFrontendEvent(R.queueEndLog(name, elapsed));
    }
    return true;
  }

  return { repairQueuedDemo };
}
