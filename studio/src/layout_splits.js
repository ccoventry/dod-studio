// layout_splits.js
// The two panel splits the user can drag and the app remembers across
// restarts (#47): Capture Studio's queue / details panes and the Demo
// Analyzer's Demos panel / report. A saved size of null means "the default
// layout"; a double-click on a handle goes back to it. The Explorer sidebar
// keeps its own width in demo_folders.js.

import { attachSplitResize, pxToPermille } from './split_resize.js';
import { logFrontendEvent } from './ipc_bridge.js';
import { STRINGS } from './strings.js';

// Smallest widths that keep the Master Demo Queue and the Highlight Details
// tables readable beside each other (#47).
const CAPTURE_MIN_LEFT_PX = 380;
const CAPTURE_MIN_RIGHT_PX = 520;
// Room a Demos panel can't shrink below (header + filters + a few rows) and
// the Report panel can't be squeezed under.
const ANALYZER_MIN_DEMOS_PX = 140;
const ANALYZER_MIN_REPORT_PX = 220;
const HANDLE_PX = 10;

/** @param {object} app  persist() saves the settings file. */
export function createLayoutSplits({ persist }) {
  let captureLeftPermille = null;
  let analyzerDemosPx = null;

  /** Fills the sizes from a loaded settings file; absent means the default layout. */
  function load(settings) {
    captureLeftPermille = settings.capture_split_left_permille ?? null;
    analyzerDemosPx = settings.analyzer_demos_height_px ?? null;
  }

  function save(label) {
    persist().catch((err) => logFrontendEvent(`${label} not saved: ${err}`));
  }

  function startCaptureSplit() {
    const handle = document.querySelector('#capture-split-handle');
    const left = document.querySelector('#pane-workspace');
    const container = document.querySelector('#main-content-split');
    if (!handle || !left || !container) return;

    const showSaved = () => {
      left.style.flex = captureLeftPermille == null ? '' : `0 0 ${captureLeftPermille / 10}%`;
    };
    showSaved();

    attachSplitResize({
      handle,
      axis: 'x',
      measure: () => left.getBoundingClientRect().width,
      limits: () => ({
        min: CAPTURE_MIN_LEFT_PX,
        max: container.clientWidth - HANDLE_PX - CAPTURE_MIN_RIGHT_PX,
      }),
      // Percent, so the split follows the window when it is resized.
      apply: (px) => { left.style.flex = `0 0 ${(px / container.clientWidth) * 100}%`; },
      commit: (px) => {
        captureLeftPermille = pxToPermille(px, container.clientWidth);
        save('Capture Studio split');
      },
      reset: () => {
        captureLeftPermille = null;
        showSaved();
        save('Capture Studio split');
        logFrontendEvent(STRINGS.LAYOUT.CAPTURE_SPLIT_RESET_LOG);
      },
    });
  }

  function startAnalyzerSplit() {
    const handle = document.querySelector('#analyzer-demos-resize-handle');
    const demos = document.querySelector('.analyzer-demos-panel');
    const column = document.querySelector('.analyzer-main-column');
    if (!handle || !demos || !column) return;

    const show = (px) => {
      demos.style.height = px == null ? '' : `${px}px`;
      demos.style.maxHeight = px == null ? '' : 'none';
    };
    show(analyzerDemosPx);

    attachSplitResize({
      handle,
      axis: 'y',
      measure: () => demos.getBoundingClientRect().height,
      limits: () => ({
        min: ANALYZER_MIN_DEMOS_PX,
        max: column.clientHeight - HANDLE_PX - ANALYZER_MIN_REPORT_PX,
      }),
      apply: show,
      commit: (px) => {
        analyzerDemosPx = Math.round(px);
        save('Demo Analyzer split');
      },
      reset: () => {
        analyzerDemosPx = null;
        show(null);
        save('Demo Analyzer split');
        logFrontendEvent(STRINGS.LAYOUT.ANALYZER_SPLIT_RESET_LOG);
      },
    });
  }

  return {
    load,
    /** Wires both handles; call once, after the page exists. */
    start: () => { startCaptureSplit(); startAnalyzerSplit(); },
    /** The fields settings_sync.js writes. */
    settingsState: () => ({ captureSplitLeftPermille: captureLeftPermille, analyzerDemosHeightPx: analyzerDemosPx }),
  };
}
