// queue_match_groups.js
// The Master Demo Queue's match grouping (#685): which match and half each
// queued demo is, from the backend's grouping of the whole queue
// (native::match_groups over analysis::group_matches, which reads the
// demos' contents, never their names). The answer is kept on each demo as
// a runtime-only `match_info` (not saved in the project) for the {match}
// and {half} placeholders, and Group by: Match (queue_group_by.js, #823)
// shows the queue in match order with a header line per match. The pure
// parts come first, so they
// can be tested on their own.

import { listen } from '@tauri-apps/api/event';
import { queueMatchGroups, queueMatchGroupsCancel } from './ipc_bridge.js';
import { groupingStepView, placed, queueProgress } from './queue_progress.js';
import { STRINGS } from './strings.js';

/**
 * Match numbers that stay put while a session grows. The backend numbers
 * matches by date over the demos it is given, so adding an older match
 * would move every number. Instead a match keeps the number it had, as
 * long as it still holds a demo that had it and no other match took that
 * number first; new matches get the next unused numbers in the backend's
 * date order.
 *
 * `previous`: path -> number from the last run. `matches`: the backend's
 * `MatchSummary` list. Returns backend number -> session number.
 */
export function stableNumbers(previous, matches) {
  const used = new Set();
  const out = new Map();
  // Each match's claim on an old number: the number most of its demos had.
  // The strongest claims are settled first, so a match that split keeps
  // its number with its larger part.
  const claims = [];
  for (const m of matches) {
    const counts = new Map();
    for (const h of m.halves) {
      for (const path of h.demos) {
        const n = previous.get(path);
        if (n) counts.set(n, (counts.get(n) || 0) + 1);
      }
    }
    const best = [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0] - b[0])[0];
    if (best) claims.push({ number: m.number, old: best[0], count: best[1] });
  }
  claims.sort((a, b) => b.count - a.count || a.number - b.number);
  for (const c of claims) {
    if (used.has(c.old)) continue;
    out.set(c.number, c.old);
    used.add(c.old);
  }
  let next = 1;
  for (const m of matches) {
    if (out.has(m.number)) continue;
    while (used.has(next)) next += 1;
    out.set(m.number, next);
    used.add(next);
  }
  return out;
}

/**
 * Puts each demo's match info on it, as a runtime-only property so the
 * project file never carries it: `{ match, half, reason, detail }`, or
 * `null` for a demo the grouping didn't place. Returns the matches with
 * their session numbers, in number order.
 */
export function applyGrouping(demos, grouping, numbers) {
  const byPath = new Map((grouping?.demos || []).map((d) => [d.path, d]));
  const runtime = { enumerable: false, configurable: true, writable: true };
  for (const demo of demos) {
    const d = byPath.get(demo.path);
    const value = d
      ? { match: numbers.get(d.match_number) ?? d.match_number, half: d.half ?? null, reason: d.half_reason, detail: d.half_detail }
      : null;
    Object.defineProperty(demo, 'match_info', { ...runtime, value });
  }
  return (grouping?.matches || [])
    .map((m) => ({ ...m, number: numbers.get(m.number) ?? m.number }))
    .sort((a, b) => a.number - b.number);
}

/** Halves in reading order: the first half, the second, then unknown. */
function halfRank(half) {
  return half === 1 ? 0 : half === 2 ? 1 : 2;
}

/**
 * The queue in match order: `{ header, demos }` per match that has a
 * visible demo, its demos first half first (queue order within a half),
 * then one group of the demos that have no match info, with a null match.
 * `matches` are the session-numbered summaries; `demos` the visible ones.
 */
export function groupedQueue(demos, matches) {
  const byNumber = new Map();
  const loose = [];
  demos.forEach((demo, i) => {
    const info = demo.match_info;
    if (!info) {
      loose.push(demo);
      return;
    }
    if (!byNumber.has(info.match)) byNumber.set(info.match, []);
    byNumber.get(info.match).push({ demo, i });
  });
  const summaries = new Map((matches || []).map((m) => [m.number, m]));
  const groups = [...byNumber.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([number, items]) => ({
      match: summaries.get(number) || { number, map: '', tags: [null, null], halves: [] },
      demos: items
        .sort((a, b) => halfRank(a.demo.match_info.half) - halfRank(b.demo.match_info.half) || a.i - b.i)
        .map((item) => item.demo),
    }));
  if (loose.length) groups.push({ match: null, demos: loose });
  return groups;
}

/**
 * A match's header line, counted over the demos shown under it:
 * "Match 3 · dod_anzio · dicE vs gskiLL · h1: 4 demos, h2: 3 demos".
 */
export function matchHeaderText(match, demos) {
  const W = STRINGS.WORKSPACE;
  if (!match) return W.UNGROUPED_HEADER;
  const [a, b] = match.tags || [];
  const teams = a || b ? W.matchTeams(a, b) : '';
  const counts = new Map();
  for (const demo of demos) {
    const half = demo.match_info?.half ?? null;
    counts.set(half, (counts.get(half) || 0) + 1);
  }
  const halves = [...counts.entries()]
    .sort((x, y) => halfRank(x[0]) - halfRank(y[0]))
    .map(([half, count]) => W.matchHalfCount(half, count))
    .join(', ');
  return W.matchHeader(match.number, match.map, teams, halves);
}

// What master_pane.js reads at render time.
let currentMatches = [];

/** The matches of the last grouping, session-numbered. */
export function matchesInQueue() {
  return currentMatches;
}

/**
 * Wires the grouping. `getDemos()` is the queue; `renderQueue()` redraws
 * it. `regroup({ parseMissing })` asks the backend again, from the analyzer
 * cache alone unless `parseMissing` (then demos the cache lacks are parsed,
 * about a second each; the default while `parseMissingByDefault()` says so,
 * i.e. while the queue is grouped by match, #823). A regroup asked for while
 * one runs follows it, on its own.
 *
 * Its progress goes on the queue's line and bar (queue_progress.js, #951):
 * `within` is a scan's place, when the grouping is the scan's last step;
 * otherwise it is a job of its own, unless something else has the line,
 * and the line it found comes back when it's done.
 */
export function createQueueMatchGroups({ getDemos, renderQueue, parseMissingByDefault = () => false }) {
  let previous = new Map();
  let running = false;
  let queued = null;
  const progress = queueProgress();
  let place = null;

  listen('match_group_progress', (event) => {
    if (running && place) progress.show(place.job, placed(place, groupingStepView(event.payload)));
  }).catch((err) => console.error('Failed to register match_group_progress listener:', err));

  async function regroup({ parseMissing = parseMissingByDefault(), within = null } = {}) {
    if (running) {
      queued = { parseMissing: parseMissing || queued?.parseMissing || false };
      return;
    }
    const demos = getDemos().filter((d) => !d.missing);
    if (!demos.length) {
      currentMatches = [];
      previous = new Map();
      return;
    }
    running = true;
    const job = within ? null : progress.begin({ ifIdle: true, text: STRINGS.WORKSPACE.groupingStatus(0, 0) });
    place = within ?? (job && { job, range: [0, 100] });
    // Until the backend counts the demos: the sliding block on a bar of its
    // own, or a scan's bar where the scan left it.
    if (place) progress.show(place.job, placed(place, { ...groupingStepView(null), indeterminate: !within }));
    try {
      const grouping = await queueMatchGroups(demos.map((d) => d.path), parseMissing);
      const numbers = stableNumbers(previous, grouping.matches || []);
      currentMatches = applyGrouping(getDemos(), grouping, numbers);
      previous = new Map(getDemos().filter((d) => d.match_info).map((d) => [d.path, d.match_info.match]));
      renderQueue();
    } catch {
      // ipc_bridge already said why.
    } finally {
      running = false;
      place = null;
      progress.end(job);
      if (queued) {
        const next = queued;
        queued = null;
        regroup(next);
      }
    }
  }

  return {
    regroup,
    cancel: () => queueMatchGroupsCancel().catch(() => {}),
  };
}
