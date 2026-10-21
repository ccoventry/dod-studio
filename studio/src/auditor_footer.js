// auditor_footer.js
// The status bar's Demo Auditor line follows the tab on show: Duplicates'
// copies and wasted space, Split Maps' multi-map demos, Rename Demos' list.
// Each tab sets its own line; the one for the tab on show is drawn.

import { STRINGS } from './strings.js';

const lines = {
  duplicates: STRINGS.AUDITOR.FOOTER_DEFAULT,
  split: STRINGS.SPLIT.footer(0, 0),
  rename: STRINGS.DEMO_RENAME.footer(0, 0),
};
let shown = 'duplicates';

function draw() {
  const el = document.querySelector('#footer-audit-summary');
  if (el) el.textContent = lines[shown] ?? '';
}

/** Sets `tab`'s line; drawn at once when that tab is on show. */
export function setAuditorFooter(tab, text) {
  lines[tab] = text;
  if (tab === shown) draw();
}

/** The tab on show changed. */
export function showAuditorFooterFor(tab) {
  if (!(tab in lines)) return;
  shown = tab;
  draw();
}
