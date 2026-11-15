// nav_tabs.js
// Wires the top nav, Configuration's tabs and Capture Studio's Highlights /
// Configuration / Render subtabs to nav.js, and re-checks the Commands tab's
// config warnings whenever it comes into view. Kept out of main.js, which only
// wires (#683).

import { switchNavTab, setCaptureDetailSubtab } from './nav.js';
import { refreshInitCommandWarnings } from './capture_pane.js';

// The game's config files change outside the app: the read-only flag on
// config.cfg (#478), or lines in config.cfg/movie.cfg. The Commands tab's
// warnings are only as fresh as their last check, so it runs again whenever
// that tab comes into view: opening it, returning to Configuration on it, or
// coming back to this window (from Explorer or an editor) while it shows.
function refreshCommandWarningsIfShown() {
  const tab = document.getElementById('tab-custom-commands');
  if (tab && tab.offsetParent !== null) refreshInitCommandWarnings();
}

export function startNavTabs() {
  const tabBtns = document.querySelectorAll('.config-tab-btn');
  tabBtns.forEach(btn => {
    btn.addEventListener('click', () => {
      tabBtns.forEach(b => b.classList.remove('active'));
      document.querySelectorAll('.config-tab-content').forEach(content => {
        content.style.display = 'none';
      });
      btn.classList.add('active');
      const targetEl = document.getElementById(btn.getAttribute('data-tab'));
      if (targetEl) targetEl.style.display = 'block';
      refreshCommandWarningsIfShown();
    });
  });
  window.addEventListener('focus', refreshCommandWarningsIfShown);

  switchNavTab('workspace');
  document.querySelectorAll('.nav-tab-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      switchNavTab(btn.getAttribute('data-nav'));
      refreshCommandWarningsIfShown();
    });
  });

  document.querySelectorAll('.capture-detail-subtab-btn').forEach((btn) => {
    btn.addEventListener('click', () => setCaptureDetailSubtab(btn.dataset.captureSubtab));
  });
}
