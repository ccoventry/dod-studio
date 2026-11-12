// analyzer_repair.js
// The Demo Analyzer's Repair now and Repair & Split (#15), on the
// damaged-demo notice (analyzer_damage.js): repairs the demo on show into
// <name>_repaired.dem beside it (demo_repair_manager.rs), and for Repair &
// Split splits that too (repair_split.js), then lists the new demos in the
// Explorer when it shows that folder and opens the first.

import { listen } from '@tauri-apps/api/event';
import { repairDemo } from './ipc_bridge.js';
import { repairResultText } from './repair_view.js';
import { noteRepairResult } from './repair_status.js';
import { repairThenSplit } from './repair_split.js';
import { fileNameOf } from './path_display.js';
import { STRINGS } from './strings.js';
import { showToast } from './toast.js';
import { samePath } from './path_display.js';

/**
 * Repairs the demo at `path`, feeding `update(RepairProgress)`. `app` holds
 * `relistFolderOf(path)` and `open(path)` from analyzer_pane.js. Resolves the
 * line the notice shows after; throws when the repair fails outright. A "not
 * damaged" verdict is remembered (repair_status.js) for the notices' redraw.
 */
export async function repairAnalyzedDemo(path, update, { relistFolderOf, open }) {
  const unlisten = await listen('repair_progress', (event) => {
    if (samePath(event.payload?.path || '', path)) update(event.payload.progress);
  }).catch(() => () => {});
  let result;
  try {
    result = await repairDemo(path);
  } finally {
    unlisten();
  }
  noteRepairResult(path, result);
  const text = repairResultText(result);
  if (result.verdict !== 'repaired') return text;
  showToast(text, 'success', 6000);
  await relistFolderOf(path);
  await open(result.written);
  return text;
}

/**
 * Repair & Split on the demo at `path`, `update({ pct, text })` following
 * both steps; `app` as for repairAnalyzedDemo. Resolves the notice's line
 * after; opens the first map's demo when it split.
 */
export async function repairSplitAnalyzedDemo(path, update, { relistFolderOf, open }) {
  const { result, written } = await repairThenSplit(path, update);
  if (!written) return repairResultText(result);
  const text = STRINGS.REPAIR.repairSplitDone(result.holes.length, written.map((w) => fileNameOf(w.path)));
  showToast(text, 'success', 6000);
  await relistFolderOf(path);
  if (written.length) await open(written[0].path);
  return text;
}
