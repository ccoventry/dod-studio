// streak_visuals.js
// A highlight's derived fields (ticks, kill count, duration, timeline), pure.

import { STRINGS } from './strings.js';
import { clockRound } from './time_format.js';

/**
 * Recomputes start_tick/end_tick/kill_count/duration_string/timeline_string
 * from streak.kills[start_index..=end_index]. Mirrors
 * `CaptureStreak::update_visuals` in native/src/patch/types.rs — must be
 * called after any mutation of start_index/end_index so the display and the
 * eventual capture payload (which is built from these same fields) agree.
 */
export function updateStreakVisuals(streak) {
  if (!streak.kills || streak.kills.length === 0) return;

  // A session saved before start_index/end_index existed (or a streak never
  // touched by the range editor) may have either as undefined.
  if (streak.end_index === undefined) streak.end_index = streak.kills.length - 1;
  if (streak.start_index === undefined) streak.start_index = 0;

  const end = Math.min(streak.end_index, streak.kills.length - 1);
  const start = Math.min(streak.start_index, end);
  streak.start_index = start;
  streak.end_index = end;

  const slice = streak.kills.slice(start, end + 1);
  streak.start_tick = slice[0][0];
  streak.end_tick = slice[slice.length - 1][0];
  streak.kill_count = slice.length;

  const totalSecs = Math.round(Math.max(slice[slice.length - 1][1] - slice[0][1], 0));
  streak.duration_string = clockRound(totalSecs);

  const parts = slice.map(([, absTime, weapon], i) => {
    // Falls back to a labelled placeholder rather than an empty string so an
    // unresolved weapon name (e.g. a missing localization key) can never
    // leave a blank array element — `Array.prototype.join` would otherwise
    // render that as an orphaned leading/embedded separator with no name.
    const weaponClean = String(weapon || '').replace(/^Weapon::/, '').trim() || STRINGS.ANALYZER.WEAPON_UNKNOWN;
    if (i === 0) return weaponClean;
    const gapSec = Math.round(Math.max(absTime - slice[i - 1][1], 0));
    return `(+${clockRound(gapSec)}) ${weaponClean}`;
  });
  streak.timeline_string = parts.join(', ');
}
