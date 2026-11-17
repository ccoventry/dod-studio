// strings/shell.js: the app's text for the window frame, menus, updates, notifications and crash toasts.
// Part of the STRINGS catalog that ../strings.js assembles (#604).

export const SHELL_STRINGS = {
  // A sticky toast's close button (toast.js).
  TOAST_DISMISS: 'Dismiss',
  // ── Top Navigation / Header ─────────────────────────────────────────────
  NAV: {
    APP_TITLE: 'DoD Studio',
    // OS window title (taskbar/Alt-Tab) — set by updater_pane.js once the
    // running build's version is known. baseVersion excludes the
    // experimental channel's `-<run number>` suffix — the title just needs
    // "what kind of build is this", not which exact run. buildKind: 'local'
    // (npm run tauri dev), 'debug' (tauri build --debug — a real bundle,
    // just not a release-profile one), 'experimental'
    // (release_experimental.yml), or anything else for a real stable build
    // (no parenthetical).
    // `branch` is set only for a build made on this PC (see localGitBranch):
    // `local build - test/capture-batch`. A release build made here reports
    // 'stable' or 'experimental' by version, but a branch means it came from
    // the repo, so it's labelled a local release build instead.
    // `port` is the Vite dev server's, set only under `npm run tauri dev`, so
    // two dev copies running side by side can be told apart.
    appWindowTitle: (baseVersion, buildKind, branch, port) => {
      const tags = { local: 'local build', debug: 'debug build', experimental: 'experimental build' };
      let tag = tags[buildKind];
      if (branch) tag = `${tag && buildKind !== 'experimental' ? tag : 'local release build'} - ${branch}`;
      if (port) tag = `${tag || 'local build'} - port ${port}`;
      return `DoD Studio — v${baseVersion}${tag ? ` (${tag})` : ''}`;
    },
    STUDIO_TAB: 'Studio',
    DEMO_AUDITOR_TAB: 'Demo Auditor',
    DEMO_ANALYZER_TAB: 'Demo Analyzer',
    HD_TEXTURES_TAB: 'HD Textures',
    BLENDER_TAB: 'Blender',
    OVERVIEWS_TAB: 'Overviews',
    NO_SESSION_LOADED: 'No session loaded',
    FILE_MENU: 'File',
    NEW_SESSION_BUTTON: 'New Session',
    NEW_SESSION_TITLE: 'Start a new, empty session',
    HELP_MENU: 'Help',
    ABOUT_MENU: 'About',
    SAVE_SESSION_BUTTON: 'Save Session',
    SAVE_SESSION_TITLE: 'Save Project Session',
    LOAD_SESSION_BUTTON: 'Load Session',
    LOAD_SESSION_TITLE: 'Load Project Session',
    // File → Rescan Recent Sessions (#938): rescan_recent.js.
    RESCAN_RECENT: 'Rescan Recent Sessions…',
    RESCAN_RECENT_TITLE: 'Rescan the demos an older DoD Studio scanned, in every recent session',
    // File → Open Recent (#935).
    OPEN_RECENT: 'Open Recent',
    OPEN_RECENT_EMPTY: 'No recent sessions',
    OPEN_RECENT_MISSING: '(missing)',
    CLEAR_RECENT: 'Clear Recent',
    CLEAR_RECENT_EMPTY_TITLE: 'Nothing to clear yet',
    RECENT_MISSING_TITLE: 'Session file not found',
    RECENT_MISSING_MESSAGE: "This session file isn't there any more. Remove it from Open Recent?",
    RECENT_MISSING_REMOVE: 'Remove from list',
    RECENT_MISSING_KEEP: 'Keep',
  },
  // ── Workspace Pane: Directory Scan & Master Demo Queue ──────────────────
  // Right-click menus (#1111): the Master Demo Queue's row menu.
  CONTEXT_MENU: {
    label: (name) => `Actions for ${name}`,
    LAUNCH_PREVIEW: 'Launch Preview',
    REVIEW: 'Review this demo',
    OPEN_IN_ANALYZER: 'Open in Demo Analyzer',
    OPEN_FOLDER: 'Open Folder in Explorer',
    SPLIT_MAPS: 'Split Maps',
    USE_FOUND_COPY: 'Use found copy',
    LOCATE: 'Locate demo...',
    COPY_NAME: 'Copy Demo Name',
    COPY_PATH: 'Copy Path',
    REMOVE: 'Remove from Queue',
    DEMO_MISSING: 'The demo file is missing. Use Locate demo first.',
    WHAT_NAME: 'Demo name',
    WHAT_PATH: 'Path',
    copied: (what) => `${what} copied to clipboard.`,
    COPY_FAILED: 'Failed to copy to the clipboard.',
    copiedLog: (what, demo) => `Right-click menu: copied ${what.toLowerCase()} of ${demo}.`,
    copyFailedLog: (what, err) => `Right-click menu: could not copy ${what.toLowerCase()}: ${err}`,
    openFolderLog: (path) => `Right-click menu: opened the folder of ${path} in Explorer.`,
  },
  // ── Draggable splits between panels (#47) ────────────────────────────────
  LAYOUT: {
    SPLIT_HANDLE_TITLE: 'Drag to resize, double-click to reset',
    CAPTURE_SPLIT_RESET_LOG: 'Capture Studio split reset to the default',
    ANALYZER_SPLIT_RESET_LOG: 'Demo Analyzer split reset to the default',
    CAPTURE_SPLIT_LABEL: 'Capture Studio split',
    ANALYZER_SPLIT_LABEL: 'Demo Analyzer split',
    OVERVIEWS_MAPS_LABEL: 'Overviews Maps panel',
    OVERVIEWS_EDIT_LABEL: 'Overviews Edit panel',
    RENDER_TABLE_LABEL: 'Render queue table',
    AUDITOR_TABLE_LABEL: 'Demo Auditor results table',
    splitResetLog: (label) => `${label} size reset to the default`,
    splitNotSaved: (label, err) => `${label} not saved: ${err}`,
  },
  IPC: {
    overviewFailed: (err) => `Could not make that overview: ${err}`,
    overviewSaveFailed: (err) => `Could not save the overview: ${err}`,
    hdSetupFailed: (err) => `Download failed: ${err}`,
    hdBuildFailed: (err) => `Build failed: ${err}`,
    blenderFailed: (err) => `Blender: ${err}`,
    vegasScriptFailed: (err) => `Couldn't add the script to VEGAS: ${err}`,
    hdPythonFailed: (err) => `Could not use that Python: ${err}`,
    hdUpscalerFailed: (err) => `Could not use that folder: ${err}`,
    hdStyleFailed: (err) => `Could not change my_styles.txt: ${err}`,
    hdMapListFailed: (err) => `Could not change hd_maps.txt: ${err}`,
    hdPreviewFailed: (err) => `Comparison failed: ${err}`,
    scanError: (err) => `Scan error: ${err}`,
    dropError: (err) => `Could not read what was dropped: ${err}`,
    validationError: (err) => `Validation error: ${err}`,
    analysisError: (err) => `Analysis error: ${err}`,
    previewFailed: (err) => `Preview failed: ${err}`,
    reviewFailed: (err) => `Could not start the review: ${err}`,
    reviewLibraryFailed: (err) => `Review marks: ${err}`,
    cfgImportFailed: (err) => `Could not read that config: ${err}`,
    processCheckFailed: (err) => `Process check failed: ${err}`,
    launchFailed: (err) => `Launch failed: ${err}`,
    killEngineFailed: (err) => `Failed to close running engine processes: ${err}`,
    cancelScanError: (err) => `Cancel scan error: ${err}`,
    settingsLoadFailed: (err) => `Failed to load settings: ${err}`,
    settingsSaveFailed: (err) => `Failed to save settings: ${err}`,
    auditFailed: (err) => `Audit failed: ${err}`,
    combineFailed: (err) => `Combining the clips failed: ${err}`,
    demoRenameFailed: (err) => `Renaming demos failed: ${err}`,
    matchGroupsFailed: (err) => `Grouping the demos by match failed: ${err}`,
    deletionFailed: (err) => `Deletion failed: ${err}`,
    cancelAuditError: (err) => `Cancel audit error: ${err}`,
    folderOpenFailed: (err) => `Could not open folder: ${err}`,
    previewScanFailed: (err) => `Preview scan failed: ${err}`,
    previewDeletionFailed: (err) => `Preview deletion failed: ${err}`,
    logFileOpenFailed: (err) => `Could not open the log file: ${err}`,
    updateCheckFailed: (err) => `Update check failed: ${err}`,
    updateInstallFailed: (err) => `Update install failed: ${err}`,
  },
  // ── error_reporter.js: the one user-facing crash toast ───────────────────
  ERROR_REPORTER: {
    NO_STACK: '(no stack)',
    NOT_AN_ERROR_OBJECT: '(not an Error object)',
    uncaughtLog: (source, message, stack) => `[error] Uncaught ${source}: ${message}\n${stack}`,
    somethingWentWrong: (message) => `Something went wrong (${message}). Details are in the activity log (Help → View Logs).`,
  },
  // ── Footer ────────────────────────────────────────────────────────────
  FOOTER: {
    VIEW_LOGS_BUTTON: 'View Logs',
    VIEW_LOGS_TITLE: "Open today's activity log in Explorer",
    CHECK_UPDATES_BUTTON: 'Check for Updates',
    CHECK_UPDATES_TITLE: 'Check for app updates',
    UPDATE_AVAILABLE_BUTTON: 'Update Available',
    UPDATE_AVAILABLE_TITLE: 'A new version is ready — click to install it.',
  },
  // ── The menu bar's update button (#868) ──────────────────────────────────
  UPDATE_CHIP: {
    available: (v) => `Update ${v} available`,
    AVAILABLE_TITLE: 'A new version of DoD Studio is out. Click to download and install it.',
    READY: 'Restart to update',
    READY_TITLE: 'The update has downloaded. Click, then Restart to Apply.',
    RESTART_AFTER_BATCH: 'Restart after the batch',
    RESTART_AFTER_BATCH_TITLE: 'The update has downloaded. A capture batch is running, so it waits for the batch to end.',
  },
  // ── About modal (issue #122) ─────────────────────────────────────────────
  ABOUT_MODAL: {
    BLURB: 'A capture, patching, and analytics pipeline for Day of Defeat 1.3 demos — batch-records highlight clips through HLAE and parses matches for scoreboards, kills, chat, and rounds.',
    CREDIT: 'Built by ccoventry',
    GITHUB_LINK: 'github.com/ccoventry/dod-studio',
  },
  // ── What's new window (#1013), whats_new.js ─────────────────────────────
  WHATS_NEW: {
    MENU_ITEM: "What's New",
    MENU_ITEM_TITLE: "What changed in this version and the ones before it",
    ABOUT_BUTTON: "What's new in this version",
    TITLE: "What's New",
    thisVersion: (v) => `This version: v${v}`,
    THIS_VERSION_UNKNOWN: 'This version',
    OTHER_VERSIONS: 'Other versions',
    versionLabel: (v, experimental) => `v${v}${experimental ? ' (experimental)' : ''}`,
    // The release body's <details> list of every pull request, folded.
    everythingElse: (summary) => `Everything else: ${summary}`,
    NO_NOTES: 'No notes were written for this version.',
    NO_BUNDLED_NOTES: "This build doesn't carry its own notes (a build made on this PC has none). They show here once GitHub's release list is read.",
    NO_NOTES_FOR_THIS_BUILD: "No notes for this build: it isn't a published version (a build made on this PC, or an experimental build GitHub has since replaced).",
    FETCH_BUTTON: "Read GitHub's list",
    FETCH_TITLE: "Reads the release notes of every version from GitHub. Update checks are off, so DoD Studio doesn't do it by itself.",
    STATUS_READING: "Reading the release list from GitHub…",
    STATUS_FAILED: "Couldn't read GitHub's release list, and none is saved from before. The reason is in the activity log.",
    releaseListUnreadableLog: (err) => `What's new: the release list could not be read: ${err}`,
    githubReleaseListUnreadableLog: (err) => `What's new: GitHub's release list could not be read: ${err}`,
    STATUS_SAVED_COPY: "Couldn't read GitHub's release list, so this is the copy saved last time. The reason is in the activity log.",
    STATUS_OFFLINE_SAVED: 'Update checks are off, so this is the release list saved last time.',
    STATUS_OFFLINE_NONE: "Update checks are off, so only this version's notes show.",
    updatedTo: (v) => `DoD Studio updated to v${v}.`,
    TOAST_ACTION: "See what's new",
    // The update window: the pending update's notes, before installing it.
    PENDING_TITLE: "What's in it",
  },
  // ── Update check/install modal (issue #133) ──────────────────────────────
  UPDATE_MODAL: {
    TITLE: 'Check for Updates',
    currentVersionLabel: (v) => `Current version: v${v}`,
    CHANNEL_LABEL: 'Update Channel:',
    CHANNEL_STABLE: 'Stable (from main)',
    CHANNEL_EXPERIMENTAL: 'Experimental (from dev, on-demand builds)',
    AUTO_CHECK_LABEL: 'Automatically Check on Startup',
    NOTIFY_LABEL: 'OS Notification on Found',
    NOTIFY_TITLE: 'Fires an OS toast when a background or manual check finds a newer version.',
    CHECK_NOW_BUTTON: 'Check for Updates Now',
    DOWNLOAD_INSTALL_BUTTON: 'Download & Install',
    RESTART_BUTTON: 'Restart to Apply',
    RESTART_AFTER_BATCH_BUTTON: 'Restart after the batch',
    RESTART_AFTER_BATCH_TITLE: 'A capture batch is running: restarting now would stop Studio watching it.',
    INSTALL_AFTER_BATCH_TITLE: 'A capture batch is running: installing closes DoD Studio, so this waits for the batch to end.',
    CLOSE_BUTTON: 'Close',
    STATUS_CHECKING: 'Checking for updates…',
    STATUS_UP_TO_DATE: 'Up to date.',
    STATUS_AVAILABLE: (v) => `Update available: v${v}`,
    STATUS_DOWNLOADING: 'Downloading the update…',
    statusDownloadingBytes: (bytes) => `Downloading the update: ${bytes}`,
    // On Windows the installer closes Studio and takes over from here.
    STATUS_INSTALLING: 'Installing the update… DoD Studio closes while it installs.',
    STATUS_READY: 'Ready: restart to finish.',
    STATUS_CHECK_FAILED: (err) => `Update check failed: ${err}`,
    // Local and debug builds: report what's published, never offer to install
    // it -- the installer would replace the *installed* app, not this one.
    statusLocalBuild: (stable, experimental) =>
      `Latest stable: ${stable ? `v${stable}` : 'unavailable'} · latest experimental: ${experimental ? `v${experimental}` : 'unavailable'}. `
      + "This is a local build, so updates aren't installed from here: installing would replace your installed DoD Studio, not this copy. "
      + 'Get the published build from the Releases page.',
  },
  // Byte counts on the update and map downloads (download_progress.js).
  DOWNLOADS: {
    bytesOf: (done, total) => `${done} of ${total} MB`,
    bytes: (done) => `${done} MB`,
  },
  // ── OS Toast Notifications (issue #98) ──────────────────────────────────
  // Titles/bodies for os_notifications.js's notify() calls. Bodies reuse the
  // existing CAPTURE/RENDER in-app-toast strings where the wording already
  // fits, rather than duplicating near-identical copy.
  NOTIFICATIONS: {
    CAPTURES_DONE_TITLE: 'Captures complete',
    CAPTURES_ERROR_TITLE: 'Capture error',
    RENDERS_DONE_TITLE: 'Renders complete',
    RENDERS_ERROR_TITLE: 'Render errors',
    patchingStartedTitle: (total) => `Patching ${total} demo${total === 1 ? '' : 's'}`,
    PATCHING_STARTED_BODY: 'Preparing demos for capture…',
    PATCHING_FINISHED_TITLE: 'Patching complete',
    patchingFinishedBody: (total) => `${total} demo${total === 1 ? '' : 's'} ready — starting capture`,
    // Shared by both the demo-loading and fast-forward-to-clip toasts —
    // same demo, same title, whichever of the two actually fires.
    demoLoadingTitle: (index, total) => `Viewing demo ${index} of ${total}`,
    demoLoadingBody: (clipCount, clipsSoFar, totalBatchClips) => {
      const clipWord = clipCount === 1 ? 'clip' : 'clips';
      const onThisDemo = `${clipCount} ${clipWord} on this demo`;
      return totalBatchClips ? `${onThisDemo} · ${clipsSoFar} of ${totalBatchClips} clips total` : onThisDemo;
    },
    fastForwardToClipBody: (clipIndex, clipCountThisDemo, clipsSoFar, totalBatchClips) => {
      const onThisDemo = `Fast-forwarding to clip ${clipIndex} of ${clipCountThisDemo}`;
      return totalBatchClips ? `${onThisDemo} · ${clipsSoFar} of ${totalBatchClips} clips total` : onThisDemo;
    },
  },
  // A file dialog that is slow to appear (dialog_wait.js).
  FILE_DIALOG: {
    OPENING: 'Opening the file dialog… Windows can take a few seconds the first time.',
  },
  // Dropping demos and folders from Windows (#530): demo_drop.js.
  DROP: {
    skippedToast: (names) => {
      const shown = names.slice(0, 5).join(', ');
      const more = names.length > 5 ? ` and ${names.length - 5} more` : '';
      return `Not a demo or a folder, so not added: ${shown}${more}`;
    },
    BUSY_TOAST: 'A scan is running. Drop them again when it has finished.',
    // A demo opened from Explorer into the queue during a scan (#528).
    openedWhileScanningToast: (names) => {
      const shown = names.slice(0, 3).join(', ');
      const more = names.length > 3 ? ` and ${names.length - 3} more` : '';
      return `A scan is running, so ${shown}${more} was not added. Open it again when the scan has finished.`;
    },
  },
};
