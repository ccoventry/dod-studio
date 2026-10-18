// rescan_carry.js
// Carries a rescanned demo's highlight work onto its new highlights.
//
// A rescan (re-adding a demo an older Studio scanned, the rescan offered when
// a project opens, or rescanning a demo changed on disk) makes new highlight
// objects, and a newer Studio may draw a life differently: a grenade kill
// becoming its own highlight changes the highlight's uid (take_index.js), and
// carrying by uid alone silently dropped the work on it.
//
// So: a highlight whose uid is still there and has as many kills keeps its
// work as it was (free, and the usual case). The rest go through the split's
// kill matcher (split_carry.js): same player, same kills in order, same gaps
// between them, never guessed. Whatever finds no single match is named in the
// activity log.

import { applyCarriedWork, hasWork, matchByKills, workLossLines } from './split_carry.js';
import { streakUid } from './take_index.js';
import { fileNameOf } from './path_display.js';
import { STRINGS } from './strings.js';

/**
 * Matches `previous`'s worked-on highlights to `fresh`'s (the same demo,
 * scanned again). Pure: changes nothing. Same shape as matchSplitHighlights,
 * with `exact: true` on the uid matches.
 */
export function matchRescanHighlights(previous, fresh) {
  const worked = (previous?.streaks || []).filter(hasWork);
  const result = { carried: [], uncarried: [], withWork: worked.length };
  if (worked.length === 0) return result;
  const freshStreaks = fresh?.streaks || [];
  const freshByUid = new Map(freshStreaks.map((s) => [streakUid(fresh.path, s), s]));
  const taken = new Set();
  const rest = [];
  worked.forEach((from) => {
    const to = freshByUid.get(streakUid(fresh.path, from));
    if (to && !taken.has(to) && (to.kills || []).length === (from.kills || []).length) {
      taken.add(to);
      const last = Math.max((to.kills || []).length - 1, 0);
      result.carried.push({
        from, to, piece: fresh, exact: true, narrowed: false,
        start_index: from.start_index ?? 0,
        end_index: from.end_index ?? last,
      });
    } else {
      rest.push(from);
    }
  });
  if (rest.length > 0) matchByKills(previous, [fresh], rest, taken, result);
  return result;
}

/**
 * Puts the work on the new highlights. Returns old uid -> new uid for the
 * take index, leaving out the uids that didn't change.
 */
export function applyRescanWork(previous, result) {
  const renames = applyCarriedWork(previous, result.carried);
  [...renames].forEach(([from, to]) => { if (from === to) renames.delete(from); });
  return renames;
}

/**
 * What a scan's rescans did with the work, over every demo it rescanned:
 * `reports` is `[{ previous, result }]`. One toast for the whole scan (null
 * when no rescanned highlight had work on it), and activity log lines: a
 * summary, then for each demo that lost or narrowed anything, its name and
 * one line per highlight.
 */
export function rescanReport(reports) {
  const total = reports.reduce((n, r) => n + r.result.withWork, 0);
  if (total === 0) return { toast: null, log: [] };
  const kept = reports.reduce((n, r) => n + r.result.carried.length, 0);
  const log = [STRINGS.MAIN.rescanCarryLog(kept, total, reports.filter((r) => r.result.withWork > 0).length)];
  reports.forEach(({ previous, result }) => {
    const lines = workLossLines(previous, result, {
      narrowed: STRINGS.MAIN.rescanNarrowedLog,
      uncarried: STRINGS.MAIN.rescanUncarriedLog,
      reasons: STRINGS.MAIN.RESCAN_UNCARRIED_REASON,
    });
    if (lines.length > 0) log.push(STRINGS.MAIN.rescanDemoLog(fileNameOf(previous.path)), ...lines);
  });
  return {
    toast: { text: STRINGS.MAIN.rescanCarryToast(kept, total), type: kept === total ? 'success' : 'warning' },
    log,
  };
}
