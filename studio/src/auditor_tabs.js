// auditor_tabs.js
// The Demo Auditor's tabs (#624): one per tool. Split Maps and Rename Demos
// work on the folder picked at the top of the page; Duplicates has its own
// list of folders (#844), so the top row is hidden on that tab.

import { showAuditorFooterFor } from './auditor_footer.js';

/** Tabs that pick their folders themselves, without the Target Folder row. */
const OWN_FOLDERS = new Set(['duplicates']);

/** Wires the tab buttons to their panels. `onShow(tab)` runs when a tab is
 *  picked, so a tool can refresh what it shows. */
export function initAuditorTabs(onShow = () => {}) {
  const buttons = [...document.querySelectorAll('.auditor-tab-btn')];
  const panels = [...document.querySelectorAll('.auditor-tab-panel')];
  const targetRow = document.querySelector('#audit-target-row');
  const showTargetRow = (tab) => {
    if (targetRow) targetRow.style.display = OWN_FOLDERS.has(tab) ? 'none' : '';
  };
  const show = (tab) => {
    for (const b of buttons) {
      const on = b.dataset.auditorTab === tab;
      b.classList.toggle('active', on);
      b.setAttribute('aria-selected', String(on));
    }
    for (const p of panels) p.hidden = p.dataset.auditorPanel !== tab;
    showTargetRow(tab);
    showAuditorFooterFor(tab);
    onShow(tab);
  };
  for (const b of buttons) b.addEventListener('click', () => show(b.dataset.auditorTab));
  showTargetRow(buttons.find((b) => b.classList.contains('active'))?.dataset.auditorTab);
  return show;
}
