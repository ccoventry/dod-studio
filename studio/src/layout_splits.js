// layout_splits.js
// The panel splits the user can drag and the app remembers across restarts:
// Capture Studio's queue / details panes and the Demo Analyzer's Demos panel /
// report (#47), then the Overviews side panels and the Render and Auditor
// tables (#46). A saved size of null means "the default layout"; a
// double-click on a handle goes back to it. The Explorer sidebar keeps its own
// width in demo_folders.js.

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

// Overviews' Maps and Edit side panels: narrowest that keeps their controls
// readable, and the room the map itself keeps between them.
const OVERVIEWS_MIN_SIDE_PX = 180;
const OVERVIEWS_MIN_MAP_PX = 300;
// A table shorter than this shows no rows; taller is a scroll box nobody asked for.
const TABLE_MIN_HEIGHT_PX = 120;
const TABLE_MAX_HEIGHT_PX = 1600;

/**
 * The splits whose size is a plain pixel count, one settings field each.
 * `field` is the settings.json key, `target` the pane that resizes, `peer` the
 * pane across the layout that keeps its own size (width splits only).
 */
const PX_SPLITS = [
  {
    field: 'overviews_maps_width_px', label: () => STRINGS.LAYOUT.OVERVIEWS_MAPS_LABEL,
    handle: '#ov-maps-resize-handle', target: '#ov-maps-panel', peer: '#ov-edit-panel',
    container: '.ov-layout', axis: 'x', invert: false,
  },
  {
    field: 'overviews_edit_width_px', label: () => STRINGS.LAYOUT.OVERVIEWS_EDIT_LABEL,
    handle: '#ov-edit-resize-handle', target: '#ov-edit-panel', peer: '#ov-maps-panel',
    container: '.ov-layout', axis: 'x', invert: true,
  },
  {
    field: 'render_table_height_px', label: () => STRINGS.LAYOUT.RENDER_TABLE_LABEL,
    handle: '#render-jobs-resize-handle', target: '#render-jobs-wrap', axis: 'y',
  },
  {
    field: 'auditor_table_height_px', label: () => STRINGS.LAYOUT.AUDITOR_TABLE_LABEL,
    handle: '#audit-results-resize-handle', target: '#audit-results-wrap', axis: 'y',
  },
];

/** @param {object} app  persist() saves the settings file. */
export function createLayoutSplits({ persist }) {
  let captureLeftPermille = null;
  let analyzerDemosPx = null;
  /** Saved size per PX_SPLITS field; null = the default layout. */
  const pxSizes = {};

  /** Fills the sizes from a loaded settings file; absent means the default layout. */
  function load(settings) {
    captureLeftPermille = settings.capture_split_left_permille ?? null;
    analyzerDemosPx = settings.analyzer_demos_height_px ?? null;
    for (const spec of PX_SPLITS) pxSizes[spec.field] = settings[spec.field] ?? null;
  }

  function save(label) {
    persist().catch((err) => logFrontendEvent(STRINGS.LAYOUT.splitNotSaved(label, err)));
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
        save(STRINGS.LAYOUT.CAPTURE_SPLIT_LABEL);
      },
      reset: () => {
        captureLeftPermille = null;
        showSaved();
        save(STRINGS.LAYOUT.CAPTURE_SPLIT_LABEL);
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
        save(STRINGS.LAYOUT.ANALYZER_SPLIT_LABEL);
      },
      reset: () => {
        analyzerDemosPx = null;
        show(null);
        save(STRINGS.LAYOUT.ANALYZER_SPLIT_LABEL);
        logFrontendEvent(STRINGS.LAYOUT.ANALYZER_SPLIT_RESET_LOG);
      },
    });
  }

  /** One PX_SPLITS entry: a panel width (x) or a table height (y) in pixels. */
  function startPxSplit(spec) {
    const handle = document.querySelector(spec.handle);
    const target = document.querySelector(spec.target);
    const container = spec.container ? document.querySelector(spec.container) : null;
    const peer = spec.peer ? document.querySelector(spec.peer) : null;
    if (!handle || !target || (spec.container && !container)) return;

    // A table keeps the inline max-height the page gives it until a size is saved.
    const defaultMaxHeight = target.style.maxHeight;
    const show = (px) => {
      if (spec.axis === 'x') {
        target.style.flex = px == null ? '' : `0 0 ${px}px`;
        return;
      }
      target.style.height = px == null ? '' : `${px}px`;
      target.style.maxHeight = px == null ? defaultMaxHeight : 'none';
    };
    show(pxSizes[spec.field]);

    attachSplitResize({
      handle,
      axis: spec.axis,
      invert: spec.invert,
      measure: () => target.getBoundingClientRect()[spec.axis === 'x' ? 'width' : 'height'],
      limits: () => (spec.axis === 'x'
        ? {
          min: OVERVIEWS_MIN_SIDE_PX,
          max: container.clientWidth - peer.getBoundingClientRect().width - 2 * HANDLE_PX - OVERVIEWS_MIN_MAP_PX,
        }
        : { min: TABLE_MIN_HEIGHT_PX, max: TABLE_MAX_HEIGHT_PX }),
      apply: show,
      commit: (px) => {
        pxSizes[spec.field] = Math.round(px);
        save(spec.label());
      },
      reset: () => {
        pxSizes[spec.field] = null;
        show(null);
        save(spec.label());
        logFrontendEvent(STRINGS.LAYOUT.splitResetLog(spec.label()));
      },
    });
  }

  return {
    load,
    /** Wires every handle; call once, after the page exists. */
    start: () => {
      startCaptureSplit();
      startAnalyzerSplit();
      PX_SPLITS.forEach(startPxSplit);
    },
    /** The fields settings_sync.js writes. */
    settingsState: () => ({
      captureSplitLeftPermille: captureLeftPermille,
      analyzerDemosHeightPx: analyzerDemosPx,
      layoutPxSplits: { ...pxSizes },
    }),
  };
}
