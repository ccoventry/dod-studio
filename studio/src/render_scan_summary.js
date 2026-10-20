// render_scan_summary.js
// What Scan for Takes says once it has merged into the queue (#772): how
// many takes are new, how many rows were refreshed, how many are gone.
// `summary` is the backend's ScanMergeSummary. Pure.

import { STRINGS } from './strings.js';

/**
 * `{ toast, level, status }` for a finished scan. Nothing found and nothing
 * changed says no takes were detected; a queued render dropped because its
 * take folder is gone is a warning, since that render won't happen now.
 */
export function scanMergeMessage(summary) {
  const R = STRINGS.RENDER;
  const { total = 0, added = 0, refreshed = 0, gone = 0, gone_queued: goneQueued = 0 } = summary || {};
  const parts = [];
  if (added > 0) parts.push(R.scanNewTakes(added));
  if (refreshed > 0) parts.push(R.scanRefreshed(refreshed));
  if (gone > 0) parts.push(R.scanGone(gone, goneQueued));

  if (parts.length === 0) {
    if (total === 0) {
      return {
        toast: R.NO_RENDER_TAKES_DETECTED,
        level: 'info',
        status: STRINGS.MAIN.statusGeneric(R.NO_RENDER_TAKES_DETECTED),
      };
    }
    return { toast: R.SCAN_NO_CHANGES, level: 'info', status: R.STATUS_SCAN_NO_CHANGES };
  }
  const text = parts.join(', ');
  let level = 'info';
  if (goneQueued > 0) level = 'warning';
  else if (added > 0 || refreshed > 0) level = 'success';
  return { toast: R.scanMergedToast(text), level, status: R.scanMergedStatus(text) };
}
