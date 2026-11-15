// app_shortcuts.js
// App-wide keyboard shortcuts and the suppressed browser context menu. Kept
// out of main.js, which only wires (#683).

export function startAppShortcuts() {
  window.addEventListener('keydown', (e) => {
    const mod = e.ctrlKey || e.metaKey;
    // Chromium's autofill sends keydown events with no key.
    const key = (e.key || '').toLowerCase();
    const isCtrlO = mod && key === 'o';
    const isCtrlS = mod && key === 's';
    const isCtrlN = mod && key === 'n';
    const isCtrlW = mod && key === 'w';
    // WebView2 keeps the browser's reload shortcuts live by default, and a
    // desktop app has no "refresh the page" affordance, so they're swallowed
    // outright rather than routed through close_guard.js's beforeunload
    // check (which still covers other reload paths, e.g. devtools).
    const isReload = e.key === 'F5' || (mod && key === 'r');

    if (isCtrlO || isCtrlS || isCtrlN || isCtrlW || isReload) {
      e.preventDefault();
    }

    // New/Save/Load Session work from any tab (#122): the buttons live in
    // the always-visible File menu.
    if (isCtrlN) document.querySelector('#new-session-btn')?.click();
    if (isCtrlO) document.querySelector('#load-project-btn')?.click();
    if (isCtrlS) document.querySelector('#save-project-btn')?.click();
  });

  // WebView2's native right-click menu (Reload/Inspect/browser Cut-Copy-
  // Paste) reads as a web page, not an app. Ctrl+C/X/V don't route through
  // it, so text fields keep normal copy/paste.
  window.addEventListener('contextmenu', (e) => e.preventDefault());
}
