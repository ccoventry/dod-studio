// queue_repair_hint.js
// A Master Demo Queue row's buttons for a demo damaged partway (#15): "Damaged
// · Repair", and for one that recorded more than one map as well, "Repair &
// Split" (its Split button is greyed out by queue_multimap.js: the damage
// stops a split). Once the repair's maps were split next to it
// (split_status.js counts the repair's pieces), the row needs neither and
// shows none. The jobs themselves are queue_repair.js's, through the
// handlers main.js passes in.

import { isDamagedDemo, repairProgressBar } from './repair_view.js';
import { isMultiMapDemo } from './queue_multimap.js';
import { isAlreadySplit, splitStatus } from './split_status.js';
import { clockFloor } from './time_format.js';
import { STRINGS } from './strings.js';

const R = STRINGS.REPAIR;

/**
 * The row's repair buttons in one span, or null for a whole demo (or when no
 * handler is wired). `handlers.repair(demo, update)` and, for a multi-map
 * demo, `handlers.repairSplit(demo, update)` run on click without selecting
 * the row; the first `update(p)` (a RepairProgress, or a `{ pct, text }`
 * view) swaps the buttons for a progress bar until the job ends.
 */
export function damagedRepairHint(demo, handlers, status = splitStatus) {
  if (!isDamagedDemo(demo) || !handlers?.repair) return null;
  const hint = document.createElement('span');
  hint.className = 'multimap-hint damaged-repair-hint';
  const buttons = [jobButton(R.QUEUE_BUTTON, R.queueButtonTitle(clockFloor(demo.damage.read_up_to_secs)), demo, handlers.repair, hint)];
  if (isMultiMapDemo(demo) && handlers.repairSplit) {
    buttons.push(jobButton(R.REPAIR_SPLIT_BUTTON, R.REPAIR_SPLIT_TITLE, demo, handlers.repairSplit, hint));
    // Repaired and split already: nothing left to offer on the original.
    const show = (answer) => { if (!hint.dataset.running) hint.hidden = isAlreadySplit(answer); };
    show(status.last(demo.path));
    status.check(demo.path, demo.signon_maps).then(show);
  }
  hint.append(...buttons);
  return hint;
}

/** One of the hint's buttons: runs `run(demo, update)`, with the hint's
 *  buttons hidden behind one bar while it runs. */
function jobButton(label, title, demo, run, hint) {
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = 'damaged-repair-btn';
  btn.textContent = label;
  btn.title = title;
  btn.addEventListener('click', async (e) => {
    e.stopPropagation(); // do not select the row
    const siblings = [...hint.querySelectorAll('button')];
    siblings.forEach((b) => { b.disabled = true; });
    hint.dataset.running = '1';
    let bar = null;
    const update = (p) => {
      if (!bar) {
        bar = repairProgressBar();
        bar.el.classList.add('multimap-split-progress');
        hint.append(bar.el);
        siblings.forEach((b) => { b.hidden = true; });
      }
      bar.update(p);
    };
    try {
      await run(demo, update);
    } finally {
      bar?.el.remove();
      siblings.forEach((b) => { b.hidden = false; b.disabled = false; });
      delete hint.dataset.running;
    }
  });
  return btn;
}
