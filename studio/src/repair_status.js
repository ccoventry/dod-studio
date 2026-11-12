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
// The same read lists every map the demo recorded, by the byte scan the
// Demo Auditor's Damaged Demos tab uses: the analyzer's own list stops at the
// damage, so a demo damaged before its level change looks like one map there.
// With the scan's maps, Repair & Split shows in the queue and the Analyzer
// wherever the Auditor shows it, and the "Already split" check is asked with
// the same list, so a repaired-and-split demo reads as split.
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
 * The maps a damaged demo's repair would split: `scanned` (the byte scan's,
 * null while unknown) when it found more than `own` (the analyzer's list,
 * which stops at the damage), else `own`. Pure.
 */
export function repairMaps(own, scanned) {
  const mine = Array.isArray(own) ? own : [];
  return Array.isArray(scanned) && scanned.length > mine.length ? scanned : mine;
}

/**
 * @param {object} deps
 *   ask(path) resolves native's `{ repairable, maps }`; now() the clock in
 *   ms (tests); log(line) where a damaged demo found unrepairable, or with
 *   maps past its damage, is said, once per path each.
 * @returns {{ last(path), check(path), remember(path, answer), lastMaps(path), checkMaps(path, own) }}
 *   `last`/`check`/`remember` as cached_check.js's, the answer a boolean
 *   (null while unknown); `lastMaps`/`checkMaps` the scan's map list (null
 *   while unknown) from the same call. `checkMaps` logs, once per path, when
 *   the list turns `own`'s one map into several, since that adds Repair &
 *   Split.
 */
export function createRepairableCheck({ ask, now, log = () => {} }) {
  const logged = new Set();
  const logOnce = (key, line) => {
    if (logged.has(key)) return;
    logged.add(key);
    log(line);
  };
  const inner = createCachedCheck({
    keepMs: REPAIRABLE_KEEP_MS,
    now,
    ask: async (path) => {
      const answer = await ask(path);
      if (typeof answer?.repairable !== 'boolean') throw new Error('no answer');
      if (answer.repairable === false) logOnce(`cannot:${path}`, STRINGS.REPAIR.cannotRepairLog(path));
      return { repairable: answer.repairable, maps: Array.isArray(answer.maps) ? answer.maps : [] };
    },
  });
  const repairableOf = (answer) => answer?.repairable ?? null;
  return {
    last: (path) => repairableOf(inner.last(path)),
    check: (path) => inner.check(path).then(repairableOf),
    // A repair's own verdict keeps the maps the scan found.
    remember: (path, repairable) => inner.remember(path, { repairable, maps: inner.last(path)?.maps ?? [] }),
    lastMaps: (path) => inner.last(path)?.maps ?? null,
    checkMaps: (path, own = []) => inner.check(path).then((answer) => {
      const maps = answer?.maps ?? null;
      const mine = Array.isArray(own) ? own : [];
      if (answer?.repairable && mine.length < 2 && repairMaps(mine, maps).length > 1) {
        logOnce(`maps:${path}`, STRINGS.REPAIR.mapsPastDamageLog(path, maps));
      }
      return maps;
    }),
  };
}

/**
 * The maps to ask split_status.js about for the demo at `path` (`own` the
 * analyzer's list): for a damaged demo, [repairMaps] once `status` has
 * scanned it (`own` if the scan fails), so every "Already split" check of
 * it uses the list its repair is split by; `own` for any other demo.
 */
export async function mapsToSplit(path, own, damaged, status = repairableStatus) {
  if (!damaged || !path) return own;
  return repairMaps(own, await status.checkMaps(path, own));
}

/** Studio's one checker, over IPC. */
export const repairableStatus = createRepairableCheck({ ask: demoRepairable, log: logFrontendEvent });

/** Remembers what a repair of the demo at `path` found: a "not damaged"
 *  verdict means there is nothing to cut out, whatever the analyzer said. */
export function noteRepairResult(path, result, status = repairableStatus) {
  if (result?.verdict === 'not_damaged') status.remember(path, false);
}
