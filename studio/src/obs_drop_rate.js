// obs_drop_rate.js
// OBS mode's live drop rate (#172): a line under the capture status line
// with the frames OBS is leaving out of the clip it records, and a warning
// that names the fix when it loses more than 5% over the last 5 seconds (an
// encoder that can't keep up at 120 fps lost a third or more, #1060). Each
// clip's totals go to the Last Batch panel.
//
// Fed by `capture_obs_drop_rate` (capture_manager.rs, about once a second
// while OBS records, and once more with a clip's totals when it ends) and
// `capture_status`, for when a batch starts and ends.

import { STRINGS } from './strings.js';
import { batchObsClip } from './batch_results.js';

let started = false;

/** What the line shows for one report: `{ text, title, lagging }`. */
export function dropLine(p) {
  const S = STRINGS.OBS_DROP_RATE;
  if (p?.recording && p.lagging) {
    const encoder = (p.recent_skipped_pct ?? 0) >= (p.recent_missed_pct ?? 0);
    const pct = encoder ? p.recent_skipped_pct : p.recent_missed_pct;
    return { text: S.lagging(pct, encoder, S.fix(p.capture_fps)), title: S.LAGGING_TITLE, lagging: true };
  }
  return {
    text: S.line(p?.clip ?? 0, p?.skipped_pct ?? 0, p?.missed_pct ?? 0, !!p?.recording),
    title: S.LINE_TITLE,
    lagging: false,
  };
}

/** Listens for the reports and keeps `el` (the line) up to date; `toast`
 *  says it once per batch the first time OBS lags. */
export function startObsDropRate({ el, listen, toast = () => {} }) {
  // The capture pane's setup can run again; one set of listeners is enough.
  if (started) return;
  started = true;
  let batchRunning = false;
  let warned = false;

  const hide = () => {
    if (!el) return;
    el.hidden = true;
    el.textContent = '';
    el.classList.remove('lagging');
  };

  const show = (p) => {
    const line = dropLine(p);
    if (line.lagging && !warned) {
      warned = true;
      toast(line.text);
    }
    if (!el) return;
    el.hidden = false;
    el.textContent = line.text;
    el.title = line.title;
    el.classList.toggle('lagging', line.lagging);
  };

  listen('capture_status', (event) => {
    const running = !!event.payload?.running;
    if (running && !batchRunning) {
      warned = false;
      hide();
    }
    if (!running) hide();
    batchRunning = running;
  }).catch((err) => console.error('Failed to register the OBS drop rate status listener:', err));

  listen('capture_obs_drop_rate', (event) => {
    const p = event.payload || {};
    if (!p.recording) batchObsClip(p);
    show(p);
  }).catch((err) => console.error('Failed to register the OBS drop rate listener:', err));
}
