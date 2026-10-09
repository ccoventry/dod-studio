// analyzer_multimap.js
// A demo that recorded more than one map (#217). The Demo Analyzer covers
// only the first (the game's viewdemo stops at the map change too), so it
// says so above every tab and offers Split now: the Demo Auditor's Split
// Maps (#624) on this one demo, after which each map is a demo of its own.
//
// Pure, apart from the banner it fills in: the IPC (finding the maps,
// splitting, opening the result) is the caller's, through `onSplit`.

import { escapeHtml as esc } from './html.js';
import { STRINGS } from './strings.js';

/** The maps a report's demo recorded, when there is more than one; else null.
 *  An older cache entry has no `signon_maps` and shows nothing. */
export function multiMapList(report) {
  const maps = report?.state?.signon_maps || [];
  return maps.length > 1 ? maps : null;
}

/**
 * Which maps (`MapSegment.index`) a split keeps: every map at least
 * `minSeconds` long. A shorter one is almost always the next map loading as
 * the recording stopped, so Split Maps leaves it unticked too. When that
 * leaves nothing, every map is kept.
 */
export function mapsToKeep(segments, minSeconds) {
  const long = segments
    .filter((s) => s.end_seconds - s.start_seconds >= minSeconds)
    .map((s) => s.index);
  return long.length ? long : segments.map((s) => s.index);
}

/**
 * Fills `el` with the notice for `report`, or hides it for a one-map demo.
 * `onSplit()` runs on Split now; while it runs the button is disabled, and
 * if it throws, the banner says why.
 */
export function renderMultiMapBanner(el, report, onSplit) {
  if (!el) return;
  const maps = multiMapList(report);
  if (!maps) {
    el.hidden = true;
    el.innerHTML = '';
    return;
  }
  el.hidden = false;
  el.innerHTML = `
    <span class="analyzer-multimap-text">${esc(STRINGS.ANALYZER.multiMapNotice(maps))}</span>
    <button type="button" class="analyzer-multimap-split primary-btn"
      title="${esc(STRINGS.ANALYZER.MULTI_MAP_SPLIT_TITLE)}">${esc(STRINGS.ANALYZER.MULTI_MAP_SPLIT_BUTTON)}</button>
    <span class="analyzer-multimap-status"></span>`;
  const btn = el.querySelector('.analyzer-multimap-split');
  const status = el.querySelector('.analyzer-multimap-status');
  btn.addEventListener('click', async () => {
    btn.disabled = true;
    status.textContent = STRINGS.ANALYZER.MULTI_MAP_SPLITTING;
    try {
      await onSplit();
      status.textContent = '';
    } catch (err) {
      status.textContent = STRINGS.ANALYZER.multiMapSplitFailed(err);
    } finally {
      btn.disabled = false;
    }
  });
}
