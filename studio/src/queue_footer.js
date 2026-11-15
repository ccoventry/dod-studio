// queue_footer.js
// The Master Queue's footer line and the Clear buttons that need a non-empty
// queue. Kept out of main.js, which only wires (#683).

import { listedStreaks } from './master_pane.js';
import { STRINGS } from './strings.js';

export function renderQueueFooter(demos) {
  const footerEl = document.querySelector('#demo-list-footer');
  if (footerEl) {
    // The same highlights the Highlights column counts (M2).
    const totalHighlights = (demos || []).reduce((sum, d) => sum + listedStreaks(d).length, 0);
    footerEl.textContent = STRINGS.WORKSPACE.demoListFooter((demos || []).length, totalHighlights);
  }
  // Clear Untracked/Clear All only make sense with something in the queue;
  // Clear Selected already gates on its own checkbox state (master_pane.js).
  const isEmpty = !demos || demos.length === 0;
  const clearUntrackedBtnEl = document.querySelector('#clear-untracked-btn');
  if (clearUntrackedBtnEl) clearUntrackedBtnEl.disabled = isEmpty;
  const clearAllBtnEl = document.querySelector('#clear-all-btn');
  if (clearAllBtnEl) clearAllBtnEl.disabled = isEmpty;
}
