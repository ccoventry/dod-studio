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

/** How long something took or lasts, the one way Studio writes it (#880):
 *  `2.1 s` under a minute (one decimal, a space before the unit), then whole
 *  seconds as `6 min 12 s` and `1 h 2 min 3 s`. Missing or negative reads
 *  `0.0 s`. Clock positions (`m:ss`) and estimates ("about 4 min") are
 *  written their own way. */
export function durationText(seconds) {
  const x = Math.max(0, Number(seconds) || 0);
  const tenths = Math.round(x * 10);
  if (tenths < 600) return `${(tenths / 10).toFixed(1)} s`;
  const whole = Math.round(x);
  const h = Math.floor(whole / 3600);
  const m = Math.floor((whole % 3600) / 60);
  const s = whole % 60;
  return `${h ? `${h} h ` : ''}${m} min ${s} s`;
}

/** {@link durationText} for milliseconds (a job's clock). */
export function durationTextMs(ms) {
  return durationText((Number(ms) || 0) / 1000);
}
