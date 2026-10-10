import { open } from '@tauri-apps/plugin-dialog';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  calculateExportPoolSpace,
  getSettings,
  openActivityLog,
  systemMemoryBytes
} from './ipc_bridge.js';
import { createQueueSplit } from './queue_split.js';
import { renderMasterList, initMasterPane } from './master_pane.js';
import { initMapWarnings, resetMapWarnings } from './map_warnings.js';
import { initRollFloors } from './roll_floors.js';

import { renderDetailView, initDetailPane } from './detail_pane.js';
import { initCaptureUI, getCommandsState, hydrateCommandsState, applyCommandsState, refreshLaunchGuard, refreshInitCommandWarnings, isCaptureRunning } from './capture_pane.js';
import { confirmCloseDuringBatch } from './batch_close_prompt.js';
import { initRenderUI, checkRenderRecoveryOnStartup, finishedRenderOutputs } from './render_pane.js';
import { initFinishClips } from './finish_clips.js';
import { initAuditorPane } from './auditor_pane.js';
import { refreshPacketEntityLimit } from './packet_entity_limit.js';
import { initAuditorTabs } from './auditor_tabs.js';
import { initSplitPane } from './split_pane.js';
import { initCombineClips } from './combine_clips.js';
import { initDemoRenamePane } from './demo_rename_ui.js';
import { initThemedConfirm } from './themed_confirm.js';
import { initAnalyzerPane } from './analyzer_pane.js';
import { initHdPane } from './hd_pane.js';
import { initBlenderPane } from './blender_pane.js';
import { initOverviewsPane } from './overviews_pane.js';
import { switchNavTab, setCaptureDetailSubtab } from './nav.js';
import { createListEditor } from './list_editor.js';
import { emptyProjectTeams } from './project_teams.js';
import { initTeamsPane } from './teams_pane.js';
import { getCheckedDemoPaths, clearCheckedPaths, recordingPlayerStreaks } from './master_pane.js';
import { initErrorReporter } from './error_reporter.js';
import { STRINGS } from './strings.js';
import { applyStaticStrings } from './apply_strings.js';
import { initInfoTooltips } from './info_tooltip.js';
import { initOsNotifications, updateNotificationSettings } from './os_notifications.js';
import { initUpdater, checkForUpdatesNow, isLocalOrDebugBuild } from './updater_pane.js';
import { initAppMenu } from './app_menu.js';
import { numberField } from './number_field.js';
import { initClipNameSettings, getClipNameTemplate, refreshClipNamePreview } from './clip_name_ui.js';
import { clipNamesForTakes, maxNameLength } from './clip_name.js';
import { initCommandProfiles, setCommandProfiles } from './command_profiles_ui.js';
import { initRenderPresets } from './render_presets_ui.js';
import { pinnedFoldersOnly } from './project_paths.js';
import { createProjectDemos } from './project_demos.js';
import { createQueueScan } from './queue_scan.js';
import { createProjectSession } from './project_session.js';
import { createSettingsSync, applySettingsToForm } from './settings_sync.js';
import { checkObsOrphanOnStartup, currentCaptureMode, applyCaptureModeUI, refreshHlaeFfmpegStatus, initConfigWiring } from './config_wiring.js';
import { createQueueActions } from './queue_actions.js';
import { initReviewMode } from './review_mode.js';

// Registered at module load, before DOMContentLoaded — so it's catching
// from the earliest possible moment, not just once the app's own init
// logic gets around to it.
initErrorReporter();

window.addEventListener("DOMContentLoaded", async () => {
  // Applies every [data-str]/[data-str-title]/[data-str-placeholder]/
  // [data-str-aria-label] element's text/attribute from STRINGS before any
  // other DOM-dependent init runs below.
  applyStaticStrings();
  initInfoTooltips();

  // Not awaited: the permission prompt (first run only) shouldn't block the
  // rest of startup, and every call site in os_notifications.js already
  // no-ops silently until permission is granted.
  initOsNotifications();

  let scanPaths = [];
  // Set when the saved pinned list held single demo files (older builds), so
  // the cleaned list is written back once after settings load.
  let pinnedListCleaned = false;
  // Analyzer Explorer sidebar's "Recent" quick-links tier — most-recent-first,
  // capped at 10, pushed via recordDemoFolderVisit() below whenever browsing
  // into a folder yields a non-empty demo listing. Mirrors dev's
  // `settings.demo_folder_history` (see docs/archive/tauri_parity_audit.md Area 3).
  let demoFolderHistory = [];
  // Gates the Analyzer Explorer tree's per-subfolder demo-count badge.
  // Mirrors dev's `settings.scan_folders_for_demos`, default false.
  let scanFoldersForDemos = false;
  // Analyzer Explorer sidebar's drag-to-resize width (analyzer_pane.js).
  let analyzerExplorerWidth = 260;
  // Doubles as Render Studio's scan-input locations — see initRenderUI below.
  let targetDrives = [];
  let renderExportDirs = []; // JIT multi-drive export pool for Render Studio
  let currentScannedDemos = [];
  let selectedDemoIdx = null;
  // take_key -> uid[]. Recorded by capture_pane.js when a batch verifies a
  // block on disk; resolved by render_pane.js when that take finishes
  // rendering, so status can auto-advance even after a restart or re-scan
  // replaced the original streak objects. Persisted in the project file.
  let takeIndex = {};
  // The Teams list's user-owned half (#445): display names and merges, keyed
  // on the detected tag (project_teams.js). Project state rather than demo
  // state, so a re-scan never touches it. Persisted in the project file.
  let projectTeams = emptyProjectTeams();
  // Save / Load / New Session, the unsaved-changes prompt and the header's
  // session indicator: project_session.js. checkMissingDemos is declared
  // further down this scope, so it's reached through a closure.
  const {
    markProjectDirty,
    saveProjectSession,
    needsSavePrompt,
    requestUnsavedChangesConfirmation,
  } = createProjectSession({
    getDemos: () => currentScannedDemos,
    replaceDemos: (demos) => replaceScannedDemos(demos),
    loadDemos: (demos) => {
      currentScannedDemos = demos;
      selectedDemoIdx = currentScannedDemos.length > 0 ? 0 : null;
      renderMasterList(currentScannedDemos, selectedDemoIdx, selectDemoAndRenderDetail);
      if (currentScannedDemos.length > 0) {
        selectDemoAndRenderDetail(currentScannedDemos[0], selectedDemoIdx);
      }
      updateDemoFooter(currentScannedDemos);
    },
    getTakeIndex: () => takeIndex,
    setTakeIndex: (index) => { takeIndex = index; },
    getProjectTeams: () => projectTeams,
    setProjectTeams: (teams) => { projectTeams = teams; },
    checkMissingDemos: (projectPath, savedScanPaths) => checkMissingDemos(projectPath, savedScanPaths),
  });

  // Initialize modular UI panes
  initThemedConfirm();
  initAuditorPane();
  initAuditorTabs();
  initSplitPane();
  initDemoRenamePane({
    projectTeams: () => projectTeams,
    onChange: () => persistAppSettings(),
  });
  initHdPane();
  initBlenderPane();
  initOverviewsPane();
  initTeamsPane({
    getDemos: () => currentScannedDemos,
    getProjectTeams: () => projectTeams,
    onChange: markProjectDirty,
    // triggerAutoScan is declared further down (queue_scan.js).
    onReadMissing: (paths) => triggerAutoScan(paths),
  });

  async function pickTargetDrive() {
    try {
      return await open({ directory: true, multiple: false, title: STRINGS.MAIN.SELECT_CAPTURE_OUTPUT_DIR_TITLE });
    } catch (err) {
      console.error("Error opening capture output directory dialog:", err);
      return null;
    }
  }

  async function pickRenderExportDir() {
    try {
      return await open({ directory: true, multiple: false, title: STRINGS.MAIN.SELECT_RENDER_EXPORT_DIR_TITLE });
    } catch (err) {
      console.error("Error opening render export directory dialog:", err);
      return null;
    }
  }

  // Shared editable-list widget (list_editor.js) for the two folder/drive
  // pools — Capture Locations (doubles as Render Studio's scan input) and
  // Render Studio's Export Drives — get add/edit/remove/reorder/browse from
  // one implementation.
  const driveOverridesEditor = createListEditor({
    container: document.querySelector('#target-drive-list'),
    getItems: () => targetDrives,
    fields: [{ key: 'value', type: 'text', primitive: true, placeholder: STRINGS.CAPTURE_CONFIG.OUTPUT_DIR_PLACEHOLDER }],
    unique: true,
    browse: pickTargetDrive,
    onChange: () => {
      updateExportPoolIndicator();
      persistAppSettings();
      refreshLaunchGuard({ targetDrives, currentScannedDemos });
    },
  });

  const renderExportDirsEditor = createListEditor({
    container: document.querySelector('#render-export-dir-list'),
    getItems: () => renderExportDirs,
    fields: [{ key: 'value', type: 'text', primitive: true, placeholder: STRINGS.RENDER.EXPORT_DIR_ROW_PLACEHOLDER }],
    unique: true,
    browse: pickRenderExportDir,
    onChange: () => persistAppSettings(),
  });

  // Demo scan workers (#246): 1..8, default 2 (SCAN_CONCURRENCY). Each one
  // holds a whole analysis, so the hint puts the memory next to the number.
  // Deliberately no clamp to the machine's RAM.
  function readScanWorkers() {
    return Math.min(8, Math.max(1, numberField('#config-scan-workers', 2, { integer: true, positive: true })));
  }
  let totalMemoryGb = null;
  function updateScanWorkersHint() {
    const hint = document.querySelector('#config-scan-workers-hint');
    if (hint) hint.textContent = STRINGS.CAPTURE_CONFIG.scanWorkersHint(totalMemoryGb);
  }
  systemMemoryBytes().then((bytes) => {
    if (bytes) totalMemoryGb = Math.round(bytes / 1024 ** 3);
    updateScanWorkersHint();
  });
  const scanWorkersInput = document.querySelector('#config-scan-workers');
  scanWorkersInput?.addEventListener('change', () => {
    scanWorkersInput.value = readScanWorkers();
    persistAppSettings();
  });

  // Saving the settings file from the form: settings_sync.js.
  const { persistAppSettings } = createSettingsSync({
    getState: () => ({
      scanPaths,
      demoFolderHistory,
      scanFoldersForDemos,
      analyzerExplorerWidth,
      targetDrives,
      renderExportDirs,
      scanWorkers: readScanWorkers(),
    }),
    currentCaptureMode,
  });

  // Load persistent settings on startup
  let settings = null;
  try {
    settings = await getSettings();
    if (settings) {
      updateNotificationSettings(settings);
      applySettingsToForm(settings, { refreshHlaeFfmpegStatus, applyCaptureModeUI });
      if (Array.isArray(settings.pinned_folders) && settings.pinned_folders.length > 0) {
        // Folders only: older builds added every file picked with
        // + Add Demo Files, one path per demo. Cleaned up once, here.
        scanPaths = pinnedFoldersOnly(settings.pinned_folders);
        if (scanPaths.length !== settings.pinned_folders.length) pinnedListCleaned = true;
      }
      if (Array.isArray(settings.demo_folder_history) && settings.demo_folder_history.length > 0) {
        demoFolderHistory = [...settings.demo_folder_history];
      }
      scanFoldersForDemos = !!settings.scan_folders_for_demos;
      if (settings.analyzer_explorer_width) {
        analyzerExplorerWidth = settings.analyzer_explorer_width;
      }
      if (Array.isArray(settings.target_drives) && settings.target_drives.length > 0) {
        targetDrives = [...settings.target_drives];
        driveOverridesEditor.render();
        updateExportPoolIndicator();
      }
      if (Array.isArray(settings.render_export_dirs) && settings.render_export_dirs.length > 0) {
        renderExportDirs = [...settings.render_export_dirs];
        renderExportDirsEditor.render();
      }
      hydrateCommandsState(settings.init_commands, settings.custom_commands);
      setCommandProfiles(settings.command_profiles, settings.command_profile_active);
      // Both halves of the question are now in the DOM: the game path, and the
      // commands that will run against whatever its configs set.
      refreshInitCommandWarnings();
    }
  } catch (err) {
    console.error("Error loading startup settings:", err);
  }
  // Wires the Updates tab's buttons regardless of whether settings loaded —
  // only the startup auto-check itself is conditional on settings being
  // present. Not awaited: a background check shouldn't block startup.
  initUpdater(settings, persistAppSettings);
  initAppMenu();
  if (pinnedListCleaned) persistAppSettings();

  const viewLogsBtn = document.querySelector('#view-logs-btn');
  if (viewLogsBtn) {
    viewLogsBtn.addEventListener('click', () => openActivityLog());
  }

  function refreshAfterRelocation(changed) {
    renderMasterList(currentScannedDemos, selectedDemoIdx, selectDemoAndRenderDetail);
    if (!changed) return;
    if (selectedDemoIdx != null && currentScannedDemos[selectedDemoIdx]) {
      selectDemoAndRenderDetail(currentScannedDemos[selectedDemoIdx], selectedDemoIdx);
    }
    markProjectDirty();
  }

  // Missing, moved, changed and copied demos (#21): project_demos.js.
  const {
    checkMissingDemos,
    pickedDemosPresent,
    useFoundCopies,
    offerIdenticalCopies,
    locateDemoByHand,
  } = createProjectDemos({
    getDemos: () => currentScannedDemos,
    getTakeIndex: () => takeIndex,
    getScanPaths: () => scanPaths,
    refreshQueue: refreshAfterRelocation,
    scan: (paths, opts) => triggerAutoScan(paths, opts),
    removeDemo: (demo) => replaceScannedDemos(currentScannedDemos.filter((d) => d !== demo)),
  });

  // Demos that recorded more than one map (#217): queue_split.js.
  const { splitQueuedDemo, captureDemosReady } = createQueueSplit({
    getDemos: () => currentScannedDemos,
    removeDemo: (demo) => replaceScannedDemos(currentScannedDemos.filter((d) => d !== demo)),
    scan: (paths, opts) => triggerAutoScan(paths, opts),
    confirmTracked: (demo) => requestTrackedClearConfirmation([demo], { title: STRINGS.MAIN.SPLIT_TRACKED_DEMO_TITLE, verb: STRINGS.MAIN.VERB_SPLITS, confirmLabel: STRINGS.MAIN.SPLIT_ANYWAY }),
    pickedDemosPresent,
  });

  // Configuration's paths, capture mode, OBS and HLAE FFmpeg: config_wiring.js.
  initConfigWiring({ persistAppSettings });

  // ── Shared "a demo row was selected" handler ────────────────────────────────
  // Extracted so every renderMasterList call site behaves identically —
  // renderMasterList persists whichever callback it was last given
  // (master_pane.js's currentOnSelectDemo) as the fallback for re-renders
  // that don't pass one, so an inline callback that drifted from this one
  // would apply inconsistently depending on which code path rendered last.
  function selectDemoAndRenderDetail(demo, idx) {
    selectedDemoIdx = idx;
    renderDetailView(demo, selectedDemoIdx);
  }

  // ── Demo list footer helper ────────────────────────────────────────────────
  function updateDemoFooter(demos) {
    const footerEl = document.querySelector('#demo-list-footer');
    if (footerEl) {
      // Same recording-player filter the Highlights column uses (M2) — this
      // used to sum every player's streaks in the demo, not just the
      // recording player's, and could read in the hundreds where the
      // visible Highlights column summed to a fraction of that.
      const totalHighlights = (demos || []).reduce((sum, d) => sum + recordingPlayerStreaks(d).length, 0);
      footerEl.textContent = STRINGS.WORKSPACE.demoListFooter((demos || []).length, totalHighlights);
    }
    // Clear Untracked/Clear All only make sense with something in the
    // queue — Clear Selected already gates on its own checkbox state
    // (master_pane.js), this is the same idea for the other two.
    const isEmpty = !demos || demos.length === 0;
    const clearUntrackedBtnEl = document.querySelector('#clear-untracked-btn');
    if (clearUntrackedBtnEl) clearUntrackedBtnEl.disabled = isEmpty;
    const clearAllBtnEl = document.querySelector('#clear-all-btn');
    if (clearAllBtnEl) clearAllBtnEl.disabled = isEmpty;
  }

  // Add Demo Files / Add Folder, Cancel Scan and the scan itself:
  // queue_scan.js. Declared before anything can start a scan; the earlier
  // callers above reach triggerAutoScan through closures.
  const { triggerAutoScan } = createQueueScan({
    getDemos: () => currentScannedDemos,
    setSelectedDemoIdx: (idx) => { selectedDemoIdx = idx; },
    selectDemoAndRenderDetail: (demo, idx) => selectDemoAndRenderDetail(demo, idx),
    updateDemoFooter: (demos) => updateDemoFooter(demos),
    markProjectDirty: () => markProjectDirty(),
    readScanWorkers: () => readScanWorkers(),
    offerIdenticalCopies: (copies, pickedFiles) => offerIdenticalCopies(copies, pickedFiles),
    getScanPaths: () => scanPaths,
    persistAppSettings: () => persistAppSettings(),
  });

  async function updateExportPoolIndicator() {
    const indicator = document.querySelector('#export-pool-free-indicator');
    if (!indicator) return;
    if (targetDrives.length === 0) {
      indicator.textContent = STRINGS.MAIN.EXPORT_POOL_FREE_DEFAULT;
      return;
    }
    try {
      const bytes = await calculateExportPoolSpace(targetDrives);
      const gb = bytes / (1024 * 1024 * 1024);
      indicator.textContent = STRINGS.MAIN.exportPoolFree(gb.toFixed(1));
    } catch (err) {
      console.error("Error calculating export pool space:", err);
      indicator.textContent = STRINGS.MAIN.EXPORT_POOL_ERROR;
    }
  }

  // Target drives management
  document.querySelector('#add-drive-btn').addEventListener('click', () => {
    const driveEl = document.querySelector('#drive-path-input');
    const drivePath = driveEl.value.trim();
    if (drivePath && driveOverridesEditor.addItem(drivePath)) {
      driveEl.value = "";
    }
  });

  const browseDriveBtn = document.querySelector('#browse-drive-btn');
  if (browseDriveBtn) {
    browseDriveBtn.addEventListener('click', async () => {
      const selected = await pickTargetDrive();
      if (selected) driveOverridesEditor.addItem(selected);
    });
  }

  // JIT multi-drive export pool for Render Studio
  const addRenderExportDirBtn = document.querySelector('#add-render-export-dir-btn');
  if (addRenderExportDirBtn) {
    addRenderExportDirBtn.addEventListener('click', () => {
      const inputEl = document.querySelector('#render-export-dir-input');
      const path = inputEl?.value?.trim();
      if (path && renderExportDirsEditor.addItem(path) && inputEl) {
        inputEl.value = '';
      }
    });
  }

  const browseRenderExportBtn = document.querySelector('#browse-render-export-btn');
  if (browseRenderExportBtn) {
    browseRenderExportBtn.addEventListener('click', async () => {
      const selected = await pickRenderExportDir();
      if (selected) renderExportDirsEditor.addItem(selected);
    });
  }

  // Export Configuration tab switching
  const tabBtns = document.querySelectorAll('.config-tab-btn');
  tabBtns.forEach(btn => {
    btn.addEventListener('click', () => {
      tabBtns.forEach(b => b.classList.remove('active'));
      document.querySelectorAll('.config-tab-content').forEach(content => {
        content.style.display = 'none';
      });
      btn.classList.add('active');
      const targetId = btn.getAttribute('data-tab');
      const targetEl = document.getElementById(targetId);
      if (targetEl) targetEl.style.display = 'block';
      refreshCommandWarningsIfShown();
    });
  });

  // The game's config files change outside the app: the read-only flag on
  // config.cfg (#478), or lines in config.cfg/movie.cfg. The Commands tab's
  // warnings are only as fresh as their last check, so it runs again whenever
  // that tab comes into view: opening it, returning to Configuration on it,
  // or coming back to this window (from Explorer or an editor) while it shows.
  function refreshCommandWarningsIfShown() {
    const tab = document.getElementById('tab-custom-commands');
    if (tab && tab.offsetParent !== null) refreshInitCommandWarnings();
  }
  window.addEventListener('focus', refreshCommandWarningsIfShown);

  // Top nav bar view routing (shared with detail_pane.js — see nav.js)
  switchNavTab('workspace');

  const navTabBtns = document.querySelectorAll('.nav-tab-btn');
  navTabBtns.forEach(btn => {
    btn.addEventListener('click', () => {
      switchNavTab(btn.getAttribute('data-nav'));
      refreshCommandWarningsIfShown();
    });
  });

  // Capture Studio in-workflow phase switch (Highlights <-> Configuration) —
  // replaces the old "Batch Capture Config" top-level nav tab, see nav.js.
  document.querySelectorAll('.capture-detail-subtab-btn').forEach((btn) => {
    btn.addEventListener('click', () => setCaptureDetailSubtab(btn.dataset.captureSubtab));
  });

  // Initialize Capture Batch UI
  // Shared by both auto-status paths (a verified capture, a finished render):
  // both tables read status and neither observes the streak objects on its
  // own, so re-render both — the Master Queue for its Pending/Captured/
  // Rendered counts, and the detail view for the per-row status dropdowns.
  const onHighlightStatusChange = () => {
    markProjectDirty();
    renderMasterList(currentScannedDemos, selectedDemoIdx);
    if (selectedDemoIdx !== null && currentScannedDemos[selectedDemoIdx]) {
      renderDetailView(currentScannedDemos[selectedDemoIdx], selectedDemoIdx);
    }
  };

  initReviewMode({
    getDemos: () => currentScannedDemos,
    getCheckedPaths: getCheckedDemoPaths,
    onChanged: onHighlightStatusChange,
  });

  initCaptureUI(() => ({
    scanPaths,
    targetDrives,
    currentScannedDemos
  }), persistAppSettings, onHighlightStatusChange, () => takeIndex, updateExportPoolIndicator, captureDemosReady);
  initCommandProfiles({ getLists: getCommandsState, applyLists: applyCommandsState, onChange: persistAppSettings });

  // Initialize Render Studio UI. First arg doubles as Render's scan-input
  // locations — see the driveOverridesEditor/targetDrives comment above.
  initRenderPresets({ onChange: persistAppSettings });
  initCombineClips({
    finishedRenders: () => finishedRenderOutputs(),
    ffmpegPath: () => document.querySelector('#ffmpeg-override-path-input')?.value?.trim() || null,
  });
  initRenderUI(() => targetDrives, () => renderExportDirs, persistAppSettings, {
    getTakeIndex: () => takeIndex,
    getAllDemos: () => currentScannedDemos,
    onStatusChange: onHighlightStatusChange
  });

  // "When a batch finishes" (#440): queues a verified batch's takes into the
  // Render tab's own queue. capture_pane.js hands it each verified batch.
  initFinishClips({
    getExportDirs: () => renderExportDirs,
    getClipNames: (exportDirs) => clipNamesForTakes(
      takeIndex, currentScannedDemos, getClipNameTemplate(), { maxLength: maxNameLength(exportDirs) },
    ),
    onSettingsChange: persistAppSettings,
  });

  // Render-batch crash-recovery prompt — checked once on startup, same
  // pattern as dev's StartupState::PendingRenderRecovery. Render is now a
  // subtab of the single 'workspace' nav destination, not its own navKey.
  checkRenderRecoveryOnStartup(() => {
    switchNavTab('workspace');
    setCaptureDetailSubtab('render');
  });

  checkObsOrphanOnStartup();

  // Flush any not-yet-persisted settings edit before the window actually
  // closes. list_editor.js (Init/Custom Commands, numeric fields) only
  // writes to disk on 'change' (blur/Enter), not every keystroke — closing
  // the app while a field still has focus (never blurred) would otherwise
  // silently drop that edit even though it's already reflected in the
  // in-memory state persistAppSettings() reads from (a real, reproducible
  // data-loss case).
  const appWindow = getCurrentWindow();
  appWindow.onCloseRequested(async (event) => {
    event.preventDefault();
    // A running batch first: closing leaves it unwatched (#545).
    if (!(await confirmCloseDuringBatch({ isRunning: isCaptureRunning, isLocalBuild: isLocalOrDebugBuild }))) return;
    // Capture Studio project state (scanned demos, takeIndex, scanPaths)
    // changed since the last save — offer to save, discard, or cancel the
    // close before losing it (project_session.js's needsSavePrompt says
    // when). See markProjectDirty() call sites above.
    if (needsSavePrompt()) {
      const outcome = await requestUnsavedChangesConfirmation();
      if (!outcome) return; // Cancel — leave the window open
      // 'save' already wrote the file inside the modal's Save button
      // handler; 'discard' falls through to close as-is either way.
    }
    await persistAppSettings();
    await appWindow.destroy();
  });

  // Page-level reload (F5, Ctrl+R, or any other in-place navigation) doesn't
  // go through Tauri's onCloseRequested above at all — it's a WebView2
  // navigation, not a window close — so it needs its own guard. Unlike the
  // themed modal above, beforeunload's confirmation dialog is browser-native
  // and cannot be styled or given custom button text (a deliberate web
  // platform restriction against sites faking dialogs); setting returnValue
  // is what triggers it, and its own text is what's shown, not this string.
  window.addEventListener('beforeunload', (event) => {
    if (needsSavePrompt()) {
      event.preventDefault();
      event.returnValue = '';
    }
  });

  // Delete callback: remove a demo from the active scan list and re-render.
  // Called by master_pane.js when the 🗑 button is clicked on a row. Returns
  // the new selectedDemoIdx so the caller's own renderMasterList call can
  // pass it through for the row-highlight — master_pane.js doesn't otherwise
  // know this file's selectedDemoIdx, and previously always re-rendered with
  // no selection at all (visually dropping the highlight) even when the
  // deleted row wasn't the selected one and the selection should have held.
  const onDeleteDemo = (deletedOriginalIdx, updatedDemos) => {
    currentScannedDemos = updatedDemos;
    markProjectDirty();
    // If the deleted demo was the selected one, clear the detail view.
    if (selectedDemoIdx === deletedOriginalIdx) {
      selectedDemoIdx = currentScannedDemos.length > 0 ? 0 : null;
      if (selectedDemoIdx !== null) {
        renderDetailView(currentScannedDemos[0], selectedDemoIdx);
      } else {
        renderDetailView(null, null);
      }
    } else if (selectedDemoIdx !== null && selectedDemoIdx > deletedOriginalIdx) {
      // Shift selection index down if a demo above it was removed.
      selectedDemoIdx -= 1;
    }
    updateDemoFooter(currentScannedDemos);
    return selectedDemoIdx;
  };

  // Shared by all three Clear actions below: swaps the scanned-demo list,
  // resets checkboxes, and re-renders both the queue and the detail view.
  //
  // Selection is preserved by object identity, not just reset to row 0:
  // Clear Untracked/Selected/All can all leave the previously-selected demo
  // still in the queue (it just wasn't the one removed), and jumping the
  // selection to whatever's now first is jarring — you were looking at one
  // demo's highlights and suddenly a different one's are shown. Only falls
  // back to row 0 (or nothing) when the selected demo was actually the one
  // that got removed.
  function replaceScannedDemos(newDemos) {
    const previouslySelectedDemo = selectedDemoIdx !== null ? currentScannedDemos[selectedDemoIdx] : null;
    currentScannedDemos = newDemos;
    markProjectDirty();
    const preservedIdx = previouslySelectedDemo ? currentScannedDemos.indexOf(previouslySelectedDemo) : -1;
    selectedDemoIdx = preservedIdx !== -1 ? preservedIdx : (currentScannedDemos.length > 0 ? 0 : null);
    clearCheckedPaths();
    updateDemoFooter(currentScannedDemos);
    renderMasterList(currentScannedDemos, selectedDemoIdx);
    renderDetailView(selectedDemoIdx !== null ? currentScannedDemos[selectedDemoIdx] : null, selectedDemoIdx);
    // The banner is about demos in the queue. With the queue empty there is
    // nothing left for it to be about.
    if (currentScannedDemos.length === 0) resetMapWarnings();
  }

  // Clear Untracked / Selected / All and the tracked-work modal:
  // queue_actions.js.
  const { requestTrackedClearConfirmation, requestTrackedDeleteConfirm } = createQueueActions({
    getDemos: () => currentScannedDemos,
    replaceDemos: (demos) => replaceScannedDemos(demos),
    saveProjectSession: () => saveProjectSession(),
  });

  initMasterPane(onDeleteDemo, requestTrackedDeleteConfirm, locateDemoByHand, (demo) => useFoundCopies([demo]), (demo, update) => splitQueuedDemo(demo, { update }));
  // Read at click time, not captured: the hl.exe path can be set after a scan
  // has already run and left the banner up.
  initMapWarnings(() => document.querySelector('#hl-path-input')?.value?.trim() || '');
  // capture_pane.js owns the Scheduled Command list, so the floor check reads
  // it from there rather than keeping a second copy.
  initRollFloors(() => getCommandsState().custom_commands);
  // Scanned once the persisted hl.exe path is in the DOM, and again whenever it
  // changes — a config file the app cannot see is exactly what this warns about.
  const hlPathInput = document.querySelector('#hl-path-input');
  if (hlPathInput) {
    refreshInitCommandWarnings();
    hlPathInput.addEventListener('change', () => refreshInitCommandWarnings());
    // Which engine it is decides which demos the Master Queue marks (#207).
    const markDemosOverLimit = () => refreshPacketEntityLimit(hlPathInput.value.trim())
      .then(() => renderMasterList(currentScannedDemos, selectedDemoIdx));
    markDemosOverLimit();
    hlPathInput.addEventListener('change', markDemosOverLimit);
  }
  // #441: the preview uses the selected demo's first checked highlight (its
  // first highlight when none is checked); a settled template change is
  // saved and redraws the automatic names in Highlight Details.
  initClipNameSettings({
    getHighlight: () => {
      const demo = selectedDemoIdx !== null ? currentScannedDemos[selectedDemoIdx] : null;
      const own = demo ? recordingPlayerStreaks(demo) : [];
      const streak = own.find((s) => s.selected) || own[0];
      return streak ? { demo, streak } : null;
    },
    getExportDirList: () => renderExportDirs,
    onChange: () => {
      persistAppSettings();
      if (selectedDemoIdx !== null && currentScannedDemos[selectedDemoIdx]) {
        renderDetailView(currentScannedDemos[selectedDemoIdx], selectedDemoIdx);
      }
    },
  });

  initDetailPane(() => currentScannedDemos, () => {
    // Fired on every detail-pane re-render, not just edits (also runs when
    // switching the selected demo, or after a capture/render completes) —
    // selection moves required capture bytes (refreshLaunchGuard) and status
    // moves the Master Queue's Highlights/Pending/Captured/Rendered columns
    // (renderMasterList), both cheap enough to just always re-derive here.
    // Must NOT mark the project dirty — see onDirty below for that.
    refreshLaunchGuard({ targetDrives, currentScannedDemos });
    renderMasterList(currentScannedDemos, selectedDemoIdx);
    refreshClipNamePreview();
  }, () => {
    // Fired only from an actual highlights-table field edit (selection,
    // kill range, status, notes) — all of it is part of the `demos` written
    // by saveProjectSession(), so all of it marks the project dirty.
    markProjectDirty();
  });

  // Demo Analyzer's Explorer sidebar Pinned tier shares the same
  // pinned_folders/scanPaths state as Capture Studio (matches dev's real
  // design — its own pinned_folders field is app-wide, not analyzer-scoped).
  // Pinning/unpinning here doesn't trigger Capture Studio's heavier
  // highlight-scan pipeline, just persists the path.
  async function pinAnalyzerFolder(folder) {
    if (!scanPaths.includes(folder)) {
      scanPaths.push(folder);
      await persistAppSettings();
    }
  }
  async function unpinAnalyzerFolder(folder) {
    scanPaths = scanPaths.filter((f) => f !== folder);
    await persistAppSettings();
  }
  // Recent tier: most-recent-first, capped at 10, deduped — mirrors dev's
  // `demo_folder_history` push-front/truncate logic exactly.
  async function recordDemoFolderVisit(folder) {
    demoFolderHistory = demoFolderHistory.filter((f) => f !== folder);
    demoFolderHistory.unshift(folder);
    if (demoFolderHistory.length > 10) demoFolderHistory.length = 10;
    await persistAppSettings();
  }
  // A pinned/recent folder that no longer exists on disk is silently
  // dropped from history when clicked, matching dev's Quick Links behavior.
  async function forgetDemoFolderVisit(folder) {
    if (demoFolderHistory.includes(folder)) {
      demoFolderHistory = demoFolderHistory.filter((f) => f !== folder);
      await persistAppSettings();
    }
  }
  async function setScanFoldersForDemos(enabled) {
    scanFoldersForDemos = enabled;
    await persistAppSettings();
  }
  async function setAnalyzerExplorerWidth(px) {
    analyzerExplorerWidth = px;
    await persistAppSettings();
  }
  initAnalyzerPane({
    getPinnedFolders: () => scanPaths,
    pinFolder: pinAnalyzerFolder,
    unpinFolder: unpinAnalyzerFolder,
    getDemoFolderHistory: () => demoFolderHistory,
    recordDemoFolderVisit,
    forgetDemoFolderVisit,
    getScanFoldersForDemos: () => scanFoldersForDemos,
    setScanFoldersForDemos,
    getAnalyzerExplorerWidth: () => analyzerExplorerWidth,
    setAnalyzerExplorerWidth,
  });

  // Context-Aware Shortcut Dispatcher
  window.addEventListener('keydown', (e) => {
    const isCtrlO = (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'o';
    const isCtrlS = (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's';
    const isCtrlN = (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'n';
    const isCtrlW = (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'w';
    // WebView2 keeps the browser's reload shortcuts live by default — a
    // desktop app has no "refresh the page" affordance at all, so these are
    // swallowed unconditionally rather than routed through the dirty-state
    // check the beforeunload listener below does for any other reload path
    // (e.g. devtools once opened — the context menu's own Reload entry is
    // gone entirely, see the contextmenu listener further down).
    const isReload = e.key === 'F5' || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'r');

    if (isCtrlO || isCtrlS || isCtrlN || isCtrlW || isReload) {
      e.preventDefault();
    }

    // New/Save/Load Session work from any tab (#122) — previously gated to
    // `activeTab === 'workspace'` because the buttons themselves only
    // existed in the Studio nav-actions area; now that they live in the
    // always-visible File menu, the shortcuts aren't tab-scoped either.
    if (isCtrlN) document.querySelector('#new-session-btn')?.click();
    if (isCtrlO) document.querySelector('#load-project-btn')?.click();
    if (isCtrlS) document.querySelector('#save-project-btn')?.click();
  });

  // WebView2's native right-click menu (Reload/Inspect/browser Cut-Copy-
  // Paste) reads as a web page, not an app — suppressed entirely. Doesn't
  // affect actual clipboard functionality: Ctrl+C/X/V are OS-level keyboard
  // bindings that don't route through this menu, so text fields keep normal
  // copy/paste with no menu at all.
  window.addEventListener('contextmenu', (e) => e.preventDefault());
});