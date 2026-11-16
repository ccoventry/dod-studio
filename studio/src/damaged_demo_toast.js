// damaged_demo_toast.js
// The game's damaged-demo notice (#1373): when a demo the game plays turns out
// damaged (it stops at a damaged frame under playdemo, or viewdemo loads it
// only up to one), the hook DLL tells Studio over its game pipe
// (damaged_demo_notices.rs) and this shows a toast with a Repair button. It
// stays up until the user clicks Repair or closes it.
//
// A demo in the Master Demo Queue is repaired as its row's Repair does
// (queue_repair.js: the repaired copy takes its place and keeps its work);
// any other is repaired beside itself, as the Demo Auditor's Repair does, on
// the queue's progress line. While a capture batch runs the toast has no
// Repair (the batch is reading the demo, and a queued repair swaps rows the
// batch marks takes on), and Studio's own working copies (dodstudio_*.dem) get
// no toast at all: the user never chose them.

import { listen } from '@tauri-apps/api/event';
import { logFrontendEvent, repairDemo } from './ipc_bridge.js';
import { repairProgressView, repairResultText } from './repair_view.js';
import { noteRepairResult } from './repair_status.js';
import { queueProgress } from './queue_progress.js';
import { findQueuedDemo } from './review_from_game.js';
import { fileNameOf, samePath } from './path_display.js';
import { clockFloor, durationTextMs } from './time_format.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

const R = STRINGS.REPAIR;

/** The page's event for a notice (damaged_demo_notices.rs's DAMAGED_EVENT). */
export const DAMAGED_EVENT = 'game_demo_damaged';

/** A second notice for the same demo within this is not shown again (a
 *  batch, or the game loading it twice). */
const REPEAT_MS = 30000;

/** Where the demo stops, as the toast writes it, or '' when unknown. */
export function noticeClock(notice) {
  return Number.isFinite(notice?.seconds) ? clockFloor(notice.seconds) : '';
}

/** The toast's line for a notice; `held` while a batch runs. */
export function damagedToastText(notice, held = false) {
  const name = fileNameOf(notice?.demo || '');
  const clock = noticeClock(notice);
  return held ? R.gameDamagedDuringBatch(name, clock) : R.gameDamaged(name, clock);
}

/** Whether the demo is one of Studio's own working copies (a capture
 *  chain's dodstudio_chain_NN.dem, the primer, a probe's copy), which the
 *  user never chose and has no use repairing. */
export function isWorkingCopy(path) {
  return /^dodstudio_/i.test(fileNameOf(path || ''));
}

/**
 * Repairs the demo a notice names. `deps`: getDemos() the Master Demo Queue,
 * repairQueuedDemo(demo) queue_repair.js's, repair(path) (repairDemo),
 * listenProgress(fn) for repair_progress events, and progress (the queue's
 * line). Resolves the verdict ('repaired', 'not_damaged', ... or 'failed').
 */
export async function repairFromNotice(notice, {
  getDemos, repairQueuedDemo, repair = repairDemo,
  listenProgress = (fn) => listen('repair_progress', fn), progress = queueProgress(),
}) {
  const path = notice.demo;
  const queued = findQueuedDemo(getDemos(), path);
  if (queued) return (await repairQueuedDemo(queued)) ? 'repaired' : 'not repaired';
  const name = fileNameOf(path);
  const head = R.queueHead(name);
  const job = progress.begin({ text: head });
  const show = (p) => {
    const view = repairProgressView(p);
    progress.show(job, { pct: view.pct, text: `${head}: ${view.text}`, indeterminate: false });
  };
  show(null);
  logFrontendEvent(R.gameRepairStartLog(name));
  const unlisten = await listenProgress((event) => {
    if (samePath(event.payload?.path || '', path)) show(event.payload.progress);
  }).catch(() => () => {});
  let verdict;
  let endLine;
  try {
    const result = await repair(path);
    noteRepairResult(path, result);
    verdict = result?.verdict || 'unknown';
    endLine = repairResultText(result);
    if (endLine) showToast(endLine, verdict === 'repaired' ? 'success' : 'warning', 8000);
  } catch (err) {
    verdict = 'failed';
    endLine = R.queueFailed(name, err);
    showToast(endLine, 'error', 8000);
  } finally {
    unlisten();
  }
  const elapsed = durationTextMs(progress.ms(job));
  progress.end(job, endLine || null);
  logFrontendEvent(R.gameRepairEndLog(name, verdict, elapsed));
  return verdict;
}

/** Whether a notice should show: not one for the same demo shown in the last
 *  REPEAT_MS. `shown` maps a demo path to when its toast went up. */
export function shouldShow(notice, shown, now) {
  const at = shown.get(notice.demo);
  return at === undefined || now - at > REPEAT_MS;
}

/** Shows a toast for every notice the game sends. `deps` as for
 *  repairFromNotice, plus isHeld() (batch_guard.js): a batch runs. */
export function startDamagedDemoToasts(deps) {
  const shown = new Map();
  const isHeld = deps.isHeld || (() => false);
  listen(DAMAGED_EVENT, (event) => {
    const notice = event.payload;
    if (!notice?.demo) return;
    logFrontendEvent(R.gameDamagedLog(notice.demo, noticeClock(notice), notice.reason || ''));
    if (isWorkingCopy(notice.demo)) {
      logFrontendEvent(R.gameDamagedWorkingCopyLog(fileNameOf(notice.demo)));
      return;
    }
    const now = Date.now();
    if (!shouldShow(notice, shown, now)) return;
    shown.set(notice.demo, now);
    if (isHeld()) {
      showToast(damagedToastText(notice, true), 'warning', 0, { sticky: true });
      return;
    }
    showToast(damagedToastText(notice), 'warning', 0, {
      sticky: true,
      action: {
        label: R.GAME_DAMAGED_ACTION,
        onClick: () => {
          repairFromNotice(notice, deps)
            .catch((err) => showToast(R.queueFailed(fileNameOf(notice.demo), err), 'error', 8000));
        },
      },
    });
  }).catch((err) => console.error('Could not listen for damaged-demo notices:', err));
}
