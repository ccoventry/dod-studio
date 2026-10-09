// render_history.js
// A take's render history in the job table (#438): the latest outcome with a
// count, and one line per attempt. Pure, so it can be tested on its own.

import { STRINGS } from './strings.js';

/**
 * The History cell's summary: `{ label, bytes }`. `label` is New, Rendered
 * ×N (N = finished attempts), or the latest attempt's outcome when that
 * didn't finish. `bytes` totals the finished outputs still on disk.
 */
export function historySummary(history) {
  const list = history || [];
  const finished = list.filter((h) => h.outcome === 'finished');
  const bytes = finished
    .filter((h) => h.output_exists)
    .reduce((total, h) => total + (h.output_size_bytes || 0), 0);
  const latest = list[list.length - 1];
  let label;
  if (!latest) label = STRINGS.RENDER.HISTORY_NEW;
  else if (latest.outcome === 'finished') label = STRINGS.RENDER.historyRendered(finished.length);
  else label = STRINGS.RENDER.HISTORY_OUTCOME[latest.outcome] || latest.outcome;
  return { label, bytes };
}

function when(unixMs) {
  const d = new Date(unixMs);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function fileName(path) {
  return String(path || '').split(/[\\/]/).pop();
}

/**
 * One line per attempt, newest first: when, settings, and what came of it.
 * `formatSize(bytes)` renders a size the way the table does.
 */
export function historyLines(history, formatSize) {
  return [...(history || [])].reverse().map((h) => {
    const settings = h.codec === 'source_copy'
      ? STRINGS.RENDER.HISTORY_SKIPPED_SETTINGS
      : `${h.codec}${h.custom_codec_args ? ` (${h.custom_codec_args})` : ''} @ ${h.fps}fps`;
    let result;
    if (h.outcome === 'finished') {
      result = h.output_exists
        ? `${fileName(h.output_path)}, ${formatSize(h.output_size_bytes)}`
        : STRINGS.RENDER.historyFileNotFound(fileName(h.output_path));
    } else {
      const outcome = STRINGS.RENDER.HISTORY_OUTCOME[h.outcome] || h.outcome;
      result = h.error ? `${outcome}: ${h.error}` : outcome;
    }
    return `${when(h.started_unix_ms)} · ${settings} · ${result}`;
  });
}
