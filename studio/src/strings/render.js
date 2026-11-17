// strings/render.js: the app's text for Render Studio, finishing clips and take cleanup.
// Part of the STRINGS catalog that ../strings.js assembles (#604).

export const RENDER_STRINGS = {
  // ── Render Studio panel + render_pane.js ─────────────────────────────────
  RENDER: {
    PANEL_TITLE: 'Render Studio',
    CODEC_LABEL: 'Codec:',
    CODEC_PRORES: 'ProRes 422 HQ',
    CODEC_PRORES_422: 'ProRes 422 (2/3 the size of HQ)',
    CODEC_PRORES_LT: 'ProRes 422 LT (half the size of HQ)',
    CODEC_PRORES_4444: 'ProRes 4444 (1.5x the size of HQ)',
    // The line under the Codec picker for each ProRes choice (codec_hint.js).
    CODEC_HINT_PRORES: 'Looks the same as the capture; the largest of the three 422 choices, for when file size is no concern.',
    CODEC_HINT_PRORES_422: 'Recommended for most videos: looks the same as HQ in our tests, about 2/3 the size.',
    CODEC_HINT_PRORES_LT: 'About half the size of HQ. Fine for game footage; a little more detail is lost in fast motion and fine textures.',
    CODEC_HINT_PRORES_4444: 'Full colour detail, about 1.5x the size of HQ. Only worth it for heavy colour grading or small coloured text drawn into the picture. No transparency: the separate HUD stream always has its own.',
    CODEC_DNXHR: 'DNxHR HQ',
    // #438: each take's render history.
    TABLE_HEADER_HISTORY: 'History',
    TABLE_HEADER_HISTORY_TITLE: 'Every time this take was rendered, kept beside the take so it survives restarts. ▸ lists each attempt.',
    HISTORY_NEW: 'New',
    historyRendered: (count) => `Rendered ×${count}`,
    HISTORY_OUTCOME: { finished: 'Rendered', failed: 'Failed', cancelled: 'Cancelled', interrupted: 'Interrupted', rendering: 'Rendering…' },
    HISTORY_SKIPPED_SETTINGS: 'Skip (kept original)',
    HISTORY_TOGGLE_TITLE: 'Show or hide every render of this take',
    historyFileNotFound: (name) => `${name} was moved or deleted`,
    // #108: named render setups.
    PRESET_LABEL: 'Preset:',
    PRESET_TITLE: 'A saved codec, FPS and concurrency. Pick one to apply it. Shows — when the settings above match no preset.',
    PRESET_NONE: '—',
    PRESET_NAME_PLACEHOLDER: 'Name for these settings',
    PRESET_SAVE_BUTTON: 'Save Preset',
    PRESET_SAVE_TITLE: 'Save the codec, FPS and concurrency above under the name typed here, or over the picked preset when no name is typed.',
    PRESET_DELETE_BUTTON: 'Delete',
    PRESET_DELETE_TITLE: 'Delete the picked preset. The settings above stay as they are.',
    // #40: sortable columns and the whole-batch bar.
    SORT_HEADER_TITLE: 'Click to sort; again to reverse; a third time for the batch order.',
    batchProgress: (pct) => `Batch ${pct}%`,
    // #110: a CSV of every captured highlight, for an editor.
    EXPORT_MARKERS_BUTTON: 'Export Marker List…',
    EXPORT_MARKERS_TITLE: 'Save a CSV with one row per captured or rendered highlight: its demo, player, kills, where it is in the demo, its take and its label. For lining clips up in your editor.',
    EXPORT_MARKERS_NONE: 'No captured highlights in the loaded project yet, so there is nothing to export.',
    exportMarkersDone: (count) => `Marker list saved (${count} highlight${count === 1 ? '' : 's'})`,
    exportMarkersFailed: (err) => `Couldn't save the marker list: ${err}`,
    // #110: the finished renders as an EDL for VEGAS Pro.
    EXPORT_EDL_BUTTON: 'Export VEGAS EDL…',
    EXPORT_EDL_TITLE: "Save the finished renders as an EDL, one after another with their sound. In VEGAS Pro use File > Import > EDL Text File. The clips stay where they are, so keep them there. It is saved as .txt because that is the only type VEGAS's import dialog lists. A .dodstudio.json file beside it lists each clip's player and nameplate, for the VEGAS nameplate script.",
    exportEdlNone: (skipped) => skipped > 0
      ? `${skipped} finished render${skipped === 1 ? '' : 's'} can't be placed: FFmpeg couldn't read ${skipped === 1 ? 'its' : 'their'} length.`
      : 'No finished renders yet, so there is nothing to export.',
    // #1264: every video rendered for the loaded project, as one EDL.
    EXPORT_PROJECT_EDL_BUTTON: 'Export Project to VEGAS…',
    EXPORT_PROJECT_EDL_TITLE: "Save an EDL with every video rendered for the loaded project that is still on disk, even when its take was deleted, in movie order, with HUD passes laid over their clips. When highlights are ticked, only their videos go in. A video you moved is found again by its name in the render export folders. Same import as Export VEGAS EDL.",
    exportProjectNone: (total) => total > 0
      ? `None of the ${total} video${total === 1 ? '' : 's'} rendered for this project could be found. Add the folder they are in to the render export folders in Configuration.`
      : 'No videos have been rendered for the loaded project yet, so there is nothing to export.',
    exportProjectLooking: (count) => `Finding ${count} rendered video${count === 1 ? '' : 's'} on disk…`,
    exportProjectReading: (n, total) => `Reading the length of video ${n} of ${total} for the EDL…`,
    EXPORT_PROJECT_PICKING: 'Choose where to save the EDL…',
    EXPORT_PROJECT_WRITING: 'Writing the EDL and the clip list…',
    exportProjectLookFailed: "Couldn't look for the project's videos on disk.",
    exportProjectDone: (count, scope, missing, skipped, nameplates = 0) => `EDL saved (${count} video${count === 1 ? '' : 's'}${scope === 'ticked' ? ' from the ticked highlights' : ''}${nameplates > 0 ? `, ${nameplates} with a nameplate` : ''}${missing > 0 ? `, ${missing} missing` : ''}${skipped > 0 ? `, ${skipped} left out: FFmpeg couldn't read the length` : ''})`,
    // #1442: find the project's videos again by the DoD Studio tag on each.
    FIND_MOVED_BUTTON: 'Find Moved Videos',
    FIND_MOVED_TITLE: "Find the videos rendered for the loaded project after you moved or renamed them, by the DoD Studio tag on each video, and point the project at their new place. Looks in the render export folders and their subfolders; when some still aren't found, you can pick another folder. Videos still where they were rendered get the tag too, so they can be found after a move. Tagging never changes the video.",
    findMovedNone: 'No videos have been rendered for the loaded project yet, so there is nothing to find.',
    findMovedTagging: (n, total) => `Looking at video ${n} of ${total} where it was rendered…`,
    findMovedListing: (files) => `Listing the folders: ${files} video file${files === 1 ? '' : 's'} so far…`,
    findMovedChecking: (n, total) => `Reading the tag of file ${n} of ${total}…`,
    findMovedDone: ({ moved, notFound, total, complete = true, cloudSkipped = 0, cancelled = false }) => {
      const found = moved > 0
        ? `Found ${moved} moved video${moved === 1 ? '' : 's'} and pointed the project at ${moved === 1 ? 'it' : 'them'}.`
        : `No moved videos found${notFound === 0 && !cancelled ? `: all ${total} are where they were rendered` : ''}.`;
      const cloud = cloudSkipped > 0
        ? ` ${cloudSkipped} file${cloudSkipped === 1 ? ' is' : 's are'} only in the cloud, so ${cloudSkipped === 1 ? 'it was' : 'they were'} left unread (reading would download ${cloudSkipped === 1 ? 'it' : 'them'}).`
        : '';
      if (cancelled) return `Stopped. ${found}${cloud}`;
      if (notFound === 0) return `${found}${cloud}`;
      const stopped = complete ? '' : ' The search stopped at its limit, so look in a smaller folder.';
      return `${found} ${notFound} of ${total} still not found.${stopped}${cloud}`;
    },
    FIND_MOVED_CANCEL_BUTTON: 'Cancel',
    FIND_MOVED_PICK_ACTION: 'Look in a folder…',
    FIND_MOVED_PICK_TITLE: 'Folder to look for the videos in',
    findMovedFailed: (err) => `Couldn't look for the project's videos: ${err}`,
    // #429, #1514: the nameplate and relink scripts into VEGAS's script menu.
    VEGAS_SCRIPT_BUTTON: 'Add Scripts to VEGAS',
    VEGAS_SCRIPT_TITLE: 'Copy the VEGAS scripts (nameplates, and relinking moved clips) into Documents\\Vegas Script Menu (made if it is not there), replacing older copies of them. Then in VEGAS Pro use Tools > Scripting > Rescan Script Menu Folder.',
    vegasScriptDone: (path) => `Scripts copied to ${path}. In VEGAS Pro use Tools > Scripting > Rescan Script Menu Folder, then pick them from Tools > Scripting.`,
    // #1514: the relink list for a VEGAS project made before the clips moved.
    RELINK_EXPORT_BUTTON: 'Export VEGAS Relink List…',
    RELINK_EXPORT_TITLE: "Save a list of where each video rendered for the loaded project used to be and where it is now, from the moves Find Moved Videos found. In VEGAS Pro, run the DoD Studio Relink Clips script on your project: it points every moved clip at its new file and keeps your edit. Save it beside your .veg project and the script finds it by itself.",
    RELINK_BACKUP_BUTTON: 'From a Project Backup…',
    RELINK_BACKUP_TITLE: "The same list, also comparing with an older copy of this project file saved before the videos moved (for videos moved before Studio kept track of moves). The copy is only read, never changed.",
    RELINK_BACKUP_PICK_TITLE: 'Older copy of this project file, from before the videos moved',
    RELINK_RERENDERS_LABEL: 'Also swap older renders of a highlight for its newest render',
    RELINK_RERENDERS_TITLE: "When a highlight was rendered again after you started editing, the edit can use the new render. A re-render can start or end somewhere else, so the list warns about each one whose length differs: check those clips' trims by eye.",
    RELINK_PICKING_BACKUP: 'Pick the older copy of the project file…',
    relinkReadingBackup: (name) => `Reading ${name}…`,
    relinkChecking: (count) => `Checking the ${count} new file${count === 1 ? '' : 's'} are there…`,
    relinkReading: (n, total) => `Reading the length of re-render ${n} of ${total}…`,
    RELINK_PICKING_SAVE: 'Choose where to save the relink list…',
    RELINK_WRITING: 'Writing the relink list…',
    RELINK_NONE: "No video of the loaded project has been moved since Studio started keeping track (Find Moved Videos keeps each one's old place). For videos moved before that, use From a Project Backup… with a copy of the project file from before the move.",
    RELINK_NONE_BACKUP: 'The backup and the loaded project have every video in the same place: nothing to relink.',
    relinkBackupFailed: (path, err) => `Couldn't read ${path} as a project file: ${err}`,
    relinkWriteFailed: (err) => `Couldn't save the relink list: ${err}`,
    relinkDone: ({ moved, reRendered, missingTargets = 0, lengthDiffers = 0 }) => {
      const parts = [`${moved} moved video${moved === 1 ? '' : 's'}`];
      if (reRendered > 0) parts.push(`${reRendered} older render${reRendered === 1 ? '' : 's'} swapped for the newest`);
      const notes = [];
      if (missingTargets > 0) notes.push(` ${missingTargets} new file${missingTargets === 1 ? " isn't" : "s aren't"} there now: run Find Moved Videos first.`);
      if (lengthDiffers > 0) notes.push(` ${lengthDiffers} re-render${lengthDiffers === 1 ? ' has' : 's have'} a different length: check ${lengthDiffers === 1 ? 'its' : 'their'} trims in VEGAS.`);
      return `Relink list saved (${parts.join(', ')}). In VEGAS, run DoD Studio Relink Clips on the project.${notes.join('')}`;
    },
    exportEdlProbing: (count) => `Reading the length of ${count} video${count === 1 ? '' : 's'} for the EDL…`,
    exportEdlDone: (count, skipped, nameplates = 0) => `EDL saved (${count} clip${count === 1 ? '' : 's'}${nameplates > 0 ? `, ${nameplates} with a nameplate` : ''}${skipped > 0 ? `, ${skipped} left out: FFmpeg couldn't read the length` : ''})`,
    exportEdlFailed: (err) => `Couldn't save the EDL: ${err}`,
    // #429: the clip manifest beside the EDL, for the nameplate script.
    exportManifestFailed: (err) => `The EDL is saved, but the clip list for the nameplate script isn't: ${err}`,
    CODEC_HUFFYUV: 'HuffYUV (Lossless, AVI)',
    CODEC_UNCOMPRESSED: 'Uncompressed (AVI, huge)',
    CODEC_H264: 'H.264 (Software, MP4)',
    CODEC_H264_NVENC: 'H.264 (NVENC GPU, MP4)',
    CODEC_CUSTOM: 'Custom (paste FFmpeg args)',
    CUSTOM_CODEC_LABEL: 'Custom FFmpeg args:',
    CUSTOM_CODEC_PLACEHOLDER: '-c:v mpeg4 -q:v 3',
    CUSTOM_CODEC_ARGS_REQUIRED: 'Enter the FFmpeg video-codec arguments to use for Custom (e.g. -c:v mpeg4 -q:v 3).',
    SOURCE_FPS_LABEL: 'Source FPS:',
    SOURCE_FPS_TITLE: 'The frame rate a take is read at when it does not say what it was captured at (takes from older versions). Takes captured by this version render at their own capture FPS, and each row says which it used.',
    MAX_CONCURRENT_LABEL: 'Max Concurrent Renders:',
    RESUMABLE_LABEL: 'Resumable renders (split long renders into pieces)',
    RESUMABLE_HINT: 'A stopped render picks up where it left off. While rendering it needs up to about twice the clip\'s size free, and the pieces are joined at the end. With "Only when space is needed" on, finished takes may be deleted to make that room. When off, renders run in one piece, and a stopped render\'s saved pieces are deleted the next time that take renders.',
    EXPORT_DIR_PLACEHOLDER: 'Add export drive/folder...',
    EXPORT_DIR_ROW_PLACEHOLDER: 'Export drive/folder path...',
    ADD_DRIVE_BUTTON: 'Add Drive',
    BROWSE_DRIVE_BUTTON: 'Browse Drive',
    TOTAL_EXPORT_POOL_FREE_LABEL: 'Total Export Pool Free:',
    EXPORT_POOL_FREE_DEFAULT: '0.0 GB',
    exportPoolFreeGb: (gb) => `${gb} GB`,
    SCAN_FOR_TAKES_BUTTON: 'Scan for Takes',
    TABLE_HEADER_CLIP_NAME: 'Clip Name',
    TABLE_HEADER_STREAM: 'Stream',
    TABLE_HEADER_FRAMES: 'Frames',
    TABLE_HEADER_DATE: 'Date',
    TABLE_HEADER_SETTINGS: 'Settings',
    TABLE_HEADER_SETTINGS_TITLE: "Codec/FPS this job is queued to render with — its own setting, not necessarily what Configuration currently shows",
    TABLE_HEADER_STATUS: 'Status',
    TABLE_HEADER_SPEED: 'Speed',
    TABLE_HEADER_PROGRESS: 'Progress',
    TABLE_HEADER_FILE_SIZE: 'File Size',
    TABLE_HEADER_FILE_SIZE_TITLE: "The real encoded size, once finished — not shown ahead of time, since it can't be predicted accurately (see Required (Over-estimated) in the footer).",
    fileSizeGb: (gb) => `${gb} GB`,
    fileSizeMb: (mb) => `${mb} MB`,
    FILE_SIZE_UNKNOWN: '—',
    TABLE_HEADER_ACTIONS: 'Actions',
    TABLE_EMPTY: 'No render jobs queued. Scan a folder, then click Start Render Batch.',
    START_RENDER_BUTTON: 'Start Render Batch',
    // #70: Start renders every queued row; the other footer buttons act on
    // the ticked rows, and say on how many.
    startRenderBatch: (n) => `Start Render Batch (${n})`,
    START_RENDER_TITLE: 'Render every queued row. Cancel or remove a row to leave it out.',
    START_RENDER_NONE_TITLE: 'Nothing is queued. Scan for takes, or Retry a finished row.',
    START_RENDER_NO_EXPORT_DIR_TITLE: 'Add an export folder in Configuration → Render Settings first.',
    cancelSelected: (n) => `Cancel (${n})`,
    CANCEL_SELECTED_TITLE: 'Stop the ticked rows that are rendering or queued.',
    cancelQueued: (n) => `Cancel Queued (${n})`,
    CANCEL_QUEUED_TITLE: 'Stop every row that is waiting to render. Rows already rendering keep going.',
    retrySelected: (n) => `Retry (${n})`,
    RETRY_SELECTED_TITLE: 'Queue the ticked rows that finished, failed or were cancelled again.',
    removeSelected: (n) => `Remove (${n})`,
    REMOVE_SELECTED_TITLE: 'Take the ticked rows off the list. A rendering row stays.',
    deleteTakesSelected: (n) => `Delete Take folders (${n})`,
    DELETE_TAKES_SELECTED_TITLE: "Move the ticked rows' take folders to the Recycle Bin. A take that is rendering stays; a queued one leaves the queue.",
    CLEAR_QUEUE_BUTTON: 'Clear Queue',
    CLEAR_QUEUE_TITLE: 'Empty the list. Anything rendering is cancelled first.',
    CLEAR_QUEUE_CONFIRM_TITLE: 'Clear Queue',
    clearQueueConfirmMessage: (rendering) => (rendering === 1
      ? 'A take is rendering. Stop it and empty the list?'
      : `${rendering} takes are rendering. Stop them and empty the list?`),
    CLEAR_QUEUE_CONFIRM_BUTTON: 'Stop and clear',
    SELECT_UNRENDERED_BUTTON: 'Select unrendered',
    SELECT_UNRENDERED_TITLE: 'Tick only the takes with no finished render in their history.',
    SELECT_FINISHED_BUTTON: 'Select finished',
    SELECT_FINISHED_TITLE: 'Tick only the takes whose clips are done: the last render of each part finished and the clip file is still there. Then Delete Take folders clears them in one go.',
    FINISHED_AGE_TITLE: 'Which finished takes Select finished ticks: all of them, or only those rendered at least this long ago.',
    FINISHED_AGE_ANY: 'of any age',
    FINISHED_AGE_DAY: 'over a day old',
    FINISHED_AGE_WEEK: 'over a week old',
    FINISHED_AGE_MONTH: 'over a month old',
    SELECT_ALL_CB_TITLE: 'Tick or untick every row',
    SELECT_ROW_TITLE: 'Tick this row for Cancel, Retry, Remove and Delete Take folders below',
    selectedCount: (ticked, total) => `${ticked} of ${total} selected`,
    STATUS_WAITING: 'Status: Waiting...',

    CANCEL_JOB_TITLE: 'Cancel this job',
    RESET_JOB_TITLE: 'Reset to Queued',
    REMOVE_JOB_TITLE: 'Remove this row — cannot be undone',
    SKIP_TOGGLE_LABEL: 'Skip',
    SKIP_TOGGLE_TITLE: 'Leave this OBS take exactly as recorded — no re-encode, just routed into the export pool under the pipeline naming.',
    setJobCodecFailed: (err) => `Could not change this job's render setting: ${err}`,
    VIEW_LOG_TITLE: 'View error log',
    VIEW_LOG_BUTTON: '⚠️ View Log',
    OPEN_OUTPUT_FOLDER_TITLE: "Open the rendered file's folder",
    OPEN_TAKE_FOLDER_TITLE: 'Open the source take folder',
    OPEN_OUTPUT_BUTTON: '📁 Open Output',
    OPEN_TAKE_FOLDER_BUTTON: '📁 Open Take Folder',
    // #644: Delete Take sends the take folder to the Recycle Bin.
    DELETE_TAKE_BUTTON: 'Delete Take',
    DELETE_TAKE_TITLE: "Move this take's folder to the Recycle Bin. Its render history stays.",
    DELETE_TAKE_RENDERING_TITLE: 'This job is rendering. Cancel it first.',
    DELETE_TAKE_PAIR_RENDERING_TITLE: 'Another stream of this take is rendering. Cancel it first.',
    DELETE_TAKE_RUNNING_TITLE: 'A take is being deleted.',
    deleteTakeConfirmTitle: (n) => (n === 1 ? 'Delete Take' : `Delete ${n} Takes`),
    deleteTakeConfirmMessage: (n, size) => (n === 1
      ? `Move this take (${size}) to the Recycle Bin?`
      : `Move these ${n} takes (${size}) to the Recycle Bin?`),
    DELETE_TAKE_CONFIRM_FOOTER: 'The space comes back when the Recycle Bin is emptied. The render history stays.',
    deleteTakeQueuedNote: (n) => (n === 1
      ? 'Its queued render is removed from the queue.'
      : `${n} queued renders are removed from the queue.`),
    DELETE_TAKE_CONFIRM_BUTTON: 'Move to Recycle Bin',
    deleteTakeMeasuring: (done, total) => `Measuring takes: ${done} of ${total}`,
    deleteTakeRecycling: (done, total) => `Moving to the Recycle Bin: ${done} of ${total}`,
    takesDeleted: (n, size) => `${n === 1 ? 'Take' : `${n} takes`} moved to the Recycle Bin (${size}).`,
    takesDeleteFailed: (deleted, failed, firstError) =>
      `${failed} take${failed === 1 ? '' : 's'} not deleted${deleted > 0 ? ` (${deleted} moved to the Recycle Bin)` : ''}: ${firstError}. Scan for Takes to list ${failed === 1 ? 'it' : 'them'} again.`,
    deleteTakeRefused: (err) => `Couldn't delete the take: ${err}`,
    // #1069: a picture of each take, and its contact sheet.
    TABLE_HEADER_PICTURE: 'Picture',
    TABLE_HEADER_PICTURE_TITLE: 'The middle of each take. Hover for a bigger picture, click for nine frames across the take.',
    TAKE_THUMB_TITLE: 'Click for nine frames across the take.',
    TAKE_THUMB_WAITING: 'Waiting for its picture.',
    TAKE_THUMB_MAKING: 'Making its picture.',
    takeThumbFailed: (err) => `No picture: ${err}`,
    takePicturesProgress: (done, total) => `Making take pictures: ${done} of ${total}`,
    takeSheetTitle: (name) => `Take pictures: ${name}`,
    TAKE_SHEET_LOADING: 'Loading the pictures…',
    takeSheetFailed: (err) => `Couldn't show the pictures: ${err}`,
    takeSheetSeconds: (secs) => `${secs} s`,
    takeSheetFrame: (frame) => `frame ${frame}`,
    TAKE_SHEET_CLOSE: 'Close',
    // Take checks (#1084): the chip under each take's picture, its tooltip
    // and the list on the contact sheet.
    TAKE_CHECK_OK: 'OK',
    takeCheckWarnings: (n) => `⚠ ${n}`,
    TAKE_CHECK_CHECKING: '…',
    TAKE_CHECK_FAILED: '?',
    TAKE_CHECK_WAITING_TITLE: 'Waiting to be checked.',
    TAKE_CHECK_CHECKING_TITLE: 'Checking the picture and sound.',
    TAKE_CHECK_ALL_GOOD: 'Picture, sound, length and kills look good.',
    takeCheckFailed: (err) => `Couldn't check this take: ${err}`,
    takeChecksProgress: (done, total) => `Checking takes: ${done} of ${total}`,
    TAKE_CHECK_CAUSE_CAPTURE: 'Capturing again helps',
    TAKE_CHECK_CAUSE_DEMO: 'The demo itself, capturing again won\'t help',
    takeCheckBlack: (secs, at) => `Black picture for ${secs} s from ${at} s.`,
    takeCheckFrozen: (secs, at) => `The picture doesn't move for ${secs} s from ${at} s: the demo may have no camera data there.`,
    takeCheckHolds: (count, secs, at) => `The picture holds still briefly ${count} ${count === 1 ? 'time' : 'times'} (longest ${secs} s at ${at} s): the demo itself lags there, or nothing moved.`,
    TAKE_CHECK_NO_SOUND: 'No sound was recorded.',
    TAKE_CHECK_SILENT: 'The sound is silent all the way through.',
    takeCheckClipping: (pct) => `The sound clips: ${pct}% of it is at full volume. Lower the capture's sound volume.`,
    takeCheckLength: (frames, planned, secs) => `The take has ${frames} frames (${secs} s) but ${planned} were planned: part of the clip is missing or it ran on too long.`,
    takeCheckKillOutside: (count, at, secs) => `${count === 1 ? 'A kill is' : `${count} kills are`} not in the clip (${count === 1 ? 'it' : 'the first'} falls at ${at} s of a ${secs} s clip).`,
    TAKE_CHECK_NO_PLAN: "The clip's length and kills weren't checked: this take has no plan recorded (captured before Studio kept one, or from a demo scanned before then).",
    TAKE_SHEET_CHECKS_TITLE: 'Checks',
    QUEUE_SUMMARY_DEFAULT: '0 queued · 0 rendering · 0 done',
    queueSummary: (queued, rendering, done) => `${queued} queued · ${rendering} rendering · ${done} done`,
    ERROR_LOG_TITLE_DEFAULT: 'FFmpeg Error Log',
    errorLogTitleForJob: (name) => `FFmpeg Error Log — ${name}`,
    exportPoolFreeFooter: (gb) => `Export Pool Free: ${gb} GB`,
    RENDER_POOL_FREE_DEFAULT: 'Export Pool Free: 0.0 GB',
    // "Over-estimated" matters here in a way it doesn't for Capture's
    // Required figure — this is a deliberately loose upper bound (raw-frame
    // math), not a tight prediction. See get_render_required_estimate_gb.
    requiredEstimatedFooter: (gb) => `Required (Over-estimated): ${gb} GB`,
    REQUIRED_ESTIMATED_FOOTER_TITLE: "A loose upper bound, not a tight prediction — most codecs compress well below this, and it can't be known ahead of time how much. Covers every job that hasn't finished yet (Queued + Rendering).",
    REQUIRED_ESTIMATED_DEFAULT: 'Required (Over-estimated): 0.00 GB',
    recoveredJobsToast: (completed, pending) => `Recovered ${completed} completed, ${pending} pending render job(s).`,
    recoverFailed: (err) => `Failed to recover render batch: ${err}`,
    STATUS_RECOVERING: 'Status: Recovering the render batch…',
    recoveredStatus: (queued) => `Status: Render batch recovered — ${queued} take(s) queued`,
    // #954: the batch line counts only this batch's rows.
    renderingLine: (n, total) => `Rendering ${n} of ${total}`,
    doneIn: (duration, parts) => `Done in ${duration}: ${parts}`,
    batchStartLog: () => '[render] started',
    batchEndLog: (elapsed, finished, failed, cancelled) => `[render] done after ${elapsed}: ${finished} rendered, ${failed} failed, ${cancelled} cancelled`,
    BATCH_CANCELLED: 'Render batch cancelled.',
    BATCH_COMPLETED: 'Render batch completed successfully!',
    // Mixed-outcome batches report every non-zero count instead of one label,
    // so cancelling the takes you did not want does not read as "nothing
    // rendered". Singular/plural matters here — these numbers are often 1.
    countRendered: (n) => `${n} rendered`,
    countFailed: (n) => (n === 1 ? '1 failed' : `${n} failed`),
    countCancelled: (n) => `${n} cancelled`,
    batchSummary: (parts) => `Render batch finished — ${parts}.`,
    STATUS_FINISHED: 'Status: Finished',
    STATUS_CANCELLED: 'Status: Cancelled',
    statusFinishedWith: (parts) => `Status: Finished — ${parts}`,
    // #954: Scan for Takes' line while it looks, and the one it ends on.
    lookingForTakes: (n) => `Looking for takes: ${n} found so far`,
    foundTakesIn: (total, parts, elapsed) => `Found ${total} ${total === 1 ? 'take' : 'takes'} (${parts}) in ${elapsed}`,
    noTakesFoundIn: (elapsed) => `Found no takes in ${elapsed}`,
    scanNewShort: (n) => `${n} new`,
    SCAN_NONE_NEW: 'none new',
    ADD_RENDER_DIR_REQUIRED: 'Please add at least one render directory.',
    // The line under the field that is wrong (#25), where a toast used to be the only word.
    RENDER_DIR_FIELD_EMPTY: 'Add the folder your captures are written to, so Scan for Takes has somewhere to look.',
    EXPORT_DIR_FIELD_EMPTY: 'Add an export folder: finished clips are written there, and Start Render Batch stays off until you do.',
    scanCompleteStatus: (count) => `Status: Scan complete — ${count} take(s) found`,
    // What a scan changed in the queue (#772); render_scan_summary.js joins them.
    scanNewTakes: (n) => (n === 1 ? '1 new take' : `${n} new takes`),
    scanRefreshed: (n) => `${n} refreshed`,
    scanGone: (n, queued) => (queued > 0 ? `${n} gone (${queued} ${queued === 1 ? 'was' : 'were'} queued)` : `${n} gone`),
    scanMergedToast: (parts) => `Scan complete: ${parts}.`,
    scanMergedStatus: (parts) => `Status: Scan complete — ${parts}`,
    SCAN_NO_CHANGES: 'Scan complete: no changes, every take found is already in the queue.',
    STATUS_SCAN_NO_CHANGES: 'Status: Scan complete — no changes',
    NO_RENDER_TAKES_DETECTED: 'No render takes detected.',
    scanDirError: (err) => `Error scanning render directories: ${err}`,
    STATUS_SCAN_FAILED: 'Status: Scan failed',
    INITIALIZING_RENDER_BATCH: 'Initializing render batch...',
    RENDER_BATCH_QUEUED: 'Render batch queued successfully!',
    renderBatchError: (err) => `Error executing render batch: ${err}`,
    CANCELLING_RENDER_BATCH: 'Cancelling render batch...',
    nvencWarning: (n) => `${n} concurrent NVENC renders may exceed your GPU's encoder session limit (often 3-5 on consumer GeForce cards). If renders start failing, lower Max Concurrent Renders.`,
    highlightsMarkedRendered: (n) => `${n} highlight(s) marked Rendered.`,
    UNKNOWN_SOURCE_FOLDER: '(unknown)',
  },
  // ── Finishing clips after a capture batch (#440) + finish_clips.js ───────
  FINISH: {
    SECTION_TITLE: 'When a batch finishes',
    WHEN_LABEL: 'When a batch finishes:',
    WHEN_TITLE:
        'Finish clips automatically turns every take the batch captured into one video with its sound, in the export folder below, as soon as the batch ends. The jobs show up in the Render tab like any other. Leave the takes to render them yourself from the Render tab.',
    WHEN_LEAVE: 'Leave the takes for the Render tab',
    WHEN_FINISH: 'Finish clips automatically',
    CODEC_OBS_LABEL: 'OBS clips:',
    CODEC_VIDEO_LABEL: 'Video clips:',
    CODEC_FRAMES_LABEL: 'Frame sequence clips:',
    CODEC_TITLE: 'What each finished clip is saved as, for batches captured in this mode.',
    CODEC_KEEP: 'Keep as captured (copy, seconds)',
    CODEC_RENDER_TAB: 'Same as the Codec setting above',
    progressStatus: (finished, total, rendering) =>
      `Finishing clips: ${finished} of ${total}${rendering > 0 ? `, ${rendering} in progress` : ''}`,
    WAITING_STATUS: 'Finishing clips: waiting for the Render tab to finish what it is already doing',
    SCANNING_STATUS: 'Finishing clips: looking for the new takes',
    startedToast: (n) => `Finishing ${n} clip${n === 1 ? '' : 's'}.`,
    VIEW_IN_RENDER_TAB: 'View in Render tab',
    NOTIFY_TITLE: 'Clips ready',
    doneSummary: (finished, failed, cancelled) => {
      const ready = `${finished} clip${finished === 1 ? '' : 's'} ready`;
      const extra = [];
      if (failed > 0) extra.push(`${failed} failed`);
      if (cancelled > 0) extra.push(`${cancelled} cancelled`);
      return extra.length ? `${ready}, ${extra.join(', ')} — see the Render tab.` : `${ready}.`;
    },
    OPEN_EXPORT_FOLDER: 'Open export folder',
    FAILED_STATUS: 'Could not finish the clips — the takes are still there for the Render tab.',
    failedToast: (err) => `Could not finish the clips automatically: ${err}. The takes are still on disk; render them from the Render tab.`,
    NOTHING_FOUND: 'No clips to finish: the Render tab found no takes it can use in this batch.',
    SKIPPED_CANCELLED: 'The batch was cancelled, so its clips were not finished. Render the takes from the Render tab if you want them.',
    SKIPPED_NO_EXPORT_DIR: 'Clips were not finished: add an export folder in Configuration → Render Settings first. The takes are still there for the Render tab.',
    SKIPPED_CUSTOM_ARGS: 'Clips were not finished: the Custom codec has no FFmpeg arguments. Add them in Configuration → Render Settings.',
  },
  // ── Keep free on the Windows drive (#1430) + windows_drive_setting.js ────
  WINDOWS_DRIVE: {
    KEEP_FREE_LABEL: 'Keep free on the Windows drive:',
    KEEP_FREE_TITLE:
        'Captures and renders skip the drive Windows is installed on once writing there would leave less than this free, so Windows keeps room for updates, its page file and temp files. A capture or export folder on that drive is used only while it has room above this; other drives are not affected. A change applies to the next render, even during a batch; a capture batch picks each clip\'s drive when it starts. 0 turns it off.',
    KEEP_FREE_UNIT: 'GB',
    // Start Capture Batch's banner when the setting is what's short.
    noneUsableWarning: (required, free, keepGb) =>
      `Capture needs ~${required} GB, and the only free space in the Capture Output folders is on the Windows drive: ${free} GB, below the ${keepGb} GB that "Keep free on the Windows drive" keeps free there. Lower that setting on Configuration → Destinations, or add a Capture Output folder on another drive.`,
    someUsableWarning: (required, free, usable, keepGb) =>
      `Capture needs ~${required} GB. The Capture Output folders have ${free} GB free, but only ${usable} GB of it can be used: "Keep free on the Windows drive" keeps ${keepGb} GB free on the Windows drive. Lower that setting on Configuration → Destinations, or add a Capture Output folder on another drive.`,
    // Appended to the "will make room" heads-ups, whose figure leaves it out.
    keptFreeNote: (keepGb) =>
      `That leaves out the ${keepGb} GB kept free on the Windows drive ("Keep free on the Windows drive", Configuration → Destinations).`,
  },
  // ── Deleting takes automatically (#644) + take_cleanup.js ────────────────
  TAKE_CLEANUP: {
    AFTER_LABEL: 'After a clip is finished:',
    AFTER_TITLE:
        'What happens to a take once its clip has rendered successfully. Only a take whose every render finished, with the clip file still there, is ever deleted. A failed, cancelled or unrendered take always stays. The render history stays either way.',
    AFTER_KEEP: 'Keep the take',
    AFTER_RECYCLE: 'Move the take to the Recycle Bin',
    AFTER_DELETE: 'Delete the take permanently',
    WHEN_NEEDED_LABEL: 'Only when space is needed',
    WHEN_NEEDED_TITLE:
        'Takes stay until a capture or a render batch needs room on its drive. Then the oldest finished takes there are deleted until it fits.',
    WHEN_NEEDED_OFF_TITLE:
        'Needs "Delete the take permanently": the Recycle Bin frees no space until you empty it.',
    HINT_KEEP: 'Takes stay on disk. Delete them from the Render tab when you are done.',
    HINT_RECYCLE:
        'Each take goes to the Recycle Bin once its clip renders. The Recycle Bin frees no space until you empty it.',
    HINT_DELETE: 'Each take is deleted for good once its clip renders. It cannot be restored.',
    HINT_DELETE_WHEN_NEEDED:
        'Takes stay until a capture or render batch is short of room. Then the oldest finished takes on that drive are deleted for good until it fits.',
    CONFIRM_TITLE: 'Delete takes permanently?',
    CONFIRM_MESSAGE:
        'Takes deleted this way skip the Recycle Bin and cannot be restored. Only takes whose clip rendered successfully are deleted, and the render history stays.',
    CONFIRM_BUTTON: 'Delete permanently',
    progressLine: (step, done, total) => {
      if (step === 'looking') return 'Making room: looking for finished takes';
      if (step === 'measuring') return `Making room: measuring finished takes (${done} of ${total})`;
      if (step === 'recycling') return `Moving finished takes to the Recycle Bin (${done} of ${total})`;
      return `Deleting finished takes (${done} of ${total})`;
    },
    /** `reason`: `finished`, `capture` or `finish`; `method`: `recycle` or `delete`. */
    doneToast: (n, size, reason, method) => {
      const takes = n === 1 ? 'a finished take' : `${n} finished takes`;
      if (method === 'recycle') return `Moved ${takes} to the Recycle Bin (${size}).`;
      if (reason === 'capture') return `Deleted ${takes} (${size}) to make room for the capture.`;
      if (reason === 'finish') return `Deleted ${takes} (${size}) to make room for the renders.`;
      return `Deleted ${takes} for good (${size}).`;
    },
    failedToast: (n, error) =>
      `Couldn't delete ${n === 1 ? 'a finished take' : `${n} finished takes`}: ${error}. Scan for Takes to list it again.`,
    shortToast: (reason, needed, available, movedTakes = 0) =>
      `${reason === 'capture' ? 'The capture needs' : 'The renders need'} ${needed} free on one drive, and deleting every finished take would leave at most ${available} on any one drive.${
        movedTakes === 1 ? ' 1 finished take stays because its clip isn\'t where it was rendered (moved or renamed).'
          : movedTakes > 1 ? ` ${movedTakes} finished takes stay because their clips aren't where they were rendered (moved or renamed).`
            : ''
      } No takes were deleted.`,
  },
  // ── Render while capturing (#861) ────────────────────────────────────────
  RENDER_WHILE_CAPTURING: {
    LABEL: 'Render while capturing',
    TITLE:
        'Each clip renders as soon as the batch has recorded it, instead of after the whole batch. A render slows the capture by however much this PC has to spare, so it is off by default. With "Delete the take permanently", a batch short of room waits between two clips until the renders have freed enough for the next one.',
    AT_ONCE_LABEL: 'Renders at once:',
    atOnceOption: (renders, percent) => `${renders} (about ${percent}% of the CPU)`,
    AT_ONCE_TITLE:
        'How many clips render at the same time while the batch captures, never more than the Render tab\'s own number. Each takes the share of the CPU one of the Render tab\'s renders gets (a quarter at 4 at once), so this is also how much of the CPU the renders take from the capture. One leaves most of the CPU idle, so the renders fall behind the capture; more keep up better but slow the capture down. The capture never drops frames, it only takes longer.',
    HINT_OFF: 'Clips render after the batch.',
    HINT_ON_KEEP:
        'Each clip renders while the batch goes on. The takes stay, so the batch still needs room for all of them.',
    HINT_ON_DELETE:
        'Each clip renders while the batch goes on, and its take is deleted once the clip is done. When the drive runs short, the batch waits between clips for the renders to free room.',
    holdingLine: (short) =>
      `Will wait after this clip for renders to free ${short || 'room'} for the next one`,
    heldLine: (short, minutes) =>
      `Waiting for renders to free ${short || 'room'} for the next clip${minutes ? ` (about ${minutes} min)` : ''}`,
    stoppedToast: (clip, reason) =>
      `The batch stopped before clip ${clip}: ${reason}. The clips recorded so far are kept.`,
    queueFailed: (err) => `Couldn't queue a recorded clip for rendering: ${err}`,
  },
  // ── FFmpeg Error Log modal ───────────────────────────────────────────────
  ERROR_LOG_MODAL: {
    TITLE_DEFAULT: 'FFmpeg Error Log',
    CLOSE_BUTTON: 'Close',
  },
  // ── Render batch crash-recovery modal ────────────────────────────────────
  RENDER_RECOVERY_MODAL: {
    TITLE: '🎬 Render Batch Interrupted',
    BODY: "The last render batch didn't finish cleanly (app closed or crashed mid-batch).",
    SOURCE_LABEL: 'Source:',
    COMPLETED_LABEL: '✅ Completed:',
    PENDING_LABEL: '⏳ Pending:',
    RECOVER_BUTTON: '🔄 Recover Render Batch',
    DISCARD_BUTTON: '🗑 Discard',
  },
  // ── Closing while renders or a combine run (render_close_prompt.js, #1439, #1496) ──
  RENDER_CLOSE_MODAL: {
    title: (running, combining) => (running === 0 && combining ? 'Combine running' : 'Renders running'),
    message: (running, combining = false) => {
      if (running === 0) {
        return 'A combine is running. Closing DoD Studio stops it and removes the unfinished video; a file already under its name stays as it was.';
      }
      const renders = `${running === 1 ? 'A render is' : `${running} renders are`} running`;
      if (combining) {
        return `${renders}, and a combine. Closing DoD Studio stops them and removes the unfinished files.`;
      }
      return `${renders}. Closing DoD Studio stops ${running === 1 ? 'it' : 'them'} and removes the unfinished clip${running === 1 ? '' : 's'}.`;
    },
    closeButton: (running, combining) => {
      if (running === 0) return 'Stop combine and close';
      return combining ? 'Stop all and close' : 'Stop renders and close';
    },
    KEEP_OPEN_BUTTON: 'Keep Studio open',
    logLine: (running, closed, combining = false) =>
      `Close asked while ${running} render(s)${combining ? ' and a combine' : ''} ran: ${closed ? 'closed, stopping them' : 'kept Studio open'}`,
  },
  // ── combine_clips.js: Render Studio's Combine Clips (#107) ──────────────
  COMBINE: {
    OPEN_BUTTON: 'Combine Clips…',
    OPEN_TITLE: 'Join rendered clips into one video',
    TITLE: 'Combine Clips',
    HINT: 'Join rendered clips into one video, in the order listed. Clips that all match are joined as they are, in seconds; mixed ones are fitted to the first clip and re-encoded.',
    ADD_FINISHED_BUTTON: 'Add finished renders',
    ADD_FILES_BUTTON: 'Add files…',
    ADD_FILES_TITLE: 'Choose clips to combine',
    CLEAR_BUTTON: 'Clear',
    VIDEO_FILTER: 'Video',
    MOVE_UP: 'Move up',
    MOVE_DOWN: 'Move down',
    REMOVE: 'Remove from the list',
    EMPTY: 'No clips yet. Add finished renders, or add files.',
    NO_FINISHED: 'No finished renders in this session yet.',
    NEED_TWO: 'Add at least two clips.',
    CHECKING: 'Checking the clips…',
    planCopy: (length) => `They match, so they're joined as they are: a few seconds, no quality lost. ${length} in all.`,
    planEncode: (length, width, height, fps) => `They differ in size, frame rate or format, so the video is re-encoded to MP4 at ${width}×${height}, ${fps} fps (the first clip's): this takes a while. ${length} in all.`,
    planFailed: (err) => `Can't combine these: ${err}`,
    START_BUTTON: 'Combine…',
    SAVE_TITLE: 'Save the combined video as',
    CANCEL_BUTTON: 'Cancel',
    CLOSE_BUTTON: 'Close',
    saved: (name) => `Saved ${name}.`,
    SHOW_FILE: 'Show',
    // combine_progress.js: the step line under the bar (#955).
    CHECKING_STEP: 'Checking the clips…',
    checkingLine: (n, total) => `Checking the clips (${n} of ${total})…`,
    joiningLine: (done, total) => `Joining clips: ${done} of ${total}`,
    savedIn: (elapsed) => `Saved in ${elapsed}.`,
    STOPPED: 'Cancelled. Nothing was saved.',
    FAILED: 'Stopped with an error (see the message).',
  },
};
