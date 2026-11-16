// damaged_demo_toast.js
// The game's damaged-demo notice (#1373): when a demo the game plays turns out
// damaged (it stops at a damaged frame under playdemo, or viewdemo loads it
// only up to one), the hook DLL tells Studio over its game pipe
// (damaged_demo_notices.rs) and this shows a toast with a Repair button. A
// demo in the Master Demo Queue is repaired as its row's Repair does
// (queue_repair.js: the repaired copy takes its place and keeps its work);
// any other is repaired beside itself, as the Demo Auditor's Repair does.

import { listen } from '@tauri-apps/api/event';
import { logFrontendEvent, repairDemo } from './ipc_bridge.js';
import { repairResultText } from './repair_view.js';
import { noteRepairResult } from './repair_status.js';
import { findQueuedDemo } from './review_from_game.js';
import { fileNameOf } from './path_display.js';
import { clockFloor, durationTextMs } from './time_format.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

const R = STRINGS.REPAIR;

/** The page's event for a notice (damaged_demo_notices.rs's DAMAGED_EVENT). */
export const DAMAGED_EVENT = 'game_demo_damaged';

/** How long the toast stays: long enough to reach for Repair. */
export const TOAST_MS = 20000;

/** A second notice for the same demo within this is not shown again (a
 *  batch, or the game loading it twice). */
const REPEAT_MS = 30000;

/** Where the demo stops, as the toast writes it, or '' when unknown. */
export function noticeClock(notice) {
  return Number.isFinite(notice?.seconds) ? clockFloor(notice.seconds) : '';
}

/** The toast's line for a notice. */
export function damagedToastText(notice) {
  return R.gameDamaged(fileNameOf(notice?.demo || ''), noticeClock(notice));
}

/**
 * Repairs the demo a notice names. `deps`: getDemos() the Master Demo Queue,
 * repairQueuedDemo(demo) queue_repair.js's, and repair(path) (repairDemo).
 * Resolves the verdict ('repaired', 'not_damaged', ... or 'failed').
 */
export async function repairFromNotice(notice, { getDemos, repairQueuedDemo, repair = repairDemo }) {
  const path = notice.demo;
  const queued = findQueuedDemo(getDemos(), path);
  if (queued) return (await repairQueuedDemo(queued)) ? 'repaired' : 'not repaired';
  const name = fileNameOf(path);
  const started = Date.now();
  logFrontendEvent(R.gameRepairStartLog(name));
  showToast(R.gameRepairing(name), 'info', 4000);
  let verdict;
  try {
    const result = await repair(path);
    noteRepairResult(path, result);
    verdict = result?.verdict || 'unknown';
    const text = repairResultText(result);
    if (text) showToast(text, verdict === 'repaired' ? 'success' : 'warning', 8000);
  } catch (err) {
    verdict = 'failed';
    showToast(R.queueFailed(name, err), 'error', 8000);
  }
  logFrontendEvent(R.gameRepairEndLog(name, verdict, durationTextMs(Date.now() - started)));
  return verdict;
}

/** Whether a notice should show: not one for the same demo shown in the last
 *  REPEAT_MS. `shown` maps a demo path to when its toast went up. */
export function shouldShow(notice, shown, now) {
  const at = shown.get(notice.demo);
  return at === undefined || now - at > REPEAT_MS;
}

/** Shows a toast with Repair for every notice the game sends. `deps` as for
 *  repairFromNotice. */
export function startDamagedDemoToasts(deps) {
  const shown = new Map();
  listen(DAMAGED_EVENT, (event) => {
    const notice = event.payload;
    if (!notice?.demo) return;
    logFrontendEvent(R.gameDamagedLog(notice.demo, noticeClock(notice), notice.reason || ''));
    const now = Date.now();
    if (!shouldShow(notice, shown, now)) return;
    shown.set(notice.demo, now);
    showToast(damagedToastText(notice), 'warning', TOAST_MS, {
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
