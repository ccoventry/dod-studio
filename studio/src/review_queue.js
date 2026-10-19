// review_queue.js
// Review highlights (#623), the pure half: which highlights to send the game,
// and what an answer the game sends back changes on its row.
//
// What an answer changes on the row:
//  - Review mark (#44): Yes = Keep, Maybe = Maybe (#661), No = Skip. The game
//    starts at the queue's first highlight; the Review box's Unmarked is how
//    a second review picks up where the first left off.
//  - Kill Range: the from/to kills given in game
//  - Notes: the note typed in game
//  - Ticked for capture: only when the user turned on "tick Yes" (people who
//    only time demos don't want rows ticked), and never for Maybe. Skip always
//    unticks.
// Status is left alone: it records what the pipeline did, not the user's call.

import { listedStreaks } from './queue_filters.js';
import { streakUid, setCuration, CURATION } from './take_index.js';

/** The game speaks yes/maybe/no; the row keeps Keep/Maybe/Skip. */
const ANSWER_FOR_MARK = { [CURATION.KEEP]: 'yes', [CURATION.MAYBE]: 'maybe', [CURATION.SKIP]: 'no' };
const MARK_FOR_ANSWER = { yes: CURATION.KEEP, maybe: CURATION.MAYBE, no: CURATION.SKIP };

/** Which highlights a review covers, by their Review mark (#661). */
export const REVIEW_COVERS = {
  ALL: 'all',
  UNMARKED: 'unmarked',
  KEEP: 'keep',
  MAYBE: 'maybe',
  SKIP: 'skip',
};

const MARK_FOR_COVERS = {
  [REVIEW_COVERS.KEEP]: CURATION.KEEP,
  [REVIEW_COVERS.MAYBE]: CURATION.MAYBE,
  [REVIEW_COVERS.SKIP]: CURATION.SKIP,
};

/** The demos a review takes: the ticked ones, or with none ticked, every
 *  demo the Master Demo Queue shows under its search and filters. */
export function reviewDemoPaths(checkedPaths, shownPaths) {
  return checkedPaths?.length ? checkedPaths : shownPaths || [];
}

/** True when a review covering `covers` includes this highlight. */
export function coveredByReview(streak, covers = REVIEW_COVERS.ALL) {
  if (covers === REVIEW_COVERS.UNMARKED) return !streak.curation;
  const mark = MARK_FOR_COVERS[covers];
  return mark ? streak.curation === mark : true;
}

/** An HLTV demo's highlight: the camera follows no one by itself (#822).
 *  `spectated` comes with a scan since #247; an older project only has the
 *  demo's `is_pov`. */
export function isHltvHighlight(demo, streak) {
  return streak.spectated === true || demo.is_pov === false;
}

/**
 * Whether an HLTV highlight's player is one of `players` (`[{ id, names }]`,
 * the queue's recorders, player_filter.js's groupPlayers), by SteamID when
 * the highlight has one, else by name. `null` takes everyone.
 */
export function hltvPlayerChosen(streak, players) {
  if (!players) return true;
  if (streak.steam_id && players.some((p) => p.id === streak.steam_id)) return true;
  const name = String(streak.target_player || '').trim().toLowerCase();
  return !!name && players.some((p) => (p.names || []).some((n) => n.trim().toLowerCase() === name));
}

/** The HLTV players box's fixed choices; any other value is one recorder's
 *  id (#822). PICKED: the players picked beside the box (#828). */
export const HLTV_PLAYERS = { MISSING: 'missing', RECORDERS: 'recorders', ALL: 'all', PICKED: 'picked' };

/**
 * The queue's recorders who have no POV demo of `hltvDemo`'s half in the
 * queue: the players an HLTV demo stands in for. The half comes from the
 * match grouping (`match_info`, queue_match_groups.js); an HLTV demo it
 * hasn't placed can't say who recorded that half, so it gets every
 * recorder rather than none.
 */
export function playersMissingThisHalf(hltvDemo, demos, recorders, recorderOf) {
  const info = hltvDemo?.match_info;
  if (!info || info.half === null || info.half === undefined) return recorders;
  const recordedHere = new Set(
    (demos || [])
      .filter((d) => d !== hltvDemo && d.match_info?.match === info.match && d.match_info?.half === info.half)
      .map((d) => recorderOf(d)?.id)
      .filter(Boolean),
  );
  return recorders.filter((p) => !recordedHere.has(p.id));
}

/**
 * The players an HLTV review covers for the box's `choice`: the recorders
 * missing their POV for that half (the default; a function of the demo),
 * every recorder in the queue, everyone (null), the players picked beside
 * the box (`queue.picked`, any player in the HLTV demos, #828), or the one
 * recorder picked. `queue`: `{ demos, recorderOf, picked }`.
 */
export function hltvPlayersFor(choice, recorders, queue = {}) {
  if (choice === HLTV_PLAYERS.ALL) return null;
  const all = recorders || [];
  if (!choice || choice === HLTV_PLAYERS.MISSING) {
    return (demo) => playersMissingThisHalf(demo, queue.demos, all, queue.recorderOf || (() => null));
  }
  if (choice === HLTV_PLAYERS.RECORDERS) return all;
  if (choice === HLTV_PLAYERS.PICKED) return queue.picked || [];
  return all.filter((p) => p.id === choice);
}

/** The order a demo's highlights play in: by time, or (for an HLTV demo
 *  with several players) one player at a time, players A to Z. */
export const REVIEW_ORDER = { TIME: 'time', PLAYER: 'player' };

const byName = (a, b) => a.player.localeCompare(b.player, undefined, { sensitivity: 'base' });
const byTime = (a, b) => a.kill_times[0] - b.kill_times[0];

/** Sorts one demo's review rows: by first kill, two at the same moment by
 *  player A to Z; or by player, each player's by first kill. The game
 *  seeks to each highlight, backwards as readily as forwards. */
export function sortReviewRows(rows, order = REVIEW_ORDER.TIME) {
  return rows.sort(order === REVIEW_ORDER.PLAYER
    ? (a, b) => byName(a, b) || byTime(a, b)
    : (a, b) => byTime(a, b) || byName(a, b));
}

/**
 * The highlights to review, in queue order: each ticked demo's highlights
 * with at least `minKills` kills and a Review mark `covers` takes in, in
 * `order` (sortReviewRows); in an HLTV demo, only `hltvPlayers`' (#822: a
 * list, a function of the demo giving one, or null = everyone's), each with
 * the player the game's camera goes on. A highlight with no demo-player
 * times (a project saved before they existed) can't be found in the demo,
 * so it is counted in `skipped` instead.
 */
export function buildReviewQueue(demos, checkedPaths, minKills = 1, covers = REVIEW_COVERS.ALL, hltvPlayers = null, order = REVIEW_ORDER.TIME) {
  const checked = new Set(checkedPaths);
  const highlights = [];
  let skipped = 0;
  // HLTV highlights the HLTV box left out, so an empty review can say why.
  let notChosen = 0;
  for (const demo of demos || []) {
    if (!checked.has(demo.path)) continue;
    const players = typeof hltvPlayers === 'function' ? hltvPlayers(demo) : hltvPlayers;
    const streaks = listedStreaks(demo)
      .filter((s) => (s.kill_count ?? (s.kills || []).length) >= minKills)
      .filter((s) => coveredByReview(s, covers))
      .filter((s) => {
        const keep = !isHltvHighlight(demo, s) || hltvPlayerChosen(s, players);
        if (!keep) notChosen += 1;
        return keep;
      });
    const rows = [];
    for (const streak of streaks) {
      const times = streak.viewdemo_times || [];
      const kills = streak.kills || [];
      if (times.length === 0 || times.length !== kills.length) {
        skipped += 1;
        continue;
      }
      const last = times.length - 1;
      const to = Math.min(streak.end_index ?? last, last);
      const from = Math.min(streak.start_index ?? 0, to);
      const hltv = isHltvHighlight(demo, streak);
      rows.push({
        demo: demo.path,
        key: streakUid(demo.path, streak),
        player: hltv ? streak.target_player || '' : '',
        kill_times: times,
        from: from + 1,
        to: to + 1,
        answered: ANSWER_FOR_MARK[streak.curation] ?? null,
        note: streak.notes || '',
        // The game numbers players from 1; the scan from 0 (as capture does).
        spectate: hltv && Number.isInteger(streak.player_index) ? streak.player_index + 1 : null,
      });
    }
    highlights.push(...sortReviewRows(rows, order));
  }
  return { highlights, skipped, notChosen };
}

/**
 * Puts one answer on its row; `tickYes` also ticks a Yes row for capture.
 * Returns the streak changed, or null when the demo or highlight is no longer
 * in the queue.
 */
export function applyReviewAnswer(demos, answer, { tickYes = false } = {}) {
  const demo = (demos || []).find((d) => d.path === answer.demo);
  if (!demo) return null;
  const streak = (demo.streaks || []).find((s) => streakUid(demo.path, s) === answer.key);
  if (!streak) return null;
  const last = Math.max((streak.kills || []).length - 1, 0);
  const start = Math.min(Math.max((answer.from || 1) - 1, 0), last);
  const end = Math.min(Math.max((answer.to || 1) - 1, start), last);
  streak.start_index = start;
  streak.end_index = end;
  streak.notes = answer.note || '';
  const mark = MARK_FOR_ANSWER[answer.verdict] ?? CURATION.SKIP;
  setCuration(streak, mark);
  if (mark === CURATION.KEEP && tickYes) streak.selected = true;
  return streak;
}

/** Shortest gap worth fast-forwarding: a highlight's own margins (2 s after
 *  a kill, 4 s before the next) leave nothing to speed up at 6 s or less. */
export const MIN_FAST_FORWARD_GAP = 7;

/**
 * The gap setting to send the game (#665): seconds between kills above which
 * the stretch plays fast, or null to play every gap at normal speed. A blank
 * or unreadable box falls back to 8; the value is held to 7..120.
 */
export function fastForwardGap(enabled, rawValue) {
  if (!enabled) return null;
  const seconds = Number.parseFloat(rawValue);
  if (!Number.isFinite(seconds)) return 8;
  return Math.max(MIN_FAST_FORWARD_GAP, Math.min(seconds, 120));
}
