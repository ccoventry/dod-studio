// strings/capture.js: the app's text for running a capture batch: the footer, progress and what a batch leaves behind.
// Part of the STRINGS catalog that ../strings.js assembles (#604).

export const CAPTURE_STRINGS = {
  CAPTURE: {
    andNMore: (n) => `...and ${n} more`,
    AGR_FPS_REQUIRED_WARNING: 'AGR FPS is empty: set it on Configuration → Output Format (marked in red) before starting an AGR capture.',
    PATHS_MISSING_WARNING: 'Set hl.exe and hlae.exe on Configuration → Paths (marked in red) before starting a capture.',
    NO_HIGHLIGHTS_SELECTED_WARNING: 'No highlights selected — tick at least one in Highlight Details on the Highlights tab before starting a capture.',
    DEMOS_MISSING_NOT_STARTED: "Capture not started: a demo with picked highlights is missing. Use its row's Locate… button, or untick its highlights.",
    NO_DRIVES_CONFIGURED_WARNING: 'No Capture Output folder yet: add one on Configuration → Destinations before starting a capture.',
    OBS_NOT_CONNECTED_WARNING: 'Not connected to OBS — capture mode is OBS, but the last connection check failed. Fix the connection in Configuration → Output Format before starting a capture.',
    bannedCommandsWarning: (n) => `${n} command${n === 1 ? '' : 's'} in Initial or Scheduled Commands can't be used — fix or remove ${n === 1 ? 'it' : 'them'} in the Commands tab before starting a capture.`,
    // Measured 2026-08-28, see docs/direct_to_video_capture.md. Spelled out
    // because both halves report success and the broken output only shows up
    // after rendering — the user has no other way to find out.
    NO_USABLE_OUTPUT_FOLDER_WARNING: 'None of the Capture Output folders can be used: see the red rows in the list.',
    NO_USABLE_SPACE_WARNING: 'None of the configured Capture Output directories have any free space.',
    insufficientSpaceWarning: (required, available) => `Insufficient disk space: capture needs ~${required} GB, only ${available} GB available across the export pool.`,
    insufficientSpaceMakesRoom: (required, available) => `Capture needs ~${required} GB and ${available} GB is free: finished takes will be deleted to make room when the batch starts ("Only when space is needed").`,
    insufficientSpaceRendersMakeRoom: (required, available) => `Capture needs ~${required} GB and ${available} GB is free: the batch will wait between clips for renders to free room ("Render while capturing").`,
    partialProblemsWarning: (wontBeUsed) => `Some Capture Output ${wontBeUsed} (see the red rows in the list). Capture will proceed using the others.`,
    ENTRY_WONT_BE_USED_SINGULAR: "entry won't be used",
    ENTRY_WONT_BE_USED_PLURAL: "entries won't be used",
    willBeCreatedSingle: (path, doesnt) => `${path} ${doesnt} exist yet — it'll be created when the capture starts.`,
    willBeCreatedMultiple: (doesnt, list) => `These Capture Output entries ${doesnt} exist yet — they'll be created when the capture starts:\n${list}`,
    DOESNT_SINGULAR: "doesn't",
    DOESNT_PLURAL: "don't",
    DELETE_SELECTED_DEFAULT: 'Delete Selected',
    deleteNSelected: (n) => `Delete ${n} Selected`,
    NO_ORPHANED_PREVIEWS: 'No orphaned preview demos found.',
    SCANNING_ORPHANED_PREVIEWS: 'Scanning for orphaned preview demos...',
    SCAN_COMPLETE: 'Scan complete.',
    SCAN_FAILED: 'Scan failed.',
    scanFailedRow: (e) => `Scan failed: ${e}`,
    CONFIGURE_HL_PATH_FIRST: 'Configure the Half-Life Executable (hl.exe) path before auditing previews.',
    deletePreviewsConfirm: (n) => `Permanently delete ${n} orphaned preview demo(s)?`,
    deletedPreviews: (n) => `Deleted ${n} orphaned preview demo(s).`,
    deletionFailed: (e) => `Deletion failed: ${e}`,
    CAPTURING_ELLIPSIS_DEFAULT: 'Capturing...',
    // #643: the batch's time left, after the status line.
    timeLeftMinutes: (minutes) => ` · about ${minutes} min left`,
    TIME_LEFT_UNDER_A_MINUTE: ' · under a minute left',
    captureErrorToast: (status) => `Capture error: ${status}`,
    CAPTURE_ERROR_STATUS_DEFAULT: 'Unknown error',
    captureErrorStatusText: (status) => `Error: ${status}`,
    CAPTURE_ERROR_TEXT_DEFAULT: 'Capture failed',
    CANCELLED: 'Cancelled',
    BATCH_CANCELLED_TOAST: 'Batch capture cancelled.',
    BATCH_COMPLETED_TOAST: 'Batch capture completed successfully!',
    // A batch that ran to its end but left clips uncaptured, or short (#1401).
    batchFinishedWithFailures: (failed, total) => `Batch capture finished, but ${failed} of ${total} clip${total === 1 ? '' : 's'} failed: not marked Captured. See Last Batch and the activity log.`,
    batchFinishedShort: (short, total) => `Batch capture finished: ${short} of ${total} clip${total === 1 ? ' is' : 's are'} shorter than planned. Check ${short === 1 ? 'it' : 'them'} before using ${short === 1 ? 'it' : 'them'}.`,
    // The highlights of the clips the game reported as failed or short (#1401).
    clipsFailed: (labels, more) => `Failed in the game (nothing usable recorded), not marked Captured: ${labels.join('; ')}${more > 0 ? ` and ${more} more` : ''}.`,
    clipsShort: (labels, more) => `Shorter than planned or missing a kill, marked Captured, check before using: ${labels.join('; ')}${more > 0 ? ` and ${more} more` : ''}.`,
    // A clip the batch's end cut short (#1081) left a partial take: it isn't
    // missing. `cutBy` says what ended it: Cancel, the game closing, or an error.
    // A take whose video stopped early (#1105) is partial too: not missing either.
    // Nor is a clip the batch ended before reaching (#1434): `endedBy` says why.
    // The counts are takes_shortfall_toast.js's `takeShortfall`.
    takesFoundMissing: (captured, total, { missing = 0, cutShort = 0, cutBy, stoppedEarly = 0, startedLate = 0, failed = 0, notReached = 0, endedBy } = {}) => {
      const cutWhen = { game_closed: 'when the game closed', stopped: 'when the batch stopped', studio_closed: 'when DoD Studio closed' }[cutBy] || 'by Cancel';
      const endedWhy = { game_closed: 'the game closed', stopped: 'the batch stopped', studio_closed: 'DoD Studio closed' }[endedBy] || 'batch cancelled';
      const parts = [];
      if (missing > 0) parts.push(`${missing} missing`);
      if (notReached > 0) parts.push(`${notReached} not reached (${endedWhy})`);
      if (cutShort > 0) parts.push(`${cutShort} cut short ${cutWhen} (partial take, not captured)`);
      if (stoppedEarly > 0) parts.push(`${stoppedEarly} with video that stops early (partial take, not captured)`);
      // OBS started recording after the clip's first frame (#1109).
      if (startedLate > 0) parts.push(`${startedLate} OBS started recording late (missing the clip's opening, not captured)`);
      // The game recorded no start, no stop or no frames (#1401).
      if (failed > 0) parts.push(`${failed} failed in the game (not captured)`);
      return `${captured}/${total} takes found on disk — ${parts.join(', ')}.`;
    },
    takesRenderStudioMiss: (captured, total, missingRender) => `${captured}/${total} takes captured, but ${missingRender} won't be seen by Render Studio.`,
    allTakesVerified: (total) => `All ${total} takes verified on disk.`,
    highlightsMarkedCaptured: (n) => ` ${n} highlight(s) marked Captured.`,
    BOTH_PATHS_REQUIRED: 'Set hl.exe and hlae.exe on Configuration → Paths first.',
    NO_CAPTURE_OUTPUT_DIR: 'Add a Capture Output folder on Configuration → Destinations first.',
    NO_CAPTURE_OUTPUT_DIR_WITH_SPACE: 'Configure at least one Capture Output directory with free space before starting a capture.',
    insufficientDiskSpaceToast: (required, available) => `Insufficient disk space. Required: ${required} GB, Available: ${available} GB`,
    INITIALIZING_CAPTURE_BATCH: 'Initializing capture batch...',
    BATCH_IN_OPEN_GAME_TOAST: 'Running the batch in the game that is already open. It stays open afterwards.',
    startBatchError: (err) => `Error starting batch: ${err}`,
    CANCELLING_BATCH_TOAST: 'Cancelling batch...',
    EMPTY_DASH: '—',
    megabytesLabel: (mb) => `${mb} MB`,
  },
  // ── Closing Studio while a capture batch runs (batch_close_prompt.js, #545) ──
  BATCH_CLOSE_MODAL: {
    TITLE: 'Capture batch running',
    MESSAGE: 'A capture batch is still running. If you close DoD Studio, the game keeps capturing on its own. Studio checks the takes and marks them Captured the next time it opens. To stop the batch instead, use Cancel Batch first.',
    CLOSE_BUTTON: 'Close DoD Studio',
    KEEP_OPEN_BUTTON: 'Keep Studio open',
    // The batch's start has not reached the game yet (#545): Studio waits for
    // it before asking, so the game does not sit at its main menu.
    WAIT_TITLE: 'Closing DoD Studio',
    starting: (secondsLeft) => `The game is still starting. Studio is starting the batch in it first, so it can keep capturing after Studio closes… (up to ${secondsLeft} s)`,
    NOT_STARTED_TITLE: 'Capture batch not started',
    NOT_STARTED_MESSAGE: 'The batch has not started in the game yet, so the game cannot capture it on its own. Closing DoD Studio cancels the batch and closes the game.',
    CANCEL_AND_CLOSE_BUTTON: 'Cancel batch and close',
    CANCELLING: 'Cancelling the batch and closing the game…',
  },
  // ── What a running batch holds back (batch_guard.js, #1085) ──
  BATCH_GUARD: {
    HELD_TITLE: 'A capture batch is running. Wait for it to finish, or cancel it.',
    REFUSED_TOAST: 'A capture batch is running: this project stays open until it ends.',
  },
  // ── A batch that ran while Studio was closed (batch_catch_up.js, #545) ──
  BATCH_CATCH_UP: {
    caughtUp: (captured, total, marked, rendered = 0) => `A capture batch ran while DoD Studio was closed: ${captured} of ${total} clips are on disk.`
      + (marked > rendered ? ` ${marked - rendered} highlight(s) marked Captured.` : '')
      + (rendered > 0 ? ` ${rendered} highlight(s) marked Rendered: their clips were finished before it closed.` : ''),
    projectUpdated: (name) => `Marked them in the project ${name}.`,
    SAVED_OPEN_PROJECT: 'Saved them in the open project.',
    projectNotUpdated: (name, err) => `Could not mark them in the project ${name}: ${err}`,
    checkFailed: (err) => `Could not check the clips of a batch that ran while DoD Studio was closed: ${err}`,
    // The game is open, so the batch may still be capturing in it (#20).
    WAITING_FOR_GAME: 'The game may still be capturing a batch from before DoD Studio closed. Studio checks its clips and marks them Captured once the game closes.',
    STILL_RUNNING_NOT_STARTED: 'Not started: the game may still be capturing a batch from before DoD Studio closed. Wait for the game to close, or close it, and start again.',
  },
  // ── Clips a batch never got to, offered after catching up (batch_resume.js, #20) ──
  BATCH_RESUME: {
    TITLE: 'Capture the rest?',
    message: (captured, total, left) => (captured === 0
      ? `A capture batch stopped before the end while DoD Studio was closed, and none of its ${total === 1 ? 'clip is' : `${total} clips are`} on disk.`
      : `A capture batch stopped before the end while DoD Studio was closed. ${captured} of ${total} clips are on disk and marked Captured; ${left === 1 ? '1 clip was' : `${left} clips were`} not captured.`),
    inProject: (name) => `Capture them now? Studio opens the project ${name}, ticks just those highlights and starts a capture batch.`,
    IN_QUEUE: 'Capture them now? Studio ticks just those highlights and starts a capture batch.',
    CAPTURE_BUTTON: 'Capture them now',
    NOT_NOW_BUTTON: 'Not now',
    NONE_FOUND: 'Could not find those highlights in the project, so no batch was started.',
  },
  // ── Highlights unticked once a batch has captured them (batch_untick.js) ──
  BATCH_UNTICK: {
    logLine: (unticked, shortLeft) => `Capture batch ended: unticked ${unticked} captured highlight(s), left ${shortLeft} short one(s) ticked.`,
  },
  // #443: the capture summary strip above Start Capture Batch.
  CAPTURE_SUMMARY: {
    MODE_FRAMES: 'Frame sequence',
    modeVideo: (codec) => (codec ? `Video · ${codec}` : 'Video'),
    modeObs: (fps) => `OBS @ ${fps} fps`,
    modeAgr: (fps) => (fps > 0 ? `AGR for Blender @ ${fps} fps` : 'AGR for Blender (AGR FPS not set)'),
    format: (w, h, fps) => `${w}×${h} @ ${fps} fps`,
    commands: (name, edited) => `Commands: ${name}${edited ? ' (edited)' : ''}`,
    COMMANDS_NONE: 'Commands: none',
    scheduled: (n) => (n === 0 ? 'No scheduled commands' : `${n} scheduled command${n === 1 ? '' : 's'}`),
    banned: (n) => `${n} banned command${n === 1 ? '' : 's'}`,
    DECALS_CLEARED: 'Decals cleared',
    DECALS_KEPT: 'Decals kept',
    finish: (codec) => (codec ? `Finish: ${codec}` : 'Finish: on'),
    FINISH_OFF: 'Finish: off',
    FINISH_KEEP: 'Keep as captured',
    NO_DESTINATION: 'No destination folder',
    LINK_TITLE: 'Open this setting in Configuration',
  },
  // ── batch_results.js: the Last Batch panel (#172) ──────────────────────
  BATCH_RESULTS: {
    title: (time) => `Last batch, ended ${time}`,
    DISMISS: 'Hide until the next batch',
    outcome: (kind, text) => ({
      completed: 'Completed.',
      cancelled: 'Cancelled.',
      error: `Stopped: ${text || 'see the activity log'}.`,
    })[kind] || '',
    totals: (captured, takes, size, notRenderable) => `${captured} of ${takes} take${takes === 1 ? '' : 's'} on disk, ${size}${notRenderable > 0 ? `; ${notRenderable} Render Studio can't use yet` : ''}.`,
    CHECKING: 'Checking the takes on disk…',
    kills: (n) => `${n} kill${n === 1 ? '' : 's'}`,
    merged: (n) => `(+${n} more, recorded as one take)`,
    // A take that recorded its long gaps between kills sped up (#831).
    rampedGaps: (n, speed) => `(${n === 1 ? 'gap' : `${n} gaps`} at ${speed}x)`,
    STATUS: {
      ok: 'Captured',
      unrenderable: "Captured, can't render yet",
      missing: 'Missing',
      cut_short: 'Cut short by Cancel (partial take, not captured)',
      short: 'Video stops early (partial take, not captured)',
      // What the game reported about the clip (#1401).
      failed: 'Failed in the game: nothing usable recorded (not captured)',
      late: 'OBS started late (missing the opening, not captured)',
    },
    // A clip the batch's end cut short, by what ended it (#1081).
    cutShortBy: (cutBy) => ({
      game_closed: 'Cut short: the game closed (partial take, not captured)',
      stopped: 'Cut short: the batch stopped (partial take, not captured)',
      // An OBS recording nobody stopped: Studio closed mid-clip (#20).
      studio_closed: 'Cut short: DoD Studio closed while OBS recorded it (not captured)',
    })[cutBy] || 'Cut short by Cancel (partial take, not captured)',
    // A clip the batch ended before reaching (#1434), by what ended it.
    notReachedBy: (endedBy) => ({
      game_closed: 'Not reached: the game closed first',
      stopped: 'Not reached: the batch stopped first',
      studio_closed: 'Not reached: DoD Studio closed first',
    })[endedBy] || 'Not reached: the batch was cancelled first',
    // A take whose video FFmpeg stopped writing mid-clip, against its audio (#1105).
    shortTake: (video, audio) => `Video stops early: ${video.toFixed(1)} s of ${audio.toFixed(1)} s (partial take, not captured)`,
    // Why the game's clip failed (#1401).
    clipFailed: (why) => ({
      no_start: 'Failed: the game never started recording it (not captured)',
      no_stop: 'Failed: the game never stopped recording it (not captured)',
      no_frames: 'Failed: the game recorded no frames (not captured)',
    })[why] || 'Failed in the game: nothing usable recorded (not captured)',
    // A take shorter than its plan, or missing a kill: still captured (#1401).
    clipShort: (recorded, planned, killsOutside) => killsOutside > 0
      ? `Captured, but missing ${killsOutside} kill${killsOutside === 1 ? '' : 's'}: ${recorded.toFixed(2)} s of ${planned.toFixed(2)} s planned`
      : `Captured, but short: ${recorded.toFixed(2)} s of ${planned.toFixed(2)} s planned`,
    // A take OBS started recording after its clip's first frame (#1109).
    startedLate: (ms) => `OBS started ${ms} ms late: missing the clip's opening (not captured)`,
    OPEN_FOLDER: 'Open folder',
    // OBS mode's per-take frame loss (#172).
    obsCell: (skippedPct, missedPct) => `OBS: ${skippedPct}% skipped${missedPct >= 0.1 ? `, ${missedPct}% missed` : ''}`,
    obsCellTitle: (skipped, frames, missed, rendered) => `OBS left out ${skipped} of ${frames} frames because the encoder couldn't keep up, and ${missed} of ${rendered} because it couldn't draw them in time. A few tenths of a percent is normal; past 5% the take looks choppy.`,
    obsSummary: (lagging, clips, fix) => `OBS dropped frames in ${lagging} of ${clips} take${clips === 1 ? '' : 's'}. ${fix}`,
  },
  // ── obs_drop_rate.js: OBS mode's live drop rate (#172) ─────────────────
  OBS_DROP_RATE: {
    line: (clip, skippedPct, missedPct, recording) => `OBS, clip ${clip}${recording ? '' : ' done'}: ${skippedPct}% of frames skipped, ${missedPct}% missed`,
    LINE_TITLE: "Frames OBS left out of this clip so far. Skipped: the encoder couldn't keep up. Missed: OBS couldn't draw them in time. A few tenths of a percent is normal; past 5% the clip looks choppy.",
    lagging: (pct, encoder, fix) => `OBS is dropping ${pct}% of frames (${encoder ? "the encoder can't keep up" : "OBS can't draw them in time"}): the clip will be choppy. ${fix}`,
    LAGGING_TITLE: 'Over the last 5 seconds. Each clip\'s total is in Last Batch when the batch ends.',
    fix: (fps) => (fps > 60
      ? 'Lower OBS Capture FPS to 60 (Configuration → Output Format) and capture again.'
      : 'Lower OBS Capture FPS or the resolution (Configuration → Output Format) and capture again.'),
  },
  // ── capture_precheck.js: the Capture footer's line while Start Capture
  //    Batch checks everything before the batch starts (#953)
  CAPTURE_PRECHECK: {
    STARTING: 'Getting ready to capture…',
    OBS: 'Checking OBS…',
    DISK: 'Checking the disk space…',
    DEMOS: 'Checking the picked demos are where they were…',
    demosCounted: (done, total) => `Checking the picked demos are where they were (${done} of ${total})…`,
    MAPS: 'Checking the maps and demos for known problems…',
    PATHS: 'Checking the game and HLAE paths…',
    GAME: 'Checking whether the game is already open…',
    STEAM: 'Checking Steam…',
    NOT_STARTED: 'Not started.',
    GAME_OPEN: 'Not started yet: the game is already open.',
    MAP_VERSIONS: 'Not started: some demos would play on another version of their map.',
  },
  // ── capture_progress.js: the capture batch's footer line and bar (#952)
  CAPTURE_PROGRESS: {
    // A clip not captured in a batch that ran to its end failed; a short one
    // is captured but shorter than planned (#1401).
    done: (duration, captured, total, short = 0) => {
      if (total == null) return `Done in ${duration}`;
      const clips = total === 1 ? 'clip' : 'clips';
      const head = captured === total
        ? `Done in ${duration}: ${total} ${clips} captured`
        : `Done in ${duration}: ${captured} of ${total} ${clips} captured, ${total - captured} failed`;
      return short > 0 ? `${head}, ${short} shorter than planned` : head;
    },
    startLog: () => '[capture] started',
    // Same wording as the footer: failed only for a batch that ran to its end.
    endLog: (outcome, elapsed, captured, total, short = 0) => {
      const failed = outcome === 'completed' && total != null ? total - captured : 0;
      const counts = total == null ? '' : `: ${captured} of ${total} clip(s) captured`;
      return `[capture] ${outcome} after ${elapsed}${counts}${failed > 0 ? `, ${failed} failed` : ''}${short > 0 ? `, ${short} shorter than planned` : ''}`;
    },
  },
};
