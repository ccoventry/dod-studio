// repair_split.js
// Repair & Split (#15) for a demo both damaged partway and recorded over more
// than one map: a split can't read past the damage, so the demo is repaired
// first (demo_repair_manager.rs, <name>_repaired.dem beside it) and the
// repaired demo split with the existing split (split_demo_auto, every map of a
// minute or more). One job, one bar: the repair's share, then the split's,
// with a line saying which step it is on. The _repaired.dem is kept, as a
// Repair alone keeps it. Used by the Demo Auditor's Damaged Demos tab, the
// Master Queue's row and the Demo Analyzer's notice.

import { listen } from '@tauri-apps/api/event';
import { repairDemo, splitDemoAuto } from './ipc_bridge.js';
import { repairProgressView } from './repair_view.js';
import { noteRepairResult } from './repair_status.js';
import { splitProgressView } from './split_progress.js';
import { SHORT_MAP_SECONDS } from './split_pane.js';
import { samePath } from './path_display.js';
import { STRINGS } from './strings.js';

/** The repair's share of the bar, in percent; the split fills the rest. */
export const REPAIR_SHARE = 50;

/** The bar's view `{ pct, text }` for a repair's RepairProgress `p`. Pure. */
export function repairStepView(p) {
  const view = repairProgressView(p);
  return { pct: Math.round((view.pct * REPAIR_SHARE) / 100), text: STRINGS.REPAIR.repairSplitRepairing(view.text) };
}

/** The bar's view for the split's SplitProgress `p`. Pure. */
export function splitStepView(p) {
  const view = splitProgressView(p);
  return {
    pct: Math.round(REPAIR_SHARE + (view.pct * (100 - REPAIR_SHARE)) / 100),
    text: STRINGS.REPAIR.repairSplitSplitting(view.text),
  };
}

/** Runs `job` with `on(payload)` fed every `event` whose `path` is `path`. */
async function withProgress(event, path, on, job) {
  const unlisten = await listen(event, (e) => {
    if (samePath(e.payload?.path || '', path)) on(e.payload.progress);
  }).catch(() => () => {});
  try {
    return await job();
  } finally {
    unlisten();
  }
}

/**
 * Repairs the demo at `path`, then splits the repair. `update({ pct, text })`
 * follows both. Resolves `{ result, written }`: the RepairResult, and the
 * split's Written list, or null when the repair saved nothing (then nothing
 * was split). A "not damaged" verdict is remembered (repair_status.js), so
 * the demo is no longer offered Repair. Throws when either step fails
 * outright.
 */
export async function repairThenSplit(path, update = () => {}) {
  update(repairStepView(null));
  const result = await withProgress('repair_progress', path, (p) => update(repairStepView(p)), () => repairDemo(path));
  noteRepairResult(path, result);
  if (result.verdict !== 'repaired') return { result, written: null };
  update(splitStepView({ fraction: 0, stage: 'reading' }));
  const written = await withProgress('split_progress', result.written, (p) => update(splitStepView(p)),
    () => splitDemoAuto(result.written, SHORT_MAP_SECONDS));
  return { result, written };
}
