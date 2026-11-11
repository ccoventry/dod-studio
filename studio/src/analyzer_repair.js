// analyzer_repair.js
// The Demo Analyzer's Repair now (#15), on the damaged-demo notice
// (analyzer_damage.js): repairs the demo on show into <name>_repaired.dem
// beside it (demo_repair_manager.rs), lists it in the Explorer when it shows
// that folder, and opens it.

import { listen } from '@tauri-apps/api/event';
import { repairDemo } from './ipc_bridge.js';
import { repairResultText } from './repair_view.js';
import { showToast } from './toast.js';
import { samePath } from './path_display.js';

/**
 * Repairs the demo at `path`, feeding `update(RepairProgress)`. `app` holds
 * `relistFolderOf(path)` and `open(path)` from analyzer_pane.js. Resolves the
 * line the notice shows after; throws when the repair fails outright.
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
  const text = repairResultText(result);
  if (result.verdict !== 'repaired') return text;
  showToast(text, 'success', 6000);
  await relistFolderOf(path);
  await open(result.written);
  return text;
}
