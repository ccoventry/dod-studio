// render_history.js
// A take's render history in the job table (#438): the latest outcome with a
// count, and one line per attempt. Pure, so it can be tested on its own.

import { STRINGS } from './strings.js';
import { fileNameOf } from './path_display.js';

/**
 * What an attempt came to, as HISTORY_OUTCOME keys it. An attempt is written
 * as `started` before FFmpeg runs, so a crash leaves that behind: it is the
 * render in progress when it is the job's `currentAttempt`, else it was cut
 * off. Files from before #818's fix say `interrupted` for the latter.
 */
function outcomeOf(h, currentAttempt) {
  if (h.outcome !== 'started') return h.outcome;
  return currentAttempt != null && h.started_unix_ms === currentAttempt ? 'rendering' : 'interrupted';
}

/**
 * The History cell's summary: `{ label, bytes }`. `label` is New, Rendered
 * ×N (N = finished attempts), or the latest attempt's outcome when that
 * didn't finish. `bytes` totals the finished outputs still on disk.
 */
export function historySummary(history, currentAttempt = null) {
  const list = history || [];
  const finished = list.filter((h) => h.outcome === 'finished');
  const bytes = finished
    .filter((h) => h.output_exists)
    .reduce((total, h) => total + (h.output_size_bytes || 0), 0);
  const latest = list[list.length - 1];
  let label;
  if (!latest) label = STRINGS.RENDER.HISTORY_NEW;
  else if (latest.outcome === 'finished') label = STRINGS.RENDER.historyRendered(finished.length);
  else {
    const outcome = outcomeOf(latest, currentAttempt);
    label = STRINGS.RENDER.HISTORY_OUTCOME[outcome] || outcome;
  }
  return { label, bytes };
}

function when(unixMs) {
  const d = new Date(unixMs);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}


/**
 * One line per attempt, newest first: when, settings, and what came of it.
 * `formatSize(bytes)` renders a size the way the table does.
 */
export function historyLines(history, formatSize, currentAttempt = null) {
  return [...(history || [])].reverse().map((h) => {
    const settings = h.codec === 'source_copy'
      ? STRINGS.RENDER.HISTORY_SKIPPED_SETTINGS
      : `${h.codec}${h.custom_codec_args ? ` (${h.custom_codec_args})` : ''} @ ${h.fps}fps`;
    let result;
    if (h.outcome === 'finished') {
      result = h.output_exists
        ? `${fileNameOf(h.output_path)}, ${formatSize(h.output_size_bytes)}`
        : STRINGS.RENDER.historyFileNotFound(fileNameOf(h.output_path));
    } else {
      const key = outcomeOf(h, currentAttempt);
      const outcome = STRINGS.RENDER.HISTORY_OUTCOME[key] || key;
      result = h.error ? `${outcome}: ${h.error}` : outcome;
    }
    return `${when(h.started_unix_ms)} · ${settings} · ${result}`;
  });
}
