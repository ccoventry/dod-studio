// auditor_tabs.js
// The Demo Auditor's tabs (#624): one per tool, all working on the folder
// picked at the top of the page.

/** Wires the tab buttons to their panels. `onShow(tab)` runs when a tab is
 *  picked, so a tool can refresh what it shows. */
export function initAuditorTabs(onShow = () => {}) {
  const buttons = [...document.querySelectorAll('.auditor-tab-btn')];
  const panels = [...document.querySelectorAll('.auditor-tab-panel')];
  const show = (tab) => {
    for (const b of buttons) {
      const on = b.dataset.auditorTab === tab;
      b.classList.toggle('active', on);
      b.setAttribute('aria-selected', String(on));
    }
    for (const p of panels) p.hidden = p.dataset.auditorPanel !== tab;
    onShow(tab);
  };
  for (const b of buttons) b.addEventListener('click', () => show(b.dataset.auditorTab));
  return show;
}
