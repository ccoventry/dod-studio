// time_format.js
// Durations and clock times as the panes show them (#485). One place, so the
// Time column, the Analyzer and the progress lines can't drift apart.

const pad2 = (n) => String(n).padStart(2, '0');

/** A whole, non-negative number of seconds as m:ss; minutes grow past 59. */
function mss(whole) {
  return `${Math.floor(whole / 60)}:${pad2(whole % 60)}`;
}

/** Seconds as m:ss, rounded down. Missing or negative reads 0:00. */
export function clockFloor(seconds) {
  return mss(Math.max(0, Math.floor(seconds || 0)));
}

/** Seconds as m:ss, rounded to the nearest second. Missing or negative
 *  reads 0:00. */
export function clockRound(seconds) {
  return mss(Math.max(0, Math.round(seconds || 0)));
}

/** Seconds as m:ss, or h:mm:ss from an hour, rounded to the nearest
 *  second. Missing or negative reads 0:00. */
export function clockLong(seconds) {
  const s = Math.max(0, Math.round(seconds || 0));
  const h = Math.floor(s / 3600);
  return h ? `${h}:${pad2(Math.floor((s % 3600) / 60))}:${pad2(s % 60)}` : mss(s);
}

/** Whole seconds of a running job as `4m 05s`, or `45s` under a minute. */
export function elapsedWords(secs) {
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return m ? `${m}m ${pad2(s)}s` : `${s}s`;
}
