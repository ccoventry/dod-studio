// analyzer_multimap.js
// A demo that recorded more than one map (#217). The Demo Analyzer covers
// only the first (the game's viewdemo stops at the map change too), so it
// says so above every tab and offers Split now: the Demo Auditor's Split
// Maps (#624) on this one demo, after which each map is a demo of its own.
// Once its maps were split next to it (split_status.js), the notice is a
// quiet line saying so instead, with no button.
//
// Splitting and opening the result are the caller's, through `onSplit`;
// whether the maps were already split is split_status.js's.

import { escapeHtml as esc } from './html.js';
import { splitProgressBar } from './split_progress.js';
import { isAlreadySplit, splitStatus } from './split_status.js';
import { isDamagedDemo } from './repair_view.js';
import { mapsToSplit, repairableStatus, splitDamageReason } from './repair_status.js';
import { STRINGS } from './strings.js';

/** The maps a report's demo recorded, when there is more than one; else null.
 *  An older cache entry has no `signon_maps` and shows nothing. */
export function multiMapList(report) {
  const maps = report?.state?.signon_maps || [];
  return maps.length > 1 ? maps : null;
}

/**
 * Fills `el` with the notice for `report`, or hides it for a one-map demo.
 * Shows the last known split answer from `status` (split_status.js's
 * checker) at once, then asks again and redraws when the answer differs,
 * unless another demo was opened or a split is running meanwhile. A damaged
 * demo's Split now is greyed out, its reason from `repairable`
 * (repair_status.js): "Repair it first", or that Repair can't cut it out.
 * `onSplit(update)` runs on Split now, with `update(SplitProgress)` for the
 * banner's progress bar. While it runs the button is disabled and the bar
 * shows; if it throws, the banner says why.
 */
export function renderMultiMapBanner(el, report, onSplit, status = splitStatus, repairable = repairableStatus) {
  if (!el) return;
  const maps = multiMapList(report);
  const path = report?.file_path;
  el.dataset.path = path || '';
  if (!maps) {
    el.hidden = true;
    el.innerHTML = '';
    el.classList.remove('analyzer-multimap-quiet');
    el.removeAttribute('title');
    return;
  }
  el.hidden = false;
  const demo = report?.demo_info;
  let split = path ? status.last(path) : null;
  let canRepair = path ? repairable.last(path) : null;
  let shown = null;
  const show = () => {
    const notice = multiMapNotice(maps, split);
    const blocked = splitDamageReason(demo, canRepair);
    const key = `${notice.quiet}/${blocked}`;
    if (key === shown) return;
    shown = key;
    if (notice.quiet) renderAlreadySplit(el, notice);
    else renderSplitHint(el, notice, onSplit, blocked);
  };
  show();
  if (!path) return;
  const redraw = () => {
    if (el.dataset.path !== path || el.querySelector('.analyzer-multimap-split[data-splitting]')) return;
    show();
  };
  // A damaged demo's "Already split" check waits for the repair check: it
  // asks with the maps the repair is split by (repair_status.js).
  const damaged = isDamagedDemo(demo);
  mapsToSplit(path, maps, damaged, repairable)
    .then((asked) => {
      if (damaged) { canRepair = repairable.last(path); redraw(); }
      return status.check(path, asked);
    })
    .then((answer) => { split = answer; redraw(); });
}

/**
 * What the notice says for `maps` and `answer` (split_status.js's, or null
 * when unknown): `{ quiet: true, text, title }` once the maps were already
 * split next to the demo (the title names the files), else
 * `{ quiet: false, text }` for the notice with Split now.
 */
export function multiMapNotice(maps, answer) {
  if (isAlreadySplit(answer)) {
    return {
      quiet: true,
      text: STRINGS.ANALYZER.multiMapAlreadySplit(maps),
      title: STRINGS.ANALYZER.multiMapAlreadySplitTitle(answer.files),
    };
  }
  return { quiet: false, text: STRINGS.ANALYZER.multiMapNotice(maps) };
}

/** The quiet line for a demo whose maps were already split next to it. */
function renderAlreadySplit(el, notice) {
  el.classList.add('analyzer-multimap-quiet');
  el.title = notice.title;
  el.innerHTML = `<span class="analyzer-multimap-text">${esc(notice.text)}</span>`;
}

/** The notice with Split now, for a demo not split yet. A damaged demo
 *  (#15) can't be split: Split now is greyed out with `blockedReason`
 *  (Repair it first, when the damage notice's Repair & Split does both). */
function renderSplitHint(el, notice, onSplit, blockedReason) {
  el.classList.remove('analyzer-multimap-quiet');
  el.removeAttribute('title');
  el.innerHTML = `
    <span class="analyzer-multimap-text">${esc(notice.text)}</span>
    <button type="button" class="analyzer-multimap-split primary-btn"
      title="${esc(STRINGS.ANALYZER.MULTI_MAP_SPLIT_TITLE)}">${esc(STRINGS.ANALYZER.MULTI_MAP_SPLIT_BUTTON)}</button>
    <span class="analyzer-multimap-status"></span>`;
  const btn = el.querySelector('.analyzer-multimap-split');
  const status = el.querySelector('.analyzer-multimap-status');
  if (blockedReason) {
    btn.disabled = true;
    btn.title = blockedReason;
    return;
  }
  btn.addEventListener('click', async () => {
    btn.disabled = true;
    btn.dataset.splitting = '1';
    const bar = splitProgressBar();
    status.replaceChildren(bar.el);
    try {
      await onSplit(bar.update);
      status.replaceChildren();
    } catch (err) {
      status.textContent = STRINGS.ANALYZER.multiMapSplitFailed(err);
    } finally {
      btn.disabled = false;
      delete btn.dataset.splitting;
    }
  });
}
