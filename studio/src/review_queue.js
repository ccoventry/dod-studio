// review_queue.js
// Review highlights (#623), the pure half: which highlights to send the game,
// and what an answer the game sends back changes on its row.
//
// What an answer changes on the row:
//  - review: 'yes' or 'no', so a second review starts where this one stopped
//  - Kill Range: the from/to kills given in game
//  - Notes: the note typed in game
//  - Status: Yes sets Pending (by hand, as picking it from the dropdown
//    does); No takes a Pending back to None. A Captured or Rendered row
//    keeps its status either way.

import { recordingPlayerStreaks } from './queue_filters.js';
import { streakUid, setStatusByHand } from './take_index.js';
import { STRINGS } from './strings.js';

/** Statuses an answer never moves a row away from. */
const KEPT_STATUSES = ['Captured', 'Rendered'];

/**
 * The highlights to review, in queue order: each ticked demo's recording-
 * player highlights with at least `minKills` kills, by time. A highlight with
 * no demo-player times (a project saved before they existed) can't be found
 * in the demo, so it is counted in `skipped` instead.
 */
export function buildReviewQueue(demos, checkedPaths, minKills = 1) {
  const checked = new Set(checkedPaths);
  const highlights = [];
  let skipped = 0;
  for (const demo of demos || []) {
    if (!checked.has(demo.path)) continue;
    const streaks = recordingPlayerStreaks(demo)
      .filter((s) => (s.kill_count ?? (s.kills || []).length) >= minKills);
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
        answered: streak.review === 'yes' || streak.review === 'no' ? streak.review : null,
        note: streak.notes || '',
      });
    }
    rows.sort((a, b) => a.kill_times[0] - b.kill_times[0]);
    highlights.push(...rows);
  }
  return { highlights, skipped };
}

/**
 * Puts one answer on its row. Returns the streak changed, or null when the
 * demo or highlight is no longer in the queue.
 */
export function applyReviewAnswer(demos, answer) {
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
  streak.review = answer.verdict;
  if (!KEPT_STATUSES.includes(streak.status)) {
    if (answer.verdict === 'yes') {
      setStatusByHand(streak, 'Pending');
    } else if (streak.status === 'Pending') {
      setStatusByHand(streak, STRINGS.HIGHLIGHTS.STATUS_UNSET_DEFAULT);
    }
  }
  return streak;
}
