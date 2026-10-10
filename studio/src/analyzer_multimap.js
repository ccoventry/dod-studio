// analyzer_multimap.js
// A demo that recorded more than one map (#217). The Demo Analyzer covers
// only the first (the game's viewdemo stops at the map change too), so it
// says so above every tab and offers Split now: the Demo Auditor's Split
// Maps (#624) on this one demo, after which each map is a demo of its own.
//
// Pure, apart from the banner it fills in: the IPC (splitting, opening the
// result) is the caller's, through `onSplit`.

import { escapeHtml as esc } from './html.js';
import { splitProgressBar } from './split_progress.js';
import { STRINGS } from './strings.js';

/** The maps a report's demo recorded, when there is more than one; else null.
 *  An older cache entry has no `signon_maps` and shows nothing. */
export function multiMapList(report) {
  const maps = report?.state?.signon_maps || [];
  return maps.length > 1 ? maps : null;
}

/**
 * Fills `el` with the notice for `report`, or hides it for a one-map demo.
 * `onSplit(update)` runs on Split now, with `update(SplitProgress)` for the
 * banner's progress bar. While it runs the button is disabled and the bar
 * shows; if it throws, the banner says why.
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
    const bar = splitProgressBar();
    status.replaceChildren(bar.el);
    try {
      await onSplit(bar.update);
      status.replaceChildren();
    } catch (err) {
      status.textContent = STRINGS.ANALYZER.multiMapSplitFailed(err);
    } finally {
      btn.disabled = false;
    }
  });
}
