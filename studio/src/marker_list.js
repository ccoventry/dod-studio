// marker_list.js
// "Export Marker List" (#110): one CSV row per captured highlight — where it
// is in its demo, what it is, and which take holds it — for lining clips up
// in an editor. Pure, so it can be tested on its own.

import { streakUid } from './take_index.js';
import { HIGHLIGHT_STATUS } from './status_colors.js';

/** Seconds as `h:mm:ss.ss` (hours only when there are some). */
export function clockTime(seconds) {
  const s = Math.max(0, Number(seconds) || 0);
  const hours = Math.floor(s / 3600);
  const minutes = Math.floor((s % 3600) / 60);
  const rest = (s % 60).toFixed(2).padStart(5, '0');
  const mm = String(minutes).padStart(hours ? 2 : 1, '0');
  return hours ? `${hours}:${mm}:${rest}` : `${mm}:${rest}`;
}

/** One CSV field, quoted when it has to be (RFC 4180). */
export function csvField(value) {
  const text = String(value ?? '');
  return /[",\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

export const MARKER_COLUMNS = [
  'Demo', 'Player', 'Kills', 'Kill Range', 'Start', 'End', 'Duration (s)',
  'Start Tick', 'End Tick', 'Status', 'Take', 'Label', 'Notes',
];

/** Demo-player seconds at kill `index`, from the scan's viewdemo times,
 *  falling back to the kill's own recorded time. */
function killSeconds(streak, index) {
  return streak.viewdemo_times?.[index] ?? streak.kills?.[index]?.[1] ?? 0;
}

/**
 * The rows: every highlight with a Captured or Rendered status, in demo and
 * then time order. `takeIndex` (take key -> highlight uids) names the take
 * each one was captured into, when it's known.
 */
export function markerRows(demos, takeIndex) {
  const takeByUid = new Map();
  Object.entries(takeIndex || {}).forEach(([key, uids]) => {
    (uids || []).forEach((uid) => { if (!takeByUid.has(uid)) takeByUid.set(uid, key); });
  });
  const rows = [];
  (demos || []).forEach((demo) => {
    (demo.streaks || [])
      .filter((s) => s.status === HIGHLIGHT_STATUS.CAPTURED || s.status === HIGHLIGHT_STATUS.RENDERED)
      .sort((a, b) => a.start_tick - b.start_tick)
      .forEach((streak) => {
        const start = streak.start_index ?? 0;
        const end = streak.end_index ?? Math.max((streak.kills || []).length - 1, 0);
        const startSecs = killSeconds(streak, start);
        const endSecs = killSeconds(streak, end);
        rows.push([
          demo.name || demo.path,
          streak.target_player || '',
          end - start + 1,
          `${start + 1}-${end + 1}`,
          clockTime(startSecs),
          clockTime(endSecs),
          Math.max(0, endSecs - startSecs).toFixed(2),
          streak.kills?.[start]?.[0] ?? streak.start_tick,
          streak.kills?.[end]?.[0] ?? streak.end_tick,
          streak.status,
          takeByUid.get(streakUid(demo.path, streak)) || '',
          streak.timeline_string || '',
          streak.notes || '',
        ]);
      });
  });
  return rows;
}

/** The whole file: a header row, then `markerRows`, CRLF line ends. */
export function markerCsv(demos, takeIndex) {
  return [MARKER_COLUMNS, ...markerRows(demos, takeIndex)]
    .map((row) => row.map(csvField).join(','))
    .join('\r\n') + '\r\n';
}
