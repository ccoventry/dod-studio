// analyzer_damage.js
// A demo damaged partway (#811): the analyzer reads it up to the first frame
// that does not parse, and everything the Demo Analyzer shows stops there.
// It says so above every tab, with where it stopped on the viewdemo clock,
// and offers Repair now (#15): the repair is the caller's, through `onRepair`.
// Damage the repair has no break to cut out (repair_status.js) keeps the
// notice, which says Repair can't fix it instead of offering it. Once the
// repair's maps were split next to the demo (split_status.js, asked with the
// maps repair_status.js found past the damage, as the queue row asks), the
// notice offers neither button: the work is done, and another Repair would
// only write a second copy.
//
// Pure, apart from the banner it fills in.

import { STRINGS } from './strings.js';
import { clockFloor } from './time_format.js';
import { escapeHtml as esc } from './html.js';
import { repairProgressBar } from './repair_view.js';
import { isUnrepairableDamage, repairMaps, repairableStatus } from './repair_status.js';
import { isAlreadySplit, splitStatus } from './split_status.js';

/** A byte count as "14.2 MB" or "312 KB". */
export function sizeText(bytes) {
  const n = Number(bytes) || 0;
  if (n >= 1 << 20) return `${(n / (1 << 20)).toFixed(1)} MB`;
  return `${Math.ceil(n / 1024)} KB`;
}

/** The notice for a damaged demo's report, or null for one that read to the
 *  end (and for an older cache entry, which has no `damage`). */
export function damageNotice(report) {
  const damage = report?.demo_info?.damage;
  if (!damage) return null;
  return STRINGS.ANALYZER.damagedDemoNotice(
    clockFloor(damage.read_up_to_secs || 0),
    sizeText(damage.bytes_unread),
    damage.reason || STRINGS.ANALYZER.DAMAGED_DEMO_REASON_UNKNOWN,
  );
}

/**
 * Fills `el` with the notice for `report`, or hides it. With `onRepair`, the
 * notice has a Repair now button that runs `onRepair(update)`, with
 * `update(p)` for the banner's bar (a RepairProgress or a `{ pct, text }`
 * view), and for a demo that recorded more than one map (its report's, or
 * those `repairable`'s scan finds past the damage), a Repair & Split
 * that runs `onRepairSplit(update)` the same way (#15). While one runs both
 * buttons are disabled and the bar shows; if it throws, the banner says why.
 * Each resolves the line the banner shows after. Once `repairable`
 * (repair_status.js) finds no break for the repair to cut out, the notice
 * says Repair can't fix it in place of the buttons, and once `status`
 * (split_status.js) finds the repair's maps already split, the notice shows
 * no buttons either ([alreadySplitNote] says so when nothing else does).
 */
export function renderDamageBanner(el, report, onRepair, onRepairSplit, { repairable = repairableStatus, status = splitStatus } = {}) {
  if (!el) return;
  const notice = damageNotice(report);
  const path = report?.file_path || '';
  el.hidden = !notice;
  el.dataset.path = path;
  if (!notice || !onRepair) {
    el.textContent = notice || '';
    return;
  }
  const demo = report.demo_info;
  if (isUnrepairableDamage(demo, repairable.last(path))) {
    renderUnrepairable(el, notice);
    return;
  }
  const own = report?.state?.signon_maps || [];
  const mapsFor = (scanned) => repairMaps(own, scanned);
  const bothFor = (scanned) => Boolean(onRepairSplit) && mapsFor(scanned).length > 1;
  const known = path ? repairable.lastMaps(path) : null;
  const both = bothFor(known);
  // A split answer already in (this demo was open a moment ago) draws the
  // done state at once, without the buttons flashing up first.
  const lastSplit = both ? status.last(path) : null;
  if (isAlreadySplit(lastSplit)) renderAlreadySplit(el, notice, alreadySplitNote(own, mapsFor(known), lastSplit));
  else renderRepairButtons(el, { notice, both, onRepair, onRepairSplit });
  if (!path) return;
  const stillOurs = () => el.dataset.path === path && !el.querySelector('button:disabled');
  repairable.checkMaps(path, own).then((scanned) => {
    if (!stillOurs()) return;
    if (isUnrepairableDamage(demo, repairable.last(path))) {
      renderUnrepairable(el, notice);
      return;
    }
    const maps = mapsFor(scanned);
    // The scan found maps past the damage: Repair & Split joins Repair now.
    if (!both && bothFor(scanned)) renderRepairButtons(el, { notice, both: true, onRepair, onRepairSplit });
    if (!bothFor(scanned)) return;
    // Asked with the list the repair is split by, as the queue row asks
    // (queue_repair_hint.js), so a repaired-and-split demo reads as split.
    status.check(path, maps).then((answer) => {
      if (stillOurs() && isAlreadySplit(answer)) renderAlreadySplit(el, notice, alreadySplitNote(own, maps, answer));
    });
  });
}

/**
 * The line the damage notice adds once the repair's `maps` were split next
 * to the demo (`answer` split_status.js's), or null when the multi-map notice
 * above it already says so: it shows for a demo whose own analysis (`own`)
 * lists more than one map. Pure.
 */
export function alreadySplitNote(own, maps, answer) {
  if (!isAlreadySplit(answer) || (Array.isArray(own) && own.length > 1)) return null;
  return { text: STRINGS.REPAIR.analyzerAlreadySplit(maps), title: STRINGS.ANALYZER.multiMapAlreadySplitTitle(answer.files || []) };
}

/** The notice for a demo whose repair was already split: no buttons, and
 *  `note` (alreadySplitNote's) when there is one. */
function renderAlreadySplit(el, notice, note) {
  el.innerHTML = `<span class="analyzer-damage-text">${esc(notice)}</span>${note
    ? `<span class="analyzer-damage-already-split" title="${esc(note.title)}">${esc(note.text)}</span>`
    : ''}`;
}

/** The notice for damage the repair has no break to cut out: no buttons. */
function renderUnrepairable(el, notice) {
  el.innerHTML = `<span class="analyzer-damage-text">${esc(notice)}</span>
    <span class="analyzer-damage-cannot-repair">${esc(STRINGS.REPAIR.CANNOT_REPAIR)}</span>`;
}

/** The notice with Repair now (and Repair & Split when `both`). */
function renderRepairButtons(el, { notice, both, onRepair, onRepairSplit }) {
  const R = STRINGS.REPAIR;
  el.innerHTML = `<span class="analyzer-damage-text">${esc(notice)}</span>
    <button type="button" class="analyzer-damage-repair primary-btn" title="${esc(R.ANALYZER_BUTTON_TITLE)}">${esc(R.ANALYZER_BUTTON)}</button>
    ${both ? `<button type="button" class="analyzer-damage-repair-split primary-btn" title="${esc(R.REPAIR_SPLIT_TITLE)}">${esc(R.REPAIR_SPLIT_BUTTON)}</button>` : ''}
    <span class="analyzer-damage-status"></span>`;
  const status = el.querySelector('.analyzer-damage-status');
  const buttons = [...el.querySelectorAll('button')];
  const wire = (btn, run, failed) => btn?.addEventListener('click', async () => {
    buttons.forEach((b) => { b.disabled = true; });
    const bar = repairProgressBar();
    status.replaceChildren(bar.el);
    try {
      const text = await run(bar.update);
      status.textContent = text || '';
    } catch (err) {
      status.textContent = failed(err);
    } finally {
      buttons.forEach((b) => { b.disabled = false; });
    }
  });
  wire(el.querySelector('.analyzer-damage-repair'), onRepair, R.failed);
  if (both) wire(el.querySelector('.analyzer-damage-repair-split'), onRepairSplit, R.repairSplitFailed);
}
