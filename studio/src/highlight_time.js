// highlight_time.js
// The Time and Dur. columns on Highlight Details.
//
// start_tick/end_tick are frame-record indices, not seconds x tickrate.
// Records are not evenly spaced (more around map changes and loading), so
// dividing by the tickrate drifts further from the demo player's clock the
// later the highlight is (#464). The streak already carries real times:
//  - viewdemo_times[i]: kill i on the demo player's (VCR bar) clock
//  - kills[i][1]: kill i's absolute time in seconds
// Both follow the selected Kill Range, as start_tick/end_tick did.

function selectedRange(streak) {
  const kills = streak.kills || [];
  const last = Math.max(kills.length - 1, 0);
  const end = Math.min(streak.end_index ?? last, last);
  const start = Math.min(streak.start_index ?? 0, end);
  return { start, end };
}

/**
 * Seconds on the demo player's clock at the first selected kill.
 * Falls back to start_tick / tickrate only for projects saved before
 * viewdemo_times existed.
 */
export function highlightStartSeconds(streak, tickrate) {
  const times = streak.viewdemo_times || [];
  if (times.length > 0) {
    const { start } = selectedRange(streak);
    return times[Math.min(start, times.length - 1)];
  }
  return streak.start_tick / (tickrate || 100);
}

/** Seconds from the first selected kill to the last. */
export function highlightDurationSeconds(streak, tickrate) {
  const kills = streak.kills || [];
  if (kills.length > 0) {
    const { start, end } = selectedRange(streak);
    return Math.max(kills[end][1] - kills[start][1], 0);
  }
  return (streak.end_tick - streak.start_tick) / (tickrate || 100);
}
