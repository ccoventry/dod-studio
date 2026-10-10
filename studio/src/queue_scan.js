// queue_scan.js
// Scanning demos into the Master Demo Queue: + Add Demo Files, + Add Folder,
// Cancel Scan, the scan_progress status line, and triggerAutoScan, which
// merges a scan's results into the queue. Moved out of main.js (#683), which
// owns the queue, the selection and the pinned-folder list; they're reached
// through the getters and setters below.

import { open } from '@tauri-apps/plugin-dialog';
import { listen } from '@tauri-apps/api/event';
import { scanDirectory, cancelScan } from './ipc_bridge.js';
import { renderMasterList } from './master_pane.js';
import { refreshMapWarnings } from './map_warnings.js';
import { refreshTeamsPane } from './teams_pane.js';
import { preserveHighlightState } from './take_index.js';
import { demoHasTeams } from './project_teams.js';
import { splitIdenticalCopies } from './demo_copies.js';
import { fileNameOf, samePath } from './path_display.js';
import { parseClock, timeLeftText } from './scan_progress.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

// The scan reads the demos the analyzer cache has first (#687); its time
// left is measured from when the parses after them started.
const scanParses = parseClock();

/**
 * @param {object} app  What this needs from main.js:
 *   getDemos() the queue (merged into in place), setSelectedDemoIdx(idx),
 *   selectDemoAndRenderDetail(demo, idx), updateDemoFooter(demos),
 *   markProjectDirty(), readScanWorkers(), offerIdenticalCopies(copies,
 *   pickedFiles) from project_demos.js, getScanPaths() the pinned-folder
 *   list (pushed to in place), and persistAppSettings().
 */
export function createQueueScan({
  getDemos,
  setSelectedDemoIdx,
  selectDemoAndRenderDetail,
  updateDemoFooter,
  markProjectDirty,
  readScanWorkers,
  offerIdenticalCopies,
  getScanPaths,
  persistAppSettings,
}) {
  // ── scan_progress event listener (registered once on load) ────────────────
  let unlistenScanProgress = null;
  listen('scan_progress', (event) => {
    const p = event.payload || {};
    const scanStatusEl = document.querySelector('#scan-status');
    const cancelScanBtn = document.querySelector('#cancel-scan-btn');
    const masterTableBody = document.querySelector('#master-demo-table-body');

    // Demos the scan could not read (#23). Only the final event carries it.
    const skipped = Array.isArray(p.skipped) ? p.skipped : [];
    if (skipped.length > 0) {
      console.warn('Scan skipped unreadable demos:', skipped);
      showToast(STRINGS.MAIN.skippedDemosToast(skipped), 'warning', 10000);
    }

    if (p.cancelled) {
      if (scanStatusEl) scanStatusEl.textContent = STRINGS.MAIN.cancelledStatus(p.found) + STRINGS.MAIN.skippedStatusSuffix(skipped.length);
      if (cancelScanBtn) cancelScanBtn.disabled = true;
    } else if (p.status === 'Complete') {
      if (scanStatusEl) scanStatusEl.textContent = STRINGS.MAIN.readyFoundStatus(p.found) + STRINGS.MAIN.skippedStatusSuffix(skipped.length);
      if (cancelScanBtn) cancelScanBtn.disabled = true;
    } else {
      if (scanStatusEl) {
        scanStatusEl.textContent = STRINGS.MAIN.statusGeneric(p.status)
          + timeLeftText(p, scanParses.elapsed(p.scanned, p.cached));
      }
      if (cancelScanBtn) cancelScanBtn.disabled = false;
    }
  }).then(fn => { unlistenScanProgress = fn; });

  // ── Cancel Scan button ────────────────────────────────────────────────────
  const cancelScanBtn = document.querySelector('#cancel-scan-btn');
  if (cancelScanBtn) {
    cancelScanBtn.disabled = true;
    cancelScanBtn.addEventListener('click', async () => {
      cancelScanBtn.disabled = true;
      try {
        await cancelScan();
        showToast(STRINGS.MAIN.SCAN_CANCEL_REQUESTED_TOAST, 'info');
      } catch (_) { /* already toasted in ipc_bridge */ }
    });
  }

  // Scans only the given paths and merges the results into the existing
  // master list (replacing entries with matching `path`, appending new ones).
  // `scanPaths` is the app-wide pinned-folder list (settings' pinned_folders,
  // also the Demo Analyzer's Pinned tier) -- it must NOT be re-walked on every
  // add, or every scan re-processes every folder ever added across the app's
  // lifetime. Capture's output folders are a separate list (targetDrives).
  //
  // Resolves true when the scan ran (a cancelled one included), false when it
  // failed -- e.g. every picked path is gone, which the backend reports (#432).
  // `pickedFiles`: the paths are files the user picked one by one (+ Add
  // Demo Files), so a copy of a queued demo is offered in its place rather
  // than only skipped (#21).
  async function triggerAutoScan(pathsToScan, { pickedFiles = false } = {}) {
    if (!pathsToScan || pathsToScan.length === 0) return false;

    const scanStatusEl = document.querySelector('#scan-status');
    const scanSpinnerEl = document.querySelector('#scan-spinner');
    const addFilesBtn = document.querySelector('#add-files-btn');
    const addFolderBtn = document.querySelector('#add-folder-btn');
    const cancelScanBtnInner = document.querySelector('#cancel-scan-btn');

    if (addFilesBtn) addFilesBtn.disabled = true;
    if (addFolderBtn) addFolderBtn.disabled = true;
    if (cancelScanBtnInner) cancelScanBtnInner.disabled = false;
    if (scanSpinnerEl) scanSpinnerEl.style.display = 'inline-block';
    if (scanStatusEl) scanStatusEl.textContent = STRINGS.MAIN.SCANNING_STATUS;
    scanParses.reset();
    showToast(STRINGS.MAIN.SCANNING_TOAST, 'info');

    const masterTableBody = document.querySelector('#master-demo-table-body');
    if (masterTableBody) masterTableBody.innerHTML = `<tr style="text-align:center"><td colspan="8">${STRINGS.MAIN.SCANNING_PLEASE_WAIT_ROW}</td></tr>`;

    try {
      // Demos already queued and unchanged on disk are skipped, not
      // re-parsed; ones from an older project (no file_key, no map_name:
      // saved before clip names, #441, no teams: #445, or no map list: #217)
      // are scanned.
      const known = getDemos()
        .filter((d) => d.file_key && d.map_name !== undefined && demoHasTeams(d) && d.signon_maps !== undefined)
        .map((d) => ({ path: d.path, file_key: d.file_key }));
      const { demos: scanned, unchanged, copies: unparsedCopies = [] } = await scanDirectory(pathsToScan, known, readScanWorkers());
      // An identical copy under another name would be a second row for the
      // same demo, capturing every highlight twice (#21). The scan skips them
      // by key before parsing (`unparsedCopies`, each naming the queued or
      // scanned demo it copies); the split below is a fallback for any that
      // reach here parsed.
      const { keep: newlyScanned, copies } = splitIdenticalCopies(getDemos(), scanned);
      unparsedCopies.forEach((c) => {
        const sameAs = c.queued
          ? getDemos().find((d) => samePath(d.path, c.same_as))
          : newlyScanned.find((d) => samePath(d.path, c.same_as));
        if (sameAs) copies.push({ demo: { path: c.path, name: fileNameOf(c.path) }, sameAs, queued: Boolean(c.queued) });
      });
      // The frontend fallback's copies are all of queued demos.
      copies.forEach((c) => { if (c.queued === undefined) c.queued = getDemos().includes(c.sameAs); });

      // Merge: replace any existing demo with the same path, append new ones.
      // (Prior behavior replaced the whole master list with the result of
      // re-scanning everything in `scanPaths`, which is what caused a single
      // "Add Demo Files" click to report hundreds of demos.)
      const indexByPath = new Map(getDemos().map((d, i) => [d.path, i]));
      newlyScanned.forEach((demo) => {
        const existingIdx = indexByPath.get(demo.path);
        if (existingIdx !== undefined) {
          // A re-scan produces brand new streak objects, so replacing
          // outright would wipe every status, selection, note and Kill
          // Range edit on this demo — carry that user-owned state across by
          // highlight uid instead.
          getDemos()[existingIdx] = preserveHighlightState(getDemos()[existingIdx], demo);
        } else {
          indexByPath.set(demo.path, getDemos().length);
          getDemos().push(demo);
        }
      });

      // footer is also updated on the Complete scan_progress event, but set
      // it here in case the event arrives before renderMasterList finishes.
      updateDemoFooter(getDemos());
      if (newlyScanned.length > 0) markProjectDirty();
      showToast(STRINGS.MAIN.scanCompleteToast(newlyScanned.length, unchanged), 'success');
      const selectedDemoIdx = newlyScanned.length > 0
        ? getDemos().indexOf(newlyScanned[0])
        : (getDemos().length > 0 ? 0 : null);
      setSelectedDemoIdx(selectedDemoIdx);
      // renderMasterList calls this with (null, null) whenever the queue is
      // empty (e.g. after Clear All) — selectDemoAndRenderDetail resets the
      // telemetry UI instead of dereferencing a demo that isn't there.
      renderMasterList(getDemos(), selectedDemoIdx, selectDemoAndRenderDetail);
      if (selectedDemoIdx !== null) {
        selectDemoAndRenderDetail(getDemos()[selectedDemoIdx], selectedDemoIdx);
      }
      // Reads 544 bytes per demo and one map file per distinct map, so it runs
      // after the scan rather than inside it. Not awaited: the queue is already
      // usable, and a demo whose map is missing is still worth listing.
      refreshMapWarnings(
        newlyScanned.map((d) => d.path),
        document.querySelector('#hl-path-input')?.value?.trim() || ''
      );
      refreshTeamsPane();
      if (copies.length > 0) await offerIdenticalCopies(copies, pickedFiles);
      return true;
    } catch (err) {
      console.error("Error scanning directories:", err);
      showToast(STRINGS.MAIN.scanErrorToast(err), 'error');
      if (scanStatusEl) scanStatusEl.textContent = STRINGS.MAIN.scanErrorStatus(err);
      return false;
    } finally {
      if (addFilesBtn) addFilesBtn.disabled = false;
      if (addFolderBtn) addFolderBtn.disabled = false;
      if (cancelScanBtnInner) cancelScanBtnInner.disabled = true;
      if (scanSpinnerEl) scanSpinnerEl.style.display = 'none';
    }
  }

  // Native Demo Files Ingestion (+ Add Demo Files)
  const addFilesBtn = document.querySelector('#add-files-btn');
  if (addFilesBtn) {
    addFilesBtn.addEventListener('click', async () => {
      try {
        const selected = await open({
          multiple: true,
          filters: [{ name: STRINGS.MAIN.DEMO_FILES_FILTER_NAME, extensions: ['dem'] }],
          title: STRINGS.MAIN.SELECT_DEMO_FILES_TITLE
        });
        if (selected) {
          const files = Array.isArray(selected) ? selected : [selected];
          // Files aren't remembered in the pinned-folder list (only folders
          // are); the scan itself adds them to the queue and the project.
          await triggerAutoScan(files, { pickedFiles: true });
        }
      } catch (err) {
        console.error("Error opening demo files dialog:", err);
      }
    });
  }

  // Native Demo Folder Ingestion (+ Add Folder)
  const addFolderBtn = document.querySelector('#add-folder-btn');
  if (addFolderBtn) {
    addFolderBtn.addEventListener('click', async () => {
      try {
        const selected = await open({
          directory: true,
          multiple: false,
          title: STRINGS.MAIN.SELECT_DEMO_FOLDER_TITLE
        });
        if (selected) {
          const folder = Array.isArray(selected) ? selected[0] : selected;
          // Always scan (#432): a folder added before, then emptied from the
          // list (bin icon, Clear All), used to be a silent no-op here.
          // scanPaths only decides whether to remember it, and a folder that
          // turned out not to exist is not remembered. The merge in
          // triggerAutoScan replaces demos by path, so a re-scan adds no
          // duplicate rows.
          const scanned = await triggerAutoScan([folder]);
          if (scanned && !getScanPaths().includes(folder)) {
            getScanPaths().push(folder);
            markProjectDirty();
            await persistAppSettings();
          }
        }
      } catch (err) {
        console.error("Error opening demo directory dialog:", err);
      }
    });
  }

  return { triggerAutoScan };
}
