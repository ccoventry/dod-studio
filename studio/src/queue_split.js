// queue_split.js
// Splitting a Master Demo Queue demo that recorded more than one map (#217).
// The game's viewdemo stops at a demo's first map change, so nothing past it
// can be captured and its highlights cover one map only. Its row's Split
// button writes every map of a minute or more as a demo of its own (the Demo
// Auditor's Split Maps) and scans them into the queue in its place; Start
// Capture Batch refuses picks in such a demo and offers the same split.
// Which demos and which maps: queue_multimap.js and analyzer_multimap.js.

import { listen } from '@tauri-apps/api/event';
import { splitDemoAuto } from './ipc_bridge.js';
import { pickedMultiMapDemos } from './queue_multimap.js';
import { SHORT_MAP_SECONDS } from './split_pane.js';
import { splitProgressView } from './split_progress.js';
import { isDemoTracked } from './take_index.js';
import { themedConfirm } from './themed_confirm.js';
import { showToast } from './toast.js';
import { fileNameOf } from './path_display.js';
import { STRINGS } from './strings.js';

/**
 * @param {object} app  What this needs from main.js:
 *   getDemos() the queue, removeDemo(demo) take one row out of it,
 *   scan(paths, opts) main.js's triggerAutoScan, confirmTracked(demo) the
 *   tracked-work modal (resolves truthy to go ahead), and
 *   pickedDemosPresent() project_demos.js's missing/changed check.
 */
export function createQueueSplit({ getDemos, removeDemo, scan, confirmTracked, pickedDemosPresent }) {
  // The queue's status line (beside the scan spinner) says how the split is
  // going whatever started it; `update(SplitProgress)`, when given, also
  // feeds the row's own bar.
  function showStatus(name, progress) {
    const status = document.querySelector('#scan-status');
    const spinner = document.querySelector('#scan-spinner');
    if (status) status.textContent = progress ? STRINGS.MAIN.queueSplitStatus(name, splitProgressView(progress).text) : '';
    if (spinner) spinner.style.display = progress ? 'inline-block' : 'none';
  }

  // The file is kept; its row, and any work on it, goes. Resolves whether it
  // split.
  async function splitQueuedDemo(demo, { askIfTracked = true, update } = {}) {
    if (askIfTracked && isDemoTracked(demo) && !(await confirmTracked(demo))) return false;
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
      for (const d of multi) await splitQueuedDemo(d, { askIfTracked: false });
    }
    return 'handled';
  }

  return { splitQueuedDemo, captureDemosReady };
}
