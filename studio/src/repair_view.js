// repair_view.js
// How a damaged demo's repair (#15) shows: its progress as a bar and a step
// line, and what it came to in a sentence. Shared by the Demo Auditor's
// Damaged Demos tab, the Master Queue's Repair button and the Demo
// Analyzer's Repair now. The repair itself is demo_repair_manager.rs, which
// sends `repair_progress` events ({ path, progress: { step, fraction } }) and
// resolves a RepairResult ({ verdict, written, holes, seconds_lost,
// percent_kept, ends_at_last_hole, reasons }).
//
// Pure, apart from the bar's own elements.

import { STRINGS } from './strings.js';
import { clockFloor } from './time_format.js';
import { fileNameOf } from './path_display.js';

/** Whether a queued demo (or an analysis report's demo_info) is damaged
 *  partway (#811): it reads up to a break. */
export function isDamagedDemo(demo) {
  return demo?.damage != null;
}

/** `{ pct, text }` for one RepairProgress: pct 0-100, whole numbers. */
export function repairProgressView(p) {
  const pct = Math.max(0, Math.min(100, Math.round((p?.fraction || 0) * 100)));
  const step = STRINGS.REPAIR.STEP[p?.step] || STRINGS.REPAIR.STEP.finding_holes;
  return { pct, text: STRINGS.REPAIR.progressLine(step, pct) };
}

/**
 * A progress bar for a repair: `{ el, update(p) }`, `update` taking a
 * RepairProgress or a `{ pct, text }` view. Starts at 0% on the first step,
 * so it shows something the moment the button is pressed.
 */
export function repairProgressBar() {
  const el = document.createElement('div');
  el.className = 'split-progress repair-progress';
  el.innerHTML = '<div class="progress-bar-container"><div class="progress-bar-fill" style="width: 0%"></div></div>'
    + '<span class="split-progress-text"></span>';
  const fill = el.querySelector('.progress-bar-fill');
  const text = el.querySelector('.split-progress-text');
  const update = (p) => {
    const view = p && 'pct' in p ? p : repairProgressView(p);
    fill.style.width = `${view.pct}%`;
    text.textContent = view.text;
  };
  update(null);
  return { el, update };
}

/** The gaps as "0:39–1:08 (29 s), 13:36–13:48 (13 s)". */
export function gapsText(holes) {
  return (holes || [])
    .map((h) => STRINGS.REPAIR.gap(clockFloor(h.before_secs), clockFloor(h.after_secs), Math.round(h.lost_secs)))
    .join(', ');
}

/** One sentence for what a repair came to. */
export function repairResultText(result) {
  const R = STRINGS.REPAIR;
  switch (result?.verdict) {
    case 'repaired': {
      const text = R.repaired(result.holes.length, Math.round(result.seconds_lost),
        result.percent_kept.toFixed(1), fileNameOf(result.written || ''));
      return result.ends_at_last_hole ? `${text} ${R.ENDS_AT_LAST_GAP}` : text;
    }
    case 'not_damaged': return R.NOT_DAMAGED;
    case 'refused': return R.refused(result.reasons || []);
    case 'cancelled': return R.CANCELLED;
    default: return '';
  }
}

/** The tooltip listing a repair's gaps, or '' when it found none. */
export function repairGapsTitle(result) {
  const list = gapsText(result?.holes);
  return list ? STRINGS.REPAIR.gapsTitle(list) : '';
}
