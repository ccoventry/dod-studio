// queue_scan.js
// Scanning demos into the Master Demo Queue: + Add Demo Files, + Add Folder,
// Cancel Scan, the scan_progress status line, and triggerAutoScan, which
// merges a scan's results into the queue. Moved out of main.js (#683), which
// owns the queue, the selection and the pinned-folder list; they're reached
// through the getters and setters below.

import { pickPath } from './pick_path.js';
import { listen } from '@tauri-apps/api/event';
import { scanDirectory, cancelScan, logFrontendEvent } from './ipc_bridge.js';
import { renderMasterList } from './master_pane.js';
import { refreshMapWarnings } from './map_warnings.js';
import { refreshTeamsPane } from './teams_pane.js';
import { renameUidsInTakeIndex } from './take_index.js';
import { scanIsCurrent } from './scan_version.js';
import { applyRescanWork, matchRescanHighlights, rescanReport } from './rescan_carry.js';
import { updateStreakVisuals } from './detail_pane.js';
import { splitIdenticalCopies } from './demo_copies.js';
import { fileNameOf, samePath } from './path_display.js';
import { parseClock, timeLeftText, scanEndLogLine, scanFailedLogLine } from './scan_progress.js';
import { learnFromScan } from './rescan_estimate.js';
import { readLastRun, rememberLastRun } from './progress_view.js';
import { durationTextMs } from './time_format.js';
import { SCAN_STEPS, nest, placed, queueProgress, scanFoldersKey, scanStepView } from './queue_progress.js';
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
 *   pickedFiles) from project_demos.js, rememberRecentFolder(folder) puts a
 *   scanned folder at the top of Recent and saves it (#533),
 *   persistAppSettings(), and regroupMatches({ within })
 *   from queue_match_groups.js, run after a scan so {match} and {half}
 *   know the new demos (#685), as the last step on the scan's bar (#951),
 *   and getTakeIndex() the project's take index,
 *   whose uids follow a rescanned highlight's work (rescan_carry.js).
 */
export function createQueueScan({
  getDemos,
  setSelectedDemoIdx,
  selectDemoAndRenderDetail,
  updateDemoFooter,
  markProjectDirty,
  readScanWorkers,
  offerIdenticalCopies,
  rememberRecentFolder = async () => {},
  persistAppSettings,
  regroupMatches = () => {},
  getTakeIndex = () => ({}),
}) {
  // ── scan_progress event listener (registered once on load) ────────────────
  // Cancel was clicked and the scan hasn't said it stopped yet: the status
  // line says so instead of a toast, so it isn't followed a moment later by
  // the cancelled one (#843).
  let cancelling = false;
  // The running scan: where it draws on the queue's line (queue_progress.js),
  // how many folders the last scan of the same paths listed, and the most
  // this one has listed so far.
  let active = null;
  const progress = queueProgress();
  listen('scan_progress', (event) => {
    const p = event.payload || {};
    const cancelScanBtn = document.querySelector('#cancel-scan-btn');

    // Demos the scan could not read (#23). Only the final event carries it.
    const skipped = Array.isArray(p.skipped) ? p.skipped : [];
    if (skipped.length > 0) {
      console.warn('Scan skipped unreadable demos:', skipped);
      showToast(STRINGS.MAIN.skippedDemosToast(skipped), 'warning', 10000);
    }

    if (p.cancelled || p.status === 'Complete') {
      // The scan's end line says how it went, once its demos are merged.
      cancelling = false;
      if (active) active.skipped = skipped.length;
      if (cancelScanBtn) cancelScanBtn.disabled = true;
      return;
    }
    if (!active) return;
    if (p.listing) active.foldersSeen = Math.max(active.foldersSeen, p.folders ?? 0);
    // While folders are being listed (#843) or their demos' keys read, no
    // parse has started, so the parse clock must not start either.
    const timeLeft = p.listing || p.keying ? '' : timeLeftText(p, scanParses.elapsed(p.scanned, p.cached));
    const view = scanStepView(p, { expectedFolders: active.expectedFolders, timeLeft });
    if (!view) return;
    const shown = placed(nest(active.place, SCAN_STEPS[view.step]), view);
    if (cancelling) {
      progress.show(active.place.job, { ...shown, text: STRINGS.MAIN.SCAN_CANCELLING_STATUS });
    } else {
      progress.show(active.place.job, shown);
      if (cancelScanBtn) cancelScanBtn.disabled = false;
    }
  });

  // ── Cancel Scan button ────────────────────────────────────────────────────
  const cancelScanBtn = document.querySelector('#cancel-scan-btn');
  if (cancelScanBtn) {
    cancelScanBtn.disabled = true;
    cancelScanBtn.addEventListener('click', async () => {
      cancelScanBtn.disabled = true;
      cancelling = true;
      if (active) progress.show(active.place.job, { text: STRINGS.MAIN.SCAN_CANCELLING_STATUS });
      try {
        await cancelScan();
      } catch (_) {
        /* already toasted in ipc_bridge */
        cancelling = false;
      }
    });
  }

  // "Include subfolders" (#531), saved with the settings when changed.
  const subfoldersBox = document.querySelector('#scan-include-subfolders');
  const includeSubfolders = () => subfoldersBox?.checked ?? true;
  subfoldersBox?.addEventListener('change', () => persistAppSettings());

  // A scan is running: the buttons are disabled, and a drop waits (#530).
  let scanning = false;

  // Cancel stopped the last scan before it added anything (e.g. while it
  // was still listing a whole drive's folders, #843).
  let lastScanAddedNothingCancelled = false;

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
  // `within`: the scan is a step of a bigger job on the queue's line (a
  // split's, queue_split.js), as a queue_progress.js place; it draws there
  // and leaves the job's end to its owner. Otherwise it is its own job.
  async function triggerAutoScan(pathsToScan, { pickedFiles = false, within = null } = {}) {
    if (!pathsToScan || pathsToScan.length === 0) return false;
    scanning = true;
    lastScanAddedNothingCancelled = false;
    cancelling = false;

    const addFilesBtn = document.querySelector('#add-files-btn');
    const addFolderBtn = document.querySelector('#add-folder-btn');
    const cancelScanBtnInner = document.querySelector('#cancel-scan-btn');

    if (addFilesBtn) addFilesBtn.disabled = true;
    if (addFolderBtn) addFolderBtn.disabled = true;
    if (cancelScanBtnInner) cancelScanBtnInner.disabled = false;
    // On the line the moment it starts, before the backend says anything:
    // a whole drive takes a while to list (#951).
    const job = within ? null : progress.begin();
    const place = within ?? { job, range: [0, 100] };
    progress.show(place.job, placed(nest(place, SCAN_STEPS.looking), {
      text: STRINGS.MAIN.SCAN_STARTING, pct: 0, indeterminate: true,
    }));
    const foldersKey = scanFoldersKey(pathsToScan, includeSubfolders());
    active = { place, expectedFolders: readLastRun(foldersKey), foldersSeen: 0, skipped: 0 };
    // The line the scan ends on; `finally` puts it up if the scan didn't.
    let endLine = null;
    scanParses.reset();
    const startedAt = Date.now();
    logFrontendEvent(STRINGS.SCAN_PROGRESS.startLog(pathsToScan.length));

    const masterTableBody = document.querySelector('#master-demo-table-body');
    if (masterTableBody) masterTableBody.innerHTML = `<tr style="text-align:center"><td colspan="7">${STRINGS.MAIN.SCANNING_PLEASE_WAIT_ROW}</td></tr>`;

    try {
      // Demos already queued and unchanged on disk are skipped, not
      // re-parsed; ones an older Studio scanned (older highlight detection,
      // or a field a scan now fills missing: scan_version.js) are scanned.
      const known = getDemos()
        .filter(scanIsCurrent)
        .map((d) => ({ path: d.path, file_key: d.file_key }));
      const outcome = await scanDirectory(pathsToScan, known, readScanWorkers(), includeSubfolders());
      const {
        demos: scanned, unchanged, copies: unparsedCopies = [], cancelled = false,
      } = outcome;
      lastScanAddedNothingCancelled = cancelled && scanned.length === 0;
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
      const rescans = [];
      newlyScanned.forEach((demo) => {
        const existingIdx = indexByPath.get(demo.path);
        if (existingIdx !== undefined) {
          // A re-scan produces brand new streak objects, so replacing
          // outright would wipe every status, mark, note and Kill Range
          // edit on this demo: carry that work across (rescan_carry.js).
          const previous = getDemos()[existingIdx];
          const result = matchRescanHighlights(previous, demo);
          renameUidsInTakeIndex(getTakeIndex(), applyRescanWork(previous, result));
          result.carried.forEach((c) => { if (!c.exact) updateStreakVisuals(c.to); });
          rescans.push({ previous, result });
          getDemos()[existingIdx] = demo;
        } else {
          indexByPath.set(demo.path, getDemos().length);
          getDemos().push(demo);
        }
      });
      logFrontendEvent(scanEndLogLine({
        cancelled,
        ms: Date.now() - startedAt,
        read: scanned.length,
        unchanged,
        outcome,
      }));
      // This PC's full-read rate, for the rescan prompt's estimate (#937).
      const rateLog = learnFromScan(outcome);
      if (rateLog) {
        logFrontendEvent(rateLog);
        persistAppSettings();
      }
      const carryReport = rescanReport(rescans);
      carryReport.log.forEach((line) => logFrontendEvent(line));

      // footer is also updated on the Complete scan_progress event, but set
      // it here in case the event arrives before renderMasterList finishes.
      updateDemoFooter(getDemos());
      if (newlyScanned.length > 0) markProjectDirty();
      if (cancelled) showToast(STRINGS.MAIN.scanCancelledToast(newlyScanned.length), 'info');
      else showToast(STRINGS.MAIN.scanCompleteToast(newlyScanned.length, unchanged), 'success');
      if (carryReport.toast) showToast(carryReport.toast.text, carryReport.toast.type, 10000);
      // The backend wrote the activity log's line (#531).
      if (outcome.cap?.depth || outcome.cap?.entries) showToast(STRINGS.MAIN.scanCapToast(outcome.cap), 'warning', 12000);
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
      // Which match and half each demo is (#685), from the analyzer cache
      // the scan just warmed: the scan's last step, on its bar (#951). It
      // takes a moment, more when the queue is grouped by match and some
      // queued demos aren't cached yet.
      if (newlyScanned.length > 0) await regroupMatches({ within: nest(place, SCAN_STEPS.grouping) });
      if (!cancelled) rememberLastRun(foldersKey, active.foldersSeen);
      const seconds = durationTextMs(Date.now() - startedAt);
      const added = newlyScanned.length - rescans.length;
      endLine = (cancelled
        ? STRINGS.MAIN.scanCancelledLine(seconds, newlyScanned.length)
        : STRINGS.MAIN.scanDoneLine(seconds, { added, rescanned: rescans.length, unchanged }))
        + STRINGS.MAIN.skippedStatusSuffix(active.skipped);
      progress.end(job, endLine);
      if (copies.length > 0) await offerIdenticalCopies(copies, pickedFiles);
      return true;
    } catch (err) {
      console.error("Error scanning directories:", err);
      logFrontendEvent(scanFailedLogLine(Date.now() - startedAt, err));
      showToast(STRINGS.MAIN.scanErrorToast(err), 'error');
      endLine = STRINGS.MAIN.scanErrorStatus(err);
      return false;
    } finally {
      scanning = false;
      if (addFilesBtn) addFilesBtn.disabled = false;
      if (addFolderBtn) addFolderBtn.disabled = false;
      if (cancelScanBtnInner) cancelScanBtnInner.disabled = true;
      if (active?.place === place) active = null;
      progress.end(job, endLine);
    }
  }

  // Native Demo Files Ingestion (+ Add Demo Files)
  const addFilesBtn = document.querySelector('#add-files-btn');
  if (addFilesBtn) {
    addFilesBtn.addEventListener('click', async () => {
      try {
        const selected = await pickPath({
          field: 'queue.add_files',
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

  // Shared by + Add Folder and the Setup window's demo step (#599).
  async function addDemoFolder(folder) {
    // Always scan (#432): a folder added before, then emptied from the
    // list (bin icon, Clear All), used to be a silent no-op here. The merge
    // in triggerAutoScan replaces demos by path, so a re-scan adds no
    // duplicate rows. The folder goes into Recent, not the pinned list
    // (#533): pinning is the user's own choice. A folder that turned out
    // not to exist, or a scan cancelled before it added anything (a drive
    // root the user gave up on), is not remembered.
    const scanned = await triggerAutoScan([folder]);
    if (scanned && !lastScanAddedNothingCancelled) await rememberRecentFolder(folder);
  }

  // Native Demo Folder Ingestion (+ Add Folder)
  const addFolderBtn = document.querySelector('#add-folder-btn');
  if (addFolderBtn) {
    addFolderBtn.addEventListener('click', async () => {
      try {
        const selected = await pickPath({
          field: 'queue.add_folder',
          directory: true,
          multiple: false,
          title: STRINGS.MAIN.SELECT_DEMO_FOLDER_TITLE
        });
        if (selected) await addDemoFolder(Array.isArray(selected) ? selected[0] : selected);
      } catch (err) {
        console.error("Error opening demo directory dialog:", err);
      }
    });
  }

  // Demos and folders dropped from Windows (#530): one scan, as + Add Demo
  // Files and + Add Folder would. Dropped files count as picked by hand, so
  // a copy of a queued demo is offered in its place (#477); the folders go
  // into Recent like + Add Folder's.
  async function addDroppedPaths(files, folders) {
    const scanned = await triggerAutoScan([...files, ...folders], { pickedFiles: files.length > 0 });
    if (!scanned || lastScanAddedNothingCancelled) return;
    // Last to first, so the first folder dropped ends up on top of Recent.
    for (const folder of [...folders].reverse()) await rememberRecentFolder(folder);
  }

  return { triggerAutoScan, addDemoFolder, addDroppedPaths, isScanning: () => scanning };
}
