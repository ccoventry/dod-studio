// analyzer_loading.js
// What the Demo Analyzer shows while a demo opens (#958): its line, and a
// bar under it from the % the backend's `analyzer_progress` gives, the
// block sliding until the first one. A demo the analyzer cache has is read,
// not parsed, so it says so instead of a % that never comes.
import { STRINGS } from './strings.js';

/// The line and bar for a demo opening: `p` is the last `analyzer_progress`
/// event for it (`null` before the first), as `{ text, short, pct }`; `pct`
/// is `null` while nothing can be measured.
export function analyzerLoadingView(p) {
  if (p?.cached) {
    return { text: STRINGS.ANALYZER.READING_CACHE, short: STRINGS.ANALYZER.READING_CACHE_SHORT, pct: null };
  }
  if (!p?.total) {
    return { text: STRINGS.ANALYZER.ANALYZING_DEMO_ELLIPSIS, short: STRINGS.ANALYZER.ANALYZING_ELLIPSIS, pct: null };
  }
  const pct = Math.min(100, Math.round((p.processed / p.total) * 100));
  return { text: STRINGS.ANALYZER.analyzingDemoPct(pct), short: STRINGS.ANALYZER.analyzingPct(pct), pct };
}

/// The panel's HTML for `analyzerLoadingView`'s result. Its text is the
/// app's own, never a file name, so it needs no escaping.
export function analyzerLoadingHtml(view) {
  const sliding = view.pct == null;
  return `<p class="analyzer-empty">${view.text}</p>`
    + `<div class="progress-bar-container analyzer-loading-bar${sliding ? ' indeterminate' : ''}">`
    + `<div class="progress-bar-fill" style="width: ${sliding ? 0 : view.pct}%;"></div></div>`;
}
