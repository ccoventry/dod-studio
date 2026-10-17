// review_queue.js
// Review highlights (#623), the pure half: which highlights to send the game,
// and what an answer the game sends back changes on its row.
//
// What an answer changes on the row:
//  - Review mark (#44): Yes = Keep, Maybe = Maybe (#661), No = Skip. The game
//    starts at the first row with no mark, so a second review passes over
//    Maybe rows unless the review covers only them.
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

/**
 * The highlights to review, in queue order: each ticked demo's recording-
 * player highlights with at least `minKills` kills and a Review mark `covers`
 * takes in, by time. A highlight with no demo-player times (a project saved
 * before they existed) can't be found in the demo, so it is counted in
 * `skipped` instead.
 */
export function buildReviewQueue(demos, checkedPaths, minKills = 1, covers = REVIEW_COVERS.ALL) {
  const checked = new Set(checkedPaths);
  const highlights = [];
  let skipped = 0;
  for (const demo of demos || []) {
    if (!checked.has(demo.path)) continue;
    const streaks = listedStreaks(demo)
      .filter((s) => (s.kill_count ?? (s.kills || []).length) >= minKills)
      .filter((s) => coveredByReview(s, covers));
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
      rows.push({
        demo: demo.path,
        key: streakUid(demo.path, streak),
        player: '',
        kill_times: times,
        from: from + 1,
        to: to + 1,
        answered: ANSWER_FOR_MARK[streak.curation] ?? null,
        note: streak.notes || '',
      });
    }
    rows.sort((a, b) => a.kill_times[0] - b.kill_times[0]);
    highlights.push(...rows);
  }
  return { highlights, skipped };
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
