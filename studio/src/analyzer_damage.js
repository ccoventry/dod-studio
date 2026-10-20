// analyzer_damage.js
// A demo damaged partway (#811): the analyzer reads it up to the first frame
// that does not parse, and everything the Demo Analyzer shows stops there.
// It says so above every tab, with where it stopped on the viewdemo clock.
//
// Pure, apart from the banner it fills in.

import { STRINGS } from './strings.js';
import { clockFloor } from './time_format.js';

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

/** Fills `el` with the notice for `report`, or hides it. */
export function renderDamageBanner(el, report) {
  if (!el) return;
  const notice = damageNotice(report);
  el.hidden = !notice;
  el.textContent = notice || '';
}
