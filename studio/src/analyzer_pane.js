// analyzer_pane.js
// Standalone "Demo Analyzer" tab: the Tauri wiring around the report viewer
// (analyzer_report.js) and the Explorer sidebar (analyzer_browser.js). It runs
// `analyze_demo_full`, shows its progress and hands the full
// analysis::{DemoInfo,AnalyzerState} payload to the report.

import { pickPath } from './pick_path.js';
import { listen } from '@tauri-apps/api/event';
import { analyzeDemoFull, getWeaponDisplayNames, loadMapOverview, splitDemoAuto } from './ipc_bridge.js';
import { renderMultiMapBanner } from './analyzer_multimap.js';
import { renderDamageBanner } from './analyzer_damage.js';
import { renderNoCameraDataBanner } from './camera_data.js';
import { SHORT_MAP_SECONDS } from './split_pane.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import { escapeHtml as esc } from './html.js';
import { analyzerLoadingView, analyzerLoadingHtml } from './analyzer_loading.js';
import { escapeHtml as esc } from './html.js';
import { startAnalyzerReport, showReport, setActiveSubTab } from './analyzer_report.js';
import { fileNameOf } from './path_display.js';
import { startAnalyzerBrowser, browserFolder, revealDemo, markDemoSelected, relistFolderOf } from './analyzer_browser.js';
// demo_folders.js redraws the quick links through the pane's interface.
export { refreshAnalyzerQuickLinks } from './analyzer_browser.js';

function setAnalyzerFileIndicator(text) {
  const titleEl = document.querySelector('#analyzer-current-file');
  const footerEl = document.querySelector('#footer-analyzer-current-file');
  if (titleEl) titleEl.textContent = text;
  if (footerEl) footerEl.textContent = text || STRINGS.ANALYZER.NO_DEMO_LOADED;
}

// The demo whose analysis the page is waiting on (null when none), and a
// count of loads: clicking a second demo before the first finishes leaves
// both analyses running, and only the latest may show its progress or result.
let analyzerLoadingPath = null;
let analyzerLoadSeq = 0;

// ── Init / entry points ──────────────────────────────────────────────────────

// Registered once here (not per-load) to avoid the double-registration bug
// noted for render_status in ipc_bridge.js — analyzer_progress is throttled
// to ~30fps backend-side (Rust `analyze_demo_full`), so no further
// throttling is needed on the receiving end.
// Each event names its demo: an earlier click's analysis still running in
// the background reports too, and mixing the two made the % jump around.
// A cached demo's event says so (`cached`) and counts nothing.
listen('analyzer_progress', (event) => {
  const p = event.payload || {};
  if (!analyzerLoadingPath || p.path !== analyzerLoadingPath) return;
  if (!p.total && !p.cached) return;
  showAnalyzerLoading(p);
});

function showAnalyzerLoading(p) {
  const view = analyzerLoadingView(p);
  const container = document.querySelector('#analyzer-tab-content');
  setAnalyzerFileIndicator(view.short);
  if (container) container.innerHTML = analyzerLoadingHtml(view);
}

/** Wires the Analyzer tab. `explorer` holds the settings the Explorer
 *  sidebar reads and writes (analyzer_browser.js's `startAnalyzerBrowser`). */
export function initAnalyzerPane(explorer = {}) {
  startAnalyzerReport({ getWeaponDisplayNames, loadMapOverview });

  const browseBtn = document.querySelector('#analyzer-browse-btn');
  if (browseBtn) {
    browseBtn.addEventListener('click', async () => {
      try {
        const selected = await pickPath({
          field: 'analyzer.demo',
          // The folder the Explorer is showing.
          current: browserFolder(),
          multiple: false,
          filters: [{ name: STRINGS.MAIN.DEMO_FILES_FILTER_NAME, extensions: ['dem'] }],
          title: STRINGS.ANALYZER.SELECT_DEMO_DIALOG_TITLE,
        });
        if (selected) {
          const path = Array.isArray(selected) ? selected[0] : selected;
          loadAnalyzerDemo(path);
        }
      } catch (err) {
        console.error('Error opening demo file dialog:', err);
      }
    });
  }

  document.querySelectorAll('.analyzer-subtab-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      document.querySelectorAll('.analyzer-subtab-btn').forEach((b) => b.classList.remove('active'));
      btn.classList.add('active');
      setActiveSubTab(btn.dataset.subtab);
    });
  });

  startAnalyzerBrowser({ ...explorer, openDemo: loadAnalyzerDemo });
}

// Entry point for cross-pane jumps (e.g. Workspace's "View Match Telemetry"
// button) — points the Explorer sidebar at the demo's own folder first, so
// the Demos table reliably highlights it instead of only coincidentally
// matching whatever folder the sidebar was already browsing. The folder's
// listing runs beside the analysis, not before it: a big folder takes seconds
// to list, and the tab stayed blank until it was done.
export async function openAnalyzerDemo(path) {
  await Promise.all([revealDemo(path), loadAnalyzerDemo(path)]);
}

async function loadAnalyzerDemo(path) {
  // Already being analysed: a second run of the same demo would only race
  // the first one's progress.
  if (analyzerLoadingPath === path) return;
  const container = document.querySelector('#analyzer-tab-content');
  showAnalyzerLoading(null);
  // The last demo's multi-map notice isn't about this one.
  renderMultiMapBanner(document.querySelector('#analyzer-multimap-banner'), null);
  renderDamageBanner(document.querySelector('#analyzer-damage-banner'), null);
  renderNoCameraDataBanner(document.querySelector('#analyzer-camera-banner'), null);
  const seq = ++analyzerLoadSeq;
  analyzerLoadingPath = path;
  try {
    const result = await analyzeDemoFull(path);
    if (seq !== analyzerLoadSeq) return; // another demo was picked meanwhile
    setAnalyzerFileIndicator(result.file_name);
    markDemoSelected(path);
    showReport(result);
    renderMultiMapBanner(document.querySelector('#analyzer-multimap-banner'), result, (update) => splitAnalyzedDemo(path, update));
    renderDamageBanner(document.querySelector('#analyzer-damage-banner'), result);
    renderNoCameraDataBanner(document.querySelector('#analyzer-camera-banner'), result);
  } catch (err) {
    if (seq !== analyzerLoadSeq) return;
    if (container) {
      container.innerHTML = `<p class="analyzer-empty" style="color:#f44336;">${STRINGS.ANALYZER.analyzeFailed(esc(String(err)))}</p>`;
    }
    setAnalyzerFileIndicator('');
  } finally {
    if (seq === analyzerLoadSeq) analyzerLoadingPath = null;
  }
}

// #217: Split now on a demo that recorded more than one map. Writes each map
// (but the stub of a next map) as a demo next to this one, with `update`
// fed native's progress, lists them in the Explorer when it shows that
// folder, and opens the first.
async function splitAnalyzedDemo(path, update) {
  const unlisten = await listen('split_progress', (event) => {
    if (event.payload?.path === path) update(event.payload.progress);
  });
  let written;
  try {
    written = await splitDemoAuto(path, SHORT_MAP_SECONDS);
  } finally {
    unlisten();
  }
  showToast(STRINGS.ANALYZER.multiMapSplitDone(written.map((w) => fileNameOf(w.path))), 'success', 6000);
  await relistFolderOf(path);
  if (written.length) await loadAnalyzerDemo(written[0].path);
}
