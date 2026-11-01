// demo_context_menu.js
// The right-click menu on a Master Demo Queue row (#1111). Every item runs
// the function an existing button or menu already runs (Launch Preview, the
// row's own Split / Locate / Remove buttons, Review, Open in Demo Analyzer,
// Reveal in Explorer); only the two copies are new, and they are one-line
// clipboard writes. It acts on the clicked row alone, however many are ticked.

import { createContextMenu } from './context_menu.js';
import { setDemoRowMenuHandler } from './master_pane.js';
import { revealInExplorer, logFrontendEvent } from './ipc_bridge.js';
import { openAnalyzerDemo } from './analyzer_pane.js';
import { switchNavTab } from './nav.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import { demoMenuItems } from './demo_menu_items.js';

/** What clicking the row's own button for an action does, or null when the row has none. */
function rowButton(row, selector) {
  const btn = row.querySelector(selector);
  return btn ? () => btn.click() : null;
}

async function copyText(text, what, demo) {
  try {
    await navigator.clipboard.writeText(text);
    showToast(STRINGS.CONTEXT_MENU.copied(what), 'info');
    logFrontendEvent(STRINGS.CONTEXT_MENU.copiedLog(what, demo.name || demo.path));
  } catch (err) {
    console.error('Clipboard write failed:', err);
    showToast(STRINGS.CONTEXT_MENU.COPY_FAILED, 'error');
    logFrontendEvent(STRINGS.CONTEXT_MENU.copyFailedLog(what, err));
  }
}

/**
 * Hooks the menu onto the queue's rows.
 * @param {object} app
 * @param {Function} app.startReview  review_mode.js's startReview(paths, opts)
 */
export function startDemoContextMenu({ startReview }) {
  const menu = createContextMenu();
  setDemoRowMenuHandler(({ demo, row, select, at, fromKeyboard }) => {
    const actions = {
      // Preview and Review work on the demo open in Highlight Details, so the
      // row is selected first, exactly as a left click would.
      launchPreview: () => { if (select()) document.querySelector('#btn-launch-preview')?.click(); },
      review: () => startReview([demo.path], {
        covers: document.querySelector('#review-covers-select')?.value,
      }),
      openInAnalyzer: () => { switchNavTab('demo-analyzer'); openAnalyzerDemo(demo.path); },
      openFolder: () => {
        logFrontendEvent(STRINGS.CONTEXT_MENU.openFolderLog(demo.path));
        revealInExplorer(demo.path).catch(() => {});
      },
      split: rowButton(row, '.multimap-split-btn'),
      useFound: rowButton(row, '.use-found-copy-btn'),
      locate: rowButton(row, '.locate-demo-btn'),
      copyName: () => copyText(demo.name || '', STRINGS.CONTEXT_MENU.WHAT_NAME, demo),
      copyPath: () => copyText(demo.path, STRINGS.CONTEXT_MENU.WHAT_PATH, demo),
      remove: rowButton(row, '.list-editor-remove-btn'),
    };
    row.classList.add('context-target');
    menu.open({
      at,
      items: demoMenuItems(demo, actions),
      label: STRINGS.CONTEXT_MENU.label(demo.name || demo.path),
      focusFirst: fromKeyboard,
      onClose: () => row.classList.remove('context-target'),
    });
  });
}
