// repair_status.js
// Whether a demo the analyzer flags as damaged (#811) has anything for the
// repair (#15) to cut out (native's demo_repairable: its frames stop reading
// before the end of the file). Damage inside a frame whose header reads stops
// the analysis but is no such break, and Repair would only say "not damaged".
// The Master Queue and the Demo Analyzer ask before they offer Repair; until
// the answer is in, or when the check fails, they offer it. Split stays greyed
// out on a damaged demo either way (the split parses the whole demo and stops
// at the damage too); only its reason follows the answer.
//
// Asked when shown, not saved with the scan: the check reads the whole file
// (about as fast as reading it), and only damaged demos are asked. A repair's
// own "not damaged" verdict is remembered at once, so the row or notice that
// ran it stops offering Repair on its next draw.

import { demoRepairable, logFrontendEvent } from './ipc_bridge.js';
import { createCachedCheck } from './cached_check.js';
import { isDamagedDemo } from './repair_view.js';
import { STRINGS } from './strings.js';

/** How long an answer is reused. A queued demo's bytes don't change; this
 *  only spares a damaged row's file a re-read on every queue redraw. */
export const REPAIRABLE_KEEP_MS = 30000;

/**
 * Whether to offer Repair (and Repair & Split) on `demo` (a queued demo, or
 * an analysis report's demo_info), given `answer`: true when the repair has a
 * break to cut out, false when it has none, null while unknown. A damaged
 * demo is offered Repair unless the answer is false. Pure.
 */
export function offersRepair(demo, answer) {
  return isDamagedDemo(demo) && answer !== false;
}

/**
 * Why `demo`'s Split is greyed out by its damage, or null when it isn't
 * damaged: "Repair it first" while Repair is offered (Repair & Split does
 * both), else that the damage stops a split and Repair can't cut it out.
 * Pure.
 */
export function splitDamageReason(demo, answer) {
  if (!isDamagedDemo(demo)) return null;
  return offersRepair(demo, answer) ? STRINGS.REPAIR.SPLIT_NEEDS_REPAIR : STRINGS.REPAIR.SPLIT_DAMAGE_CANNOT_REPAIR;
}

/**
 * Whether `demo` is damaged in a way the repair can't fix, so the Analyzer's
 * notice says so in place of its Repair buttons. Pure.
 */
export function isUnrepairableDamage(demo, answer) {
  return isDamagedDemo(demo) && answer === false;
}

/**
 * @param {object} deps
 *   ask(path) resolves native's boolean; now() the clock in ms (tests);
 *   log(line) where a damaged demo found unrepairable is said, once per path.
 * @returns {{ last(path), check(path), remember(path, answer) }} as
 *   cached_check.js's, the answer a boolean (null while unknown).
 */
export function createRepairableCheck({ ask, now, log = () => {} }) {
  const logged = new Set();
  return createCachedCheck({
    keepMs: REPAIRABLE_KEEP_MS,
    now,
    ask: async (path) => {
      const answer = await ask(path);
      if (typeof answer !== 'boolean') throw new Error('no answer');
      if (answer === false && !logged.has(path)) {
        logged.add(path);
        log(STRINGS.REPAIR.cannotRepairLog(path));
      }
      return answer;
    },
  });
}

/** Studio's one checker, over IPC. */
export const repairableStatus = createRepairableCheck({ ask: demoRepairable, log: logFrontendEvent });

/** Remembers what a repair of the demo at `path` found: a "not damaged"
 *  verdict means there is nothing to cut out, whatever the analyzer said. */
export function noteRepairResult(path, result, status = repairableStatus) {
  if (result?.verdict === 'not_damaged') status.remember(path, false);
}
