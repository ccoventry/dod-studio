// repair_carry.js
// What a repaired queued demo (#15) kept of the original's highlight work,
// for the toast and the activity log. The matching is a split's
// (split_carry.js): the repaired demo is the one piece, holding every
// highlight the original listed, at the same times. Pure.

import { workLossLines } from './split_carry.js';
import { fileNameOf } from './path_display.js';
import { STRINGS } from './strings.js';

/** The toast (null when nothing had work on it) and the activity log lines,
 *  one per highlight whose work didn't carry or whose Kill Range narrowed.
 *  `result` is matchSplitHighlights' for the repaired demo at `repairedPath`. */
export function repairCarryReport(original, result, repairedPath) {
  const R = STRINGS.REPAIR;
  const name = fileNameOf(original.path);
  const kept = result.carried.length;
  const total = result.withWork;
  if (total === 0) return { toast: null, log: [] };
  return {
    toast: { text: R.carryToast(name, kept, total), type: kept === total ? 'success' : 'warning' },
    log: [
      R.carryLog(name, kept, total, fileNameOf(repairedPath)),
      ...workLossLines(original, result, { narrowed: R.narrowedLog, uncarried: R.uncarriedLog, reasons: R.UNCARRIED_REASON }),
    ],
  };
}
