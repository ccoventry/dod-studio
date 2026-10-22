// split_progress.js
// What a demo split is doing, for its progress bar (#217): native's
// `SplitProgress` (sent as `split_progress` events with the demo's path) as
// a percentage and a line of text. Shared by the Demo Analyzer's Split now,
// the Demo Auditor's Split Maps and the Master Queue's Split.

import { STRINGS } from './strings.js';

/** `{ pct, text }` for one `SplitProgress`: pct 0-100, whole numbers. */
export function splitProgressView(p) {
  const pct = Math.max(0, Math.min(100, Math.round((p?.fraction || 0) * 100)));
  const S = STRINGS.SPLIT;
  if (p?.stage === 'writing') return { pct, text: S.progressWriting(p.map, p.part, p.parts, pct) };
  if (p?.stage === 'checking') return { pct, text: S.progressChecking(p.map, p.part, p.parts, pct) };
  return { pct, text: S.progressReading(pct) };
}

/**
 * A progress bar for a split: `{ el, update(p) }`. `el` is a bar with a line
 * of text under it; `update` takes a `SplitProgress`. Starts at 0% saying
 * it's reading, so it shows something the moment the button is pressed.
 */
export function splitProgressBar() {
  const el = document.createElement('div');
  el.className = 'split-progress';
  el.innerHTML = '<div class="progress-bar-container"><div class="progress-bar-fill" style="width: 0%"></div></div>'
    + '<span class="split-progress-text"></span>';
  const fill = el.querySelector('.progress-bar-fill');
  const text = el.querySelector('.split-progress-text');
  const update = (p) => {
    const view = splitProgressView(p);
    fill.style.width = `${view.pct}%`;
    text.textContent = view.text;
  };
  update(null);
  return { el, update };
}
