import { configurePickPath } from './pick_path.js';
import { getSettings, openActivityLog, builtinCommandProfiles } from './ipc_bridge.js';
import { createQueueSplit } from './queue_split.js';
import { renderMasterList, initMasterPane } from './master_pane.js';
import { resetMapWarnings } from './map_warnings.js';
import { initRollFloors } from './roll_floors.js';

import { renderDetailView, initDetailPane, updateStreakVisuals } from './detail_pane.js';
import { initCaptureUI, getCommandsState, hydrateCommandsState, applyCommandsState, refreshLaunchGuard, refreshInitCommandWarnings } from './capture_pane.js';
import { createBatchCatchUp } from './batch_catch_up.js';
import { initRenderUI, checkRenderRecoveryOnStartup, finishedRenderOutputs } from './render_pane.js';
import { initFinishClips } from './finish_clips.js';
import { createObsCaptureTest } from './obs_capture_test.js';
import { initTakeCleanup } from './take_cleanup.js';
import { initRenderWhileCapturing } from './render_while_capturing.js';
import { initAuditorPane } from './auditor_pane.js';
import { initAuditorTabs } from './auditor_tabs.js';
import { initSplitPane } from './split_pane.js';
import { initCombineClips } from './combine_clips.js';
import { initDemoRenamePane } from './demo_rename_ui.js';
import { initThemedConfirm } from './themed_confirm.js';
import { startOpenFromExplorer } from './open_from_explorer.js';
import { initHdPane } from './hd_pane.js';
import { initBlenderPane } from './blender_pane.js';
import { initOverviewsPane } from './overviews_pane.js';
import { switchNavTab, setCaptureDetailSubtab } from './nav.js';
import { emptyProjectTeams } from './project_teams.js';
import { initTeamsPane } from './teams_pane.js';
import { getCheckedDemoPaths, clearCheckedPaths, listedStreaks, getVisibleDemos, getQueueRecorders, queueRecorderOf } from './master_pane.js';
import { initErrorReporter } from './error_reporter.js';
import { applyStaticStrings } from './apply_strings.js';
import { initInfoTooltips } from './info_tooltip.js';
import { initOsNotifications, updateNotificationSettings } from './os_notifications.js';
import { initUpdater } from './updater_pane.js';
import { startWhatsNew } from './whats_new.js';
import { initAppMenu } from './app_menu.js';
import { initClipNameSettings, getClipNameTemplate, refreshClipNamePreview } from './clip_name_ui.js';
import { clipNamesForTakes, maxNameLength } from './clip_name.js';
import { initCommandProfiles, setCommandProfiles } from './command_profiles_ui.js';
import { renderCaptureSummary } from './capture_summary_ui.js';
import { initRenderPresets } from './render_presets_ui.js';
import { createFolderPools } from './folder_pools.js';
import { readScanWorkers, startScanWorkers } from './scan_workers.js';
import { createDemoFolders } from './demo_folders.js';
import { startNavTabs } from './nav_tabs.js';
import { startCloseGuard } from './close_guard.js';
import { readHlPath, startHlPathWatch } from './hl_path_watch.js';
import { renderQueueFooter } from './queue_footer.js';
import { startAppShortcuts } from './app_shortcuts.js';
import { createProjectDemos } from './project_demos.js';
import { createQueueScan } from './queue_scan.js';
import { createProjectRescan } from './project_rescan.js';
import { createRescanRecent } from './rescan_recent.js';
import { createQueueMatchGroups } from './queue_match_groups.js';
import { createQueueGroupBy, isGroupedByMatch } from './queue_group_by.js';
import { createProjectSession } from './project_session.js';
import { createRecentProjects } from './recent_projects.js';
import { createProjectConvert } from './project_convert.js';
import { createSettingsSync, applySettingsToForm } from './settings_sync.js';
import { checkObsOrphanOnStartup, currentCaptureMode, applyCaptureModeUI, refreshHlaeFfmpegStatus, initConfigWiring } from './config_wiring.js';
import { createQueueActions } from './queue_actions.js';
import { initReviewMode } from './review_mode.js';
import { createReviewLibrary } from './review_library.js';
import { createHltvPicker } from './review_hltv_picker.js';
import { createSetup } from './setup_wiring.js';
import { offerPinnedReview } from './pinned_review.js';
import { createDemoDrop } from './demo_drop.js';
import { createPlayersUi } from './players_ui.js';
import { getGameFolder } from './game_folder_ui.js';

// Registered at module load, before DOMContentLoaded, so it catches errors
// from the earliest moment, not only once the app's own init gets to it.
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

  // Pinned folders (Capture Studio's scan paths), the Explorer's Recent tier
  // and its toggles: demo_folders.js.
  const demoFolders = createDemoFolders({ persist: () => persistAppSettings() });
  // Set when the saved pinned list held single demo files (older builds), so
  // the cleaned list is written back once after settings load.
  let pinnedListCleaned = false;
  let currentScannedDemos = [];
  // The Setup window and the Highlights page's checklist (#599): setup_wiring.js,
  // created once everything it reads is wired, near the end of startup.
  let setup = null;
  let savedSetupDismissed = false;
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
  // The Review box's HLTV pick (#828), kept in the project file.
  const hltvPick = createHltvPicker({
    getDemos: () => currentScannedDemos,
    getProjectTeams: () => projectTeams,
    onChange: () => markProjectDirty(),
  });
  // Review marks kept outside the project, for demos a scan adds (#848).
  const reviewLibrary = createReviewLibrary({
    getDemos: () => currentScannedDemos, hasProjectFile: () => !!projectSession.currentPath(),
    onBroughtIn: (streaks) => { streaks.forEach(updateStreakVisuals); refreshAfterRelocation(true); },
  });
  // Save / Load / New Session, the unsaved-changes prompt and the header's
  // session indicator: project_session.js. checkMissingDemos is declared
  // further down this scope, so it's reached through a closure.
  const projectSession = createProjectSession({
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
      // Match and half per demo (#685), from the analyzer cache; declared
      // further down (queue_match_groups.js), reached through the closure.
      regroupMatches();
      reviewLibrary.noteLoaded();
    },
    getTakeIndex: () => takeIndex,
    setTakeIndex: (index) => { takeIndex = index; },
    getProjectTeams: () => projectTeams,
    setProjectTeams: (teams) => { projectTeams = teams; },
    checkMissingDemos: (path, scanPaths, options) => checkMissingDemos(path, scanPaths, undefined, options),
    offerRescan: () => offerRescan(),
    getReviewSettings: () => hltvPick.getState(),
    setReviewSettings: (raw) => hltvPick.setState(raw),
    onProjectFile: (path) => recentProjects.remember(path),
    offerConvert: createProjectConvert({ persist: () => persistAppSettings() }).offerConvert,
    onProjectRenamed: (oldPath, newPath) => recentProjects.rename(oldPath, newPath),
  });
  const { markProjectDirty: markSessionDirty, saveProjectSession, needsSavePrompt, requestUnsavedChangesConfirmation, openProject } = projectSession;
  const markProjectDirty = () => { markSessionDirty(); reviewLibrary.sync(); }; // the library sees every change
  // File → Open Recent (#935), saved in the settings file.
  const recentProjects = createRecentProjects({ openProject, persistAppSettings: () => persistAppSettings() });

  // Initialize modular UI panes
  initThemedConfirm();
  initAuditorPane({ getDemos: () => currentScannedDemos, onSettingsChange: () => persistAppSettings() });
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
    onChange: () => {
      markProjectDirty();
      // A renamed or merged team changes the clip names shown (#445).
      // renderDetailView and refreshClipNamePreview are imported above;
      // selectedDemoIdx is declared further down this scope.
      if (selectedDemoIdx !== null && currentScannedDemos[selectedDemoIdx]) {
        renderDetailView(currentScannedDemos[selectedDemoIdx], selectedDemoIdx);
      }
      refreshClipNamePreview();
    },
    // triggerAutoScan is declared further down (queue_scan.js).
    onReadMissing: (paths) => triggerAutoScan(paths),
  });

  // Capture Locations (also Render Studio's scan input) and Export Drives: folder_pools.js.
  const folderPools = createFolderPools({
    getDemos: () => currentScannedDemos,
    persist: () => persistAppSettings(),
    refreshSetup: () => setup?.refresh(),
  });
  startScanWorkers({ persist: () => persistAppSettings() });

  // Saving the settings file from the form: settings_sync.js.
  const { persistAppSettings } = createSettingsSync({
    getState: () => ({
      ...demoFolders.settingsState(),
      recentProjects: recentProjects.list(),
      targetDrives: folderPools.targetDrives(),
      renderExportDirs: folderPools.renderExportDirs(),
      scanWorkers: readScanWorkers(),
      // Read from the file until setup_wiring.js exists, so a save made
      // earlier in startup keeps it.
      setupDismissed: setup ? setup.isDismissed() : savedSetupDismissed,
    }),
    currentCaptureMode,
  });
  // Where each Browse dialog starts (#936): the game paths, and saving a
  // newly remembered folder.
  configurePickPath({
    getContext: () => ({ hlPath: readHlPath(), gameFolder: getGameFolder() }),
    persist: () => persistAppSettings(),
  });

  // Load persistent settings on startup
  let settings = null;
  try {
    settings = await getSettings();
    if (settings) {
      updateNotificationSettings(settings);
      applySettingsToForm(settings, { refreshHlaeFfmpegStatus, applyCaptureModeUI });
      pinnedListCleaned = demoFolders.load(settings);
      recentProjects.load(settings.recent_projects);
      savedSetupDismissed = !!settings.setup_dismissed;
      folderPools.load(settings);
      hydrateCommandsState(settings.init_commands, settings.custom_commands);
      setCommandProfiles(settings.command_profiles, settings.command_profile_active,
        { builtins: await builtinCommandProfiles(), notice: settings.builtin_profile_notice });
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
  startWhatsNew();
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
    getScanPaths: demoFolders.all,
    refreshQueue: refreshAfterRelocation,
    scan: (paths, opts) => triggerAutoScan(paths, opts),
    removeDemo: (demo) => replaceScannedDemos(currentScannedDemos.filter((d) => d !== demo)),
  });

  // Demos that recorded more than one map (#217): queue_split.js.
  const { splitQueuedDemo, captureDemosReady } = createQueueSplit({
    getDemos: () => currentScannedDemos,
    removeDemo: (demo) => replaceScannedDemos(currentScannedDemos.filter((d) => d !== demo)),
    scan: (paths, opts) => triggerAutoScan(paths, opts),
    getTakeIndex: () => takeIndex,
    refreshQueue: () => refreshAfterRelocation(true),
    pickedDemosPresent,
  });

  // Configuration's paths, capture mode, OBS and HLAE FFmpeg: config_wiring.js.
  initConfigWiring({ persistAppSettings });
  // Configuration > Players (#428): players_ui.js.
  createPlayersUi({
    getDemos: () => currentScannedDemos,
    getHlPath: readHlPath,
    getGameFolder,
    persist: () => persistAppSettings(),
  }).refresh();

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
    renderQueueFooter(demos);
    setup?.refresh();
    // The HLTV pick's lists follow the queue: filled while it was empty they
    // are disabled, and a disabled list never opens to refill itself.
    hltvPick.refresh();
  }

  // Which match and half each queued demo is (#685): queue_match_groups.js.
  // Runs after a scan and a project load. The Group by dropdown (#823):
  // queue_group_by.js; grouped by match, a regroup also reads uncached demos.
  const { regroup: regroupMatches } = createQueueMatchGroups({
    getDemos: () => currentScannedDemos,
    renderQueue: () => renderMasterList(currentScannedDemos, selectedDemoIdx),
    parseMissingByDefault: isGroupedByMatch,
  });
  createQueueGroupBy({
    getDemos: () => currentScannedDemos,
    renderQueue: () => renderMasterList(currentScannedDemos, selectedDemoIdx),
    regroupMatches: (opts) => regroupMatches(opts),
  });

  // Add Demo Files / Add Folder, Cancel Scan and the scan itself:
  // queue_scan.js. Declared before anything can start a scan; the earlier
  // callers above reach triggerAutoScan through closures.
  const { triggerAutoScan, addDemoFolder, addDroppedPaths, isScanning } = createQueueScan({
    getDemos: () => currentScannedDemos,
    setSelectedDemoIdx: (idx) => { selectedDemoIdx = idx; },
    selectDemoAndRenderDetail: (demo, idx) => selectDemoAndRenderDetail(demo, idx),
    updateDemoFooter: (demos) => updateDemoFooter(demos),
    markProjectDirty: () => markProjectDirty(),
    readScanWorkers: () => readScanWorkers(),
    offerIdenticalCopies: (copies, pickedFiles) => offerIdenticalCopies(copies, pickedFiles),
    rememberRecentFolder: (folder) => demoFolders.recordVisit(folder),
    persistAppSettings: () => persistAppSettings(),
    regroupMatches: (opts) => regroupMatches(opts),
    getTakeIndex: () => takeIndex,
  });

  // Demos and folders dropped from Windows (#530): demo_drop.js.
  createDemoDrop({ addDroppedPaths, isScanning });

  // Demos an older DoD Studio scanned, offered for a rescan when a project
  // opens: project_rescan.js.
  const { offerRescan } = createProjectRescan({
    getDemos: () => currentScannedDemos,
    scan: (paths) => triggerAutoScan(paths),
  });
  // After an update, every recent session's outdated demos at once (#938).
  const rescanRecent = createRescanRecent({
    getRecent: () => recentProjects.list(),
    session: projectSession,
    getDemos: () => currentScannedDemos,
    scan: (paths) => triggerAutoScan(paths),
    isScanning: () => isScanning(),
    isSetupOpen: () => !!setup?.isOpen(),
    persistAppSettings: () => persistAppSettings(),
  });

  // Top nav, Configuration tabs and Capture Studio subtabs: nav_tabs.js.
  startNavTabs();

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
    getShownPaths: () => getVisibleDemos().map((d) => d.path),
    getRecorders: getQueueRecorders,
    recorderOf: queueRecorderOf,
    onChanged: onHighlightStatusChange,
    hltvPick,
  });

  initCaptureUI(() => ({
    scanPaths: demoFolders.pinned(),
    targetDrives: folderPools.targetDrives(),
    currentScannedDemos,
    projectPath: projectSession.currentPath(),
  }), persistAppSettings, onHighlightStatusChange, () => takeIndex, folderPools.updateExportPoolIndicator, captureDemosReady);
  createObsCaptureTest({ getState: () => ({ targetDrives: folderPools.targetDrives() }), persist: () => persistAppSettings() });
  // Saving, renaming or deleting a profile changes its name in the capture
  // summary strip without touching a field the strip listens to.
  initCommandProfiles({
    getLists: getCommandsState,
    applyLists: applyCommandsState,
    onChange: () => { persistAppSettings(); renderCaptureSummary(); },
  });

  // Initialize Render Studio UI. First arg doubles as Render's scan-input
  // locations (folder_pools.js).
  initRenderPresets({ onChange: persistAppSettings });
  initCombineClips({
    finishedRenders: () => finishedRenderOutputs(),
    ffmpegPath: () => document.querySelector('#ffmpeg-override-path-input')?.value?.trim() || null,
  });
  initRenderUI(folderPools.targetDrives, folderPools.renderExportDirs, persistAppSettings, {
    getTakeIndex: () => takeIndex,
    getAllDemos: () => currentScannedDemos,
    getProjectTeams: () => projectTeams,
    onStatusChange: onHighlightStatusChange
  });

  // "When a batch finishes" (#440): queues a verified batch's takes into the
  // Render tab's own queue. capture_pane.js hands it each verified batch.
  initFinishClips({
    getExportDirs: folderPools.renderExportDirs,
    getClipNames: (exportDirs) => clipNamesForTakes(
      takeIndex, currentScannedDemos, getClipNameTemplate(),
      { maxLength: maxNameLength(exportDirs), projectTeams },
    ),
    onSettingsChange: persistAppSettings,
  });
  // "After a clip is finished" (#644): the setting, and the backend's reports.
  initTakeCleanup({ onSettingsChange: persistAppSettings });
  // "Render while capturing" (#861): queues each take as the batch records it.
  initRenderWhileCapturing({ onSettingsChange: persistAppSettings });

  // Render-batch crash-recovery prompt — checked once on startup, same
  // pattern as dev's StartupState::PendingRenderRecovery. Render is now a
  // subtab of the single 'workspace' nav destination, not its own navKey.
  checkRenderRecoveryOnStartup(() => {
    switchNavTab('workspace');
    setCaptureDetailSubtab('render');
  });

  checkObsOrphanOnStartup();

  // A batch that ran while Studio was closed (#545): mark its finished clips.
  createBatchCatchUp({ getDemos: () => currentScannedDemos, getTakeIndex: () => takeIndex,
    getSessionPath: projectSession.currentPath, onStatusChange: onHighlightStatusChange }).start();

  // Window close and page reload with a batch running or unsaved work: close_guard.js.
  startCloseGuard({ needsSavePrompt, requestUnsavedChangesConfirmation,
    persistAppSettings: async () => { await reviewLibrary.flush(); await persistAppSettings(); } });

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
  const { requestTrackedDeleteConfirm } = createQueueActions({
    getDemos: () => currentScannedDemos,
    replaceDemos: (demos) => replaceScannedDemos(demos),
    saveProjectSession: () => saveProjectSession(),
  });

  initMasterPane(onDeleteDemo, requestTrackedDeleteConfirm, locateDemoByHand, (demo) => useFoundCopies([demo]), (demo, update) => splitQueuedDemo(demo, { update }));
  // capture_pane.js owns the Scheduled Command list, so the floor check reads
  // it from there rather than keeping a second copy.
  initRollFloors(() => getCommandsState().custom_commands);
  // Map warnings, demo links and the checks that follow the hl.exe path: hl_path_watch.js.
  startHlPathWatch({ renderQueue: () => renderMasterList(currentScannedDemos, selectedDemoIdx) });
  // #441: the preview uses the selected demo's first checked highlight (its
  // first highlight when none is checked); a settled template change is
  // saved and redraws the automatic names in Highlight Details.
  initClipNameSettings({
    getHighlight: () => {
      const demo = selectedDemoIdx !== null ? currentScannedDemos[selectedDemoIdx] : null;
      const own = demo ? listedStreaks(demo) : [];
      const streak = own.find((s) => s.selected) || own[0];
      return streak ? { demo, streak } : null;
    },
    getExportDirList: folderPools.renderExportDirs,
    getProjectTeams: () => projectTeams,
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
    refreshLaunchGuard({ targetDrives: folderPools.targetDrives(), currentScannedDemos });
    renderMasterList(currentScannedDemos, selectedDemoIdx);
    refreshClipNamePreview();
  }, () => {
    // Fired only from an actual highlights-table field edit (selection,
    // kill range, status, notes) — all of it is part of the `demos` written
    // by saveProjectSession(), so all of it marks the project dirty.
    markProjectDirty();
  }, { getProjectTeams: () => projectTeams });

  // The Demo Analyzer's Explorer shares the pinned folders with Capture Studio.
  demoFolders.startAnalyzerPane();

  setup = createSetup({
    getDemos: () => currentScannedDemos,
    getScanPaths: demoFolders.all,
    getTargetDrives: folderPools.targetDrives,
    getRenderExportDirs: folderPools.renderExportDirs,
    captureFolders: folderPools.captureFolders,
    renderFolders: folderPools.renderFolders,
    addDemoFolder: (folder) => addDemoFolder(folder),
    persistAppSettings: () => persistAppSettings(),
    dismissed: savedSetupDismissed,
  });
  // The one-time look at the pins older builds' + Add Folder made (#533).
  setup.start().finally(() => offerPinnedReview({
    getPinned: demoFolders.pinned,
    unpin: demoFolders.unpin,
    isSetupOpen: () => setup.isOpen(),
    persist: () => persistAppSettings(),
  }))
    // One startup prompt at a time: the rescan offer (#938) waits for the pins review.
    .finally(() => rescanRecent.offerAfterUpdate().catch((err) => console.error("Rescan recent sessions:", err)));

  startOpenFromExplorer({ openProject, isScanning, persist: () => persistAppSettings(),
    addDemosToQueue: (paths) => triggerAutoScan(paths, { pickedFiles: true }) });

  // Ctrl+N/O/S, swallowed reload keys, no browser context menu: app_shortcuts.js.
  startAppShortcuts();
});