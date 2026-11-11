// analyzer_damage.js
// A demo damaged partway (#811): the analyzer reads it up to the first frame
// that does not parse, and everything the Demo Analyzer shows stops there.
// It says so above every tab, with where it stopped on the viewdemo clock,
// and offers Repair now (#15): the repair is the caller's, through `onRepair`.
// Damage the repair has no break to cut out (repair_status.js) keeps the
// notice, which says Repair can't fix it instead of offering it.
//
// Pure, apart from the banner it fills in.

import { STRINGS } from './strings.js';
import { clockFloor } from './time_format.js';
import { escapeHtml as esc } from './html.js';
import { repairProgressBar } from './repair_view.js';
import { isUnrepairableDamage, repairableStatus } from './repair_status.js';

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
 * view), and for a demo that recorded more than one map, a Repair & Split
 * that runs `onRepairSplit(update)` the same way (#15). While one runs both
 * buttons are disabled and the bar shows; if it throws, the banner says why.
 * Each resolves the line the banner shows after. Once `repairable`
 * (repair_status.js) finds no break for the repair to cut out, the notice
 * says Repair can't fix it in place of the buttons.
 */
export function renderDamageBanner(el, report, onRepair, onRepairSplit, repairable = repairableStatus) {
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
  renderRepairButtons(el, { notice, both: (report?.state?.signon_maps || []).length > 1 && onRepairSplit, onRepair, onRepairSplit });
  if (!path) return;
  repairable.check(path).then((answer) => {
    if (el.dataset.path !== path || el.querySelector('button:disabled')) return;
    if (isUnrepairableDamage(demo, answer)) renderUnrepairable(el, notice);
  });
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
