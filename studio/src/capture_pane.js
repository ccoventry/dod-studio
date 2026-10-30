import { ensureSteamReady } from './steam_guard.js';
import { startCaptureBatch, cancelCaptureBatch, validatePaths, calculateExportPoolSpace, diagnoseCaptureOutputPaths, scanOrphanedPreviews, deleteOrphanedPreviews, checkEngineProcesses, runningGameForBatch, launchStandaloneGame, launchObs, obsTestConnection, readCfgCommands, logFrontendEvent } from './ipc_bridge.js';
import { listen } from '@tauri-apps/api/event';
import { pickPath } from './pick_path.js';
import { themedConfirm } from './themed_confirm.js';
import { closeGameWithOtherSettings } from './running_game_guard.js';
import { HIGHLIGHT_STATUS } from './status_colors.js';
import { confirmCrashMaps } from './crash_map_warnings.js';
import { confirmOverLimit } from './packet_entity_limit.js';
import { confirmNoCameraData } from './camera_data.js';
import { batchStarted, batchEnded, batchVerified } from './batch_results.js';
import { showToast } from './toast.js';
import { requestProcessGuardedLaunch } from './detail_pane.js';
import { createListEditor } from './list_editor.js';
import { attachCommandSuggest } from './command_suggest.js';
import { refreshCfgWarnings, bannedCommandCount } from './cfg_warnings.js';
import { isObsConnected, obsConnectionChecked, setObsConnected } from './obs_status.js';
import { refreshRollFloors } from './roll_floors.js';
import { streakUid, recordTake, setVerifiedStatus, isSkipped, markBlockCaptured } from './take_index.js';
import { STRINGS } from './strings.js';
import { notify, isNotificationEnabled } from './os_notifications.js';
import { numberField } from './number_field.js';
import { requestFinishClips, finishSummary } from './finish_clips.js';
import { syncCommandProfileRow, getAllCommandProfiles, getActiveCommandProfile } from './command_profiles_ui.js';
import { profileState } from './command_profiles.js';
import { initCaptureSummary, renderCaptureSummary } from './capture_summary_ui.js';
import { computeRequiredCaptureBytes, bytesPerFrameFor, obsBytesPerSecond } from './capture_estimate.js';
import { readKillGaps, estimateKillGaps, wireKillGaps } from './kill_gaps_setting.js';
import { setStatusLine, uiStatusText } from './status_line.js';
import { createCapturePrecheck } from './capture_precheck.js';
import { createCaptureProgress, endKind } from './capture_progress.js';
import { startObsDropRate } from './obs_drop_rate.js';
import { refreshAfterTyping } from './input_refresh.js';
import { isObsTestBatch } from './obs_test_flag.js';
import { takeCleanupMakesRoom } from './take_cleanup.js';
import { rendersMakeRoom } from './render_while_capturing.js';
import { isBatchStillRunningInGame } from './batch_catch_up.js';

let listeningForExternalErrors = false;
let unlistenCaptureStatus = null;
let unlistenDemoLoading = null;
let unlistenFastForwardToClip = null;
let unlistenPatchingStarted = null;
let unlistenPatchingFinished = null;
// Tracks whether a batch is actively running so refreshLaunchGuard() never
// re-enables Start Capture out from under the capture_status "running" lock.
let capturingInFlight = false;
// Start Capture's checks before the batch (#953): while they run,
// refreshLaunchGuard() leaves Start greyed out too.
let precheck = null;
// The batch's footer line and bar (#952), with its time left (#643).
let captureProgress = null;

/** Whether a capture batch is running right now (#545's close prompt). */
export function isCaptureRunning() {
  return capturingInFlight;
}

// Start Capture Batch's click, once initCaptureUI has wired it.
let startCaptureClick = null;

/** Runs Start Capture Batch as its click does, every check included, for a
 *  batch Studio offers itself (batch_resume.js, #20). */
export function startCaptureFromOffer() {
  return startCaptureClick ? startCaptureClick() : Promise.resolve();
}
// getState callback captured from initCaptureUI() so refreshLaunchGuard()
// can be called with no args from other panes (e.g. main.js after a target
// drive is added, or detail_pane.js after a streak selection changes).
let currentGetState = null;
// onSettingsChange callback captured from initCaptureUI() — main.js's
// persistAppSettings, wired here so Timing Options fields and Init/Custom
// Commands actually get written to settings.json on edit instead of only
// being saved incidentally whenever some unrelated action (e.g. browsing
// for hlae.exe) happens to also call it.
let currentOnSettingsChange = null;
// Fired after a verified capture advances highlight statuses, so the panes
// that render status (Master Queue counts, Highlight Details rows) re-render.
// Neither watches the streak objects, so without this the tables stay stale
// until the next unrelated interaction.
let currentOnStatusChange = null;
// Returns the live project-level take index object (take_key -> uid[]) owned
// by main.js, so recording into it here persists into the same object that
// gets serialized on Save Session. Null until main.js wires one up.
let currentGetTakeIndex = null;
// Fired once per batch when capture_status reports it's no longer running
// (finished, cancelled, or errored) — folder_pools.js's updateExportPoolIndicator,
// so the footer's "Capture Output Free" figure reflects what the batch just
// wrote. Previously only refreshLaunchGuard() ran here, which reads its own
// separate availableBytes for the launch-guard banner and never touched the
// footer, so it stayed stuck at its last edit-time value until something
// unrelated (editing the drive list, restarting) happened to refresh it.
// See issue #13.
let currentOnBatchFinished = null;
// The most recent batch dispatched from this window: its session id and the
// live streak objects, in the exact order they were sent. The backend's take
// manifest indexes into that same order, which is what lets a verified block
// resolve back to the highlights it actually recorded.
let lastDispatch = null;
let unlistenTakesVerified = null;

function notifySettingsChange() {
  if (currentOnSettingsChange) currentOnSettingsChange();
}

/**
 * Settings the pipeline turns into init commands of its own, appended after
 * the user's own list. Capture FPS becomes `mirv_movie_fps <n>`, and the
 * decal flush contributes the `r_decals` pin — so each can displace a value
 * from a config file, and the warning banner is stale until it is told one
 * changed.
 *
 * Keyed by element id rather than by tab, deliberately: what makes these
 * special is that they become commands, not where they happen to sit.
 */
const FIELDS_THAT_BECOME_COMMANDS = [
  '#config-capture-fps',
  '#config-decal-flush',
];

// Illustrative gap between the two kill anchors in the timing diagram below
// — a real streak's length varies per capture; this is just enough to make
// Start Lead/Stop Trail visually distinct from Pre-roll/Post-roll.
const TIMING_DIAGRAM_ILLUSTRATIVE_STREAK_SECONDS = 8.0;

/** Seconds of demo per frame while a batch fast-forwards (`host_framerate`),
 *  the settings' `fast_forward_speed`. Fixed, and no longer shown: batches
 *  skip straight to each clip, and fast-forward is only their fallback. */
export const FAST_FORWARD_STEP = 0.05;
let fastForwardStep = FAST_FORWARD_STEP;
/** The saved value, kept as it is (settings_sync.js sets it on load). */
export function setFastForwardStep(value) {
  fastForwardStep = Number.isFinite(value) && value > 0 ? value : FAST_FORWARD_STEP;
}
export function getFastForwardStep() {
  return fastForwardStep;
}

/**
 * Rebuilds the Timings tab's execution-timeline table (#150) — revives the
 * pre-Tauri egui build's "Mock Execution Timeline" (native/src/bin/gui/views/
 * capture/panels.rs, dropped in the egui purge): a sorted list of events at
 * their time relative to the first kill (0.0s), not a to-scale bar. Called
 * once at init and on every `input` event from the five timing fields.
 */
export function renderTimingDiagram() {
  const preRoll = numberField('#config-pre-roll', 0);
  const postRoll = numberField('#config-post-roll', 0);
  const startLead = numberField('#config-record-start-lead', 0);
  const stopTrail = numberField('#config-record-stop-trail', 0);
  const initialDelay = numberField('#config-initial-delay', 0);
  const streak = TIMING_DIAGRAM_ILLUSTRATIVE_STREAK_SECONDS;

  // Initial Delay only applies to patched demos: with Seek Between Clips on,
  // the hook waits for the demo to load and moves to clip 1 straight away
  // (native::patch::runtime_batch), so the field and its note are hidden.
  const seeks = document.querySelector('#config-hook-runs-batch')?.checked ?? true;
  const delayGroup = document.querySelector('#config-initial-delay-group');
  if (delayGroup) delayGroup.style.display = seeks ? 'none' : '';
  const note = document.querySelector('#timing-diagram-initial-delay-note');
  if (note) {
    note.textContent = seeks ? '' : STRINGS.CAPTURE_CONFIG.timingDiagramInitialDelayNote(initialDelay);
    note.style.display = seeks ? 'none' : '';
  }

  const C = STRINGS.CAPTURE_CONFIG;
  const events = [
    { time: -(startLead + preRoll), label: C.timingDiagramPreRollEvent(preRoll), anchor: false },
    { time: -startLead, label: C.timingDiagramRecordStartEvent(startLead), anchor: false },
    { time: 0, label: C.TIMING_DIAGRAM_FIRST_KILL_EVENT, anchor: true },
    { time: streak, label: C.TIMING_DIAGRAM_LAST_KILL_EVENT, anchor: true },
    { time: streak + stopTrail, label: C.timingDiagramRecordStopEvent(stopTrail), anchor: false },
    { time: streak + stopTrail + postRoll, label: C.timingDiagramPostRollEvent(postRoll), anchor: false },
  ].sort((a, b) => a.time - b.time);

  const tbody = document.querySelector('#timing-diagram-events');
  if (!tbody) return;
  tbody.innerHTML = events.map((e) => `
    <tr${e.anchor ? ' class="timing-diagram-anchor"' : ''}>
      <td class="timing-diagram-time">${C.timingDiagramTime(e.time)}</td>
      <td>${e.label}</td>
    </tr>`).join('');
}

/**
 * Re-check the game's config files against the commands a capture would apply.
 *
 * Passes the settings that decide what the pipeline appends for itself — the
 * movie fps, the decal pin — so the overrides reported are the ones a real
 * capture would really perform, not just the ones typed by hand.
 * `movie.cfg` setting `mirv_movie_fps 300` against a capture configured for 120
 * is a collision nobody typed and nobody would otherwise see.
 *
 * Called by main.js once persisted settings have landed in the DOM, whenever
 * the hl.exe path changes, by the command editors on every edit, and by the
 * settings below that the pipeline turns into commands of its own.
 */
/** Seek Between Clips (Pipeline tab): the hook runs both command lists, past
 *  the game's demo filter. Off, a patched demo carries them through it. */
function seeksBetweenClips() {
  return document.querySelector('#config-hook-runs-batch')?.checked ?? true;
}

export async function refreshInitCommandWarnings() {
  const gamePath = document.querySelector('#hl-path-input')?.value?.trim() || '';
  await refreshCfgWarnings(
    gamePath,
    initCommands.map((c) => c.trim()).filter((c) => c.length > 0),
    // Relation and offset travel with the command: they decide which one the
    // engine reaches first, and only the first to touch a cvar displaces what
    // the configs left it at.
    customCommands
      .filter((c) => (c?.command || '').trim())
      .map((c) => ({
        command: c.command.trim(),
        relation: c.relation === 'After' ? 'After' : 'Before',
        offset_seconds: Number(c.offsetSeconds) || 0,
      })),
    {
      captureFps: parseInt(document.querySelector('#config-capture-fps')?.value, 10) || null,
      decalFlush: document.querySelector('#config-decal-flush')?.checked ?? true,
      // Decides whether the game's demo filter applies to these commands.
      hookRunsBatch: seeksBetweenClips(),
    }
  );
  // A banned command appearing or disappearing has to reach Start Capture
  // Batch immediately, not wait for some unrelated field to trigger the next
  // refreshLaunchGuard() — this scan is the only thing that would ever know.
  refreshLaunchGuard();
}

/** Generates a `session_YYYYMMDD_HHMMSS` id so each batch routes into its own
 *  output subfolder instead of colliding in the export root (mirrors dev's
 *  `chrono::Local::now().format("%Y%m%d_%H%M%S")` stamp in widgets.rs). */
function generateSessionId() {
  const now = new Date();
  const pad = (n) => String(n).padStart(2, '0');
  return `session_${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}_${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
}

// ── Pre-Flight Disk Space Estimator ───────────────────────────────────────────

const PATH_PROBLEM_REASONS = {
  not_absolute: STRINGS.CAPTURE.pathProblem.notAbsolute,
  malformed: STRINGS.CAPTURE.pathProblem.malformed,
  not_found: STRINGS.CAPTURE.pathProblem.notFound,
  not_a_directory: STRINGS.CAPTURE.pathProblem.notADirectory,
};

const MAX_PROBLEM_PATHS_SHOWN = 3;

/** Turns a list of non-"ok" diagnostics into a point-form list of reasons
 *  (one bullet per line, `warningEl`'s `white-space: pre-line` renders the
 *  `\n`s), capped so a long invalid list doesn't turn into a wall of text.
 *  Falls back to a single plain sentence when there's just one problem. */
function describeProblemPaths(problems) {
  const shown = problems.slice(0, MAX_PROBLEM_PATHS_SHOWN)
    .map((d) => PATH_PROBLEM_REASONS[d.status]?.(d.path) || STRINGS.CAPTURE.pathProblem.unusable(d.path));
  const remaining = problems.length - MAX_PROBLEM_PATHS_SHOWN;
  if (remaining > 0) shown.push(STRINGS.CAPTURE.andNMore(remaining));
  return shown.length === 1 ? shown[0] : shown.map((s) => `• ${s}`).join('\n');
}

/**
 * Runs `obs_test_connection` and renders the result — the manual Test
 * Connection button's own logic, extracted so it can also be run
 * automatically when actively switching into OBS mode (main.js), and as
 * Start Capture Batch's own pre-flight below. Deliberately NOT run at app
 * startup even when OBS mode is already the persisted choice — OBS is the
 * user's own program, not expected to already be running just because
 * dod-studio opened, same as HLAE.
 *
 * `auto` skips the button disable/relabel churn (there was no click to
 * originate it) but still populates the same status panel — quiet the same
 * way `obs_check_orphan` is quiet at startup: "OBS is not running yet" is
 * the ordinary case here, not something to interrupt the user over.
 *
 * Lives here rather than main.js (where the OBS Connection UI is otherwise
 * wired up) specifically so Start Capture Batch's click handler below can
 * call it directly — main.js already imports from this module, so importing
 * this back the other way would be circular.
 */
export async function runObsConnectionTest({ auto = false } = {}) {
  const btn = document.querySelector('#obs-test-btn');
  const status = document.querySelector('#obs-status');
  if (!status) return;
  const label = btn ? btn.textContent : '';
  if (!auto && btn) { btn.disabled = true; btn.textContent = STRINGS.CAPTURE_CONFIG.OBS_TESTING; }
  status.style.display = '';
  status.textContent = STRINGS.CAPTURE_CONFIG.OBS_TESTING;
  try {
    const report = await obsTestConnection({
      host: document.querySelector('#config-obs-host')?.value?.trim() || '127.0.0.1',
      port: parseInt(document.querySelector('#config-obs-port')?.value, 10) || 4455,
      password: document.querySelector('#config-obs-password')?.value || '',
      gameWidth: numberField('#config-res-width', 1280, { integer: true, positive: true }),
      gameHeight: numberField('#config-res-height', 720, { integer: true, positive: true }),
      obsCaptureFps: numberField('#config-obs-capture-fps', 120, { integer: true, positive: true }),
      obsRecordingQuality: document.querySelector('#config-obs-recording-quality')?.value || '',
    });
    renderObsReport(report);
  } catch (e) {
    // Every invoke needs this: without it a Rust-side failure is swallowed
    // and the button simply appears to do nothing.
    status.textContent = STRINGS.CAPTURE_CONFIG.obsTestFailed(e);
    setObsConnected(false);
  } finally {
    if (!auto && btn) { btn.disabled = false; btn.textContent = label || STRINGS.CAPTURE_CONFIG.OBS_TEST_BUTTON; }
    // Whatever this check found, Start Capture Batch's gate (issue #147)
    // needs to reflect it immediately, not wait for some unrelated field to
    // trigger the next refreshLaunchGuard().
    refreshLaunchGuard();
  }
}

/**
 * Renders an `obs_test_connection` result into the status row.
 *
 * Warnings are shown rather than swallowed: the canvas mismatch in particular
 * costs most of the picture's detail and has no visible symptom, so it would
 * otherwise be found only by comparing a finished clip against expectations.
 * Should be rare now that `obs_test_connection` provisions dod-studio' own
 * profile/scene itself rather than validating whatever the user picked, but
 * still worth surfacing if OBS itself refuses one of those settings.
 */
function renderObsReport(report) {
  const status = document.querySelector('#obs-status');
  if (!status) return;
  status.style.display = '';

  setObsConnected(!!report?.connected);

  if (!report?.connected) {
    status.textContent = report?.error || STRINGS.CAPTURE_CONFIG.OBS_UNREACHABLE;
    return;
  }

  const lines = [
    STRINGS.CAPTURE_CONFIG.obsConnectedSummary(report.obs_version, report.websocket_version),
    // Read-only — dod-studio always targets its own fixed profile/scene now,
    // there is nothing here for the user to pick.
    STRINGS.CAPTURE_CONFIG.obsUsingSummary(report.current_profile, report.current_scene),
    STRINGS.CAPTURE_CONFIG.obsCanvasSummary(report.canvas, report.output, report.fps),
    STRINGS.CAPTURE_CONFIG.obsRecordingToSummary(report.record_directory),
  ];
  if (report.missing_requests?.length) {
    lines.push(STRINGS.CAPTURE_CONFIG.obsMissingRequests(report.missing_requests));
  }
  if (report.recording) lines.push(STRINGS.CAPTURE_CONFIG.OBS_ALREADY_RECORDING);
  if (report.streaming) lines.push(STRINGS.CAPTURE_CONFIG.OBS_ALREADY_STREAMING);
  for (const w of report.warnings || []) lines.push(w);
  status.textContent = lines.join('\n');
}

// obs-websocket does not start accepting connections the instant OBS's
// process starts — OBS has its own window/plugin init to get through first
// — so a single immediate retry after launching would still usually find it
// unreachable. Polls instead of guessing a single fixed wait, since actual
// startup time depends entirely on the machine and how many plugins/scenes
// OBS has to load; not a measured constant like this codebase's other
// timing figures, just a reasonable ceiling.
const OBS_LAUNCH_RETRY_ATTEMPTS = 10;
const OBS_LAUNCH_RETRY_DELAY_MS = 2000;

/**
 * Launches OBS (via the configured "OBS Path") and retries the connection —
 * which also provisions dod-studio' own profile/scene, same as any other
 * check — while it starts up. Used by Start Capture Batch's pre-flight below
 * so OBS doesn't have to already be open (much like HLAE doesn't) — the
 * batch can bring it up itself. Returns whether OBS ended up reachable.
 *
 * No-ops (returns false immediately) if no OBS path is configured — nothing
 * to launch, and the caller's existing "not connected" error already covers
 * that case correctly.
 */
export async function launchObsAndRetry() {
  const obsExePath = document.querySelector('#config-obs-exe-path')?.value?.trim();
  if (!obsExePath) return false;

  const status = document.querySelector('#obs-status');
  if (status) {
    status.style.display = '';
    status.textContent = STRINGS.CAPTURE_CONFIG.OBS_LAUNCHING_AND_CONNECTING;
  }

  try {
    await launchObs();
  } catch (err) {
    // Already toasted by ipc_bridge.js (missing/invalid path, etc.) — nothing
    // was launched, so there is nothing to wait for.
    return false;
  }

  for (let attempt = 0; attempt < OBS_LAUNCH_RETRY_ATTEMPTS; attempt++) {
    await new Promise((resolve) => setTimeout(resolve, OBS_LAUNCH_RETRY_DELAY_MS));
    await runObsConnectionTest({ auto: true });
    if (isObsConnected()) return true;
  }
  return false;
}

/**
 * Recomputes required-vs-available disk space and hard-locks the Launch
 * button (rather than just toasting at click time) whenever the capture
 * pool can't cover it — including the zero-drive case, which previously
 * bypassed the check entirely because `availableBytes > 0` gated the old
 * warning. Safe to call with no args once `initCaptureUI` has run; other
 * panes (main.js, detail_pane.js) call it after anything that can move
 * required/available bytes: streak selection, target drives, timing/res
 * config fields.
 */
export async function refreshLaunchGuard(state) {
  const startBtn = document.querySelector('#start-capture-btn') || document.querySelector('#start-batch-btn');
  const warningEl = document.querySelector('#disk-space-warning-banner');
  if (!startBtn) return null;

  const resolvedState = state || (currentGetState ? currentGetState() : null) || { targetDrives: [], currentScannedDemos: [] };

  const preRollVal = numberField('#config-pre-roll', 5.0);
  const postRollVal = numberField('#config-post-roll', 1.0);
  const recordStartLeadVal = numberField('#config-record-start-lead', 0.0);
  const recordStopTrailVal = numberField('#config-record-stop-trail', 0.0);
  const captureFpsVal = numberField('#config-capture-fps', 300, { integer: true, positive: true });
  const resWidthVal = numberField('#config-res-width', 1280, { integer: true, positive: true });
  const resHeightVal = numberField('#config-res-height', 720, { integer: true, positive: true });
  // AGR mode writes a few KB a frame, at its own rate, and a video take is
  // compressed (#1096) — sizing either like a frame sequence would block
  // batches that fit with room to spare.
  const captureMode = document.querySelector('#config-capture-mode')?.value;
  const agrMode = captureMode === 'agr';
  const agrFpsVal = numberField('#config-agr-fps', captureFpsVal, { integer: true, positive: true });
  const requiredBytes = computeRequiredCaptureBytes(resolvedState.currentScannedDemos, {
    preRollSeconds: preRollVal,
    postRollSeconds: postRollVal,
    recordStartLead: recordStartLeadVal,
    recordStopTrail: recordStopTrailVal,
    captureFps: agrMode ? agrFpsVal : captureFpsVal,
    resWidth: resWidthVal,
    resHeight: resHeightVal,
    bytesPerFrame: bytesPerFrameFor(
      captureMode,
      document.querySelector('#config-capture-codec')?.value,
      resWidthVal,
      resHeightVal,
    ),
    // An OBS take is billed by its length at its quality's rate (#1096).
    bytesPerSecond: captureMode === 'obs'
      ? obsBytesPerSecond(
        document.querySelector('#config-obs-recording-quality')?.value,
        resWidthVal,
        resHeightVal,
        numberField('#config-obs-capture-fps', 120, { integer: true, positive: true }),
      )
      : undefined,
    // Long gaps between kills recorded sped up write fewer frames (#831).
    killGaps: estimateKillGaps(readKillGaps(), document.querySelector('#config-hook-runs-batch')?.checked ?? true),
  });

  // Mirrors buildCapturePayload's outputDrivePool — Capture Output is the
  // sole (required) source of output directories now that Primary Media Dir
  // is gone.
  const effectiveDrivePool = (resolvedState.targetDrives || []).filter(Boolean);

  let availableBytes = 0;
  let problemPaths = [];
  let willBeCreatedPaths = [];
  if (effectiveDrivePool.length > 0) {
    try {
      availableBytes = await calculateExportPoolSpace(effectiveDrivePool);
    } catch (err) {
      console.error("Error calculating export pool space for launch guard:", err);
    }
    // Fetched unconditionally (not just when the pool is fully unusable) so a
    // *partially* bad pool — some valid drives, some typo'd/missing ones —
    // can still be called out below instead of going silently ignored just
    // because the aggregate sum happens to clear.
    try {
      const diagnostics = await diagnoseCaptureOutputPaths(effectiveDrivePool);
      problemPaths = diagnostics.filter((d) => !d.usable);
      // Usable but not found yet: real drive, folder just hasn't been
      // created — worth a quiet FYI (mainly to catch a typo of an intended
      // *existing* folder) but not a "problem," since it's the same case
      // get_available_bytes already counts as fine.
      willBeCreatedPaths = diagnostics.filter((d) => d.usable && d.status === 'not_found');
    } catch (err) {
      console.error("Error diagnosing Capture Output paths:", err);
    }
  }

  // Zero configured (or zero-space) drives must lock the button on its own —
  // `requiredBytes > availableBytes` alone would pass with requiredBytes 0.
  // Kept as two distinct booleans (not one merged "noDrivesConfigured") so
  // the warning message can tell "you haven't added anything" apart from
  // "you added something, but it doesn't resolve to a usable directory" —
  // those used to share one message, which told the user to do something
  // they'd already done.
  // Nothing selected is the most basic reason a batch cannot run, and it was
  // the one condition nothing checked: `buildCapturePayload` happily produced
  // a payload with zero streaks, so the batch started, patched nothing and
  // captured nothing. It belongs here rather than in the click handler so the
  // button is simply not live until there is something to capture.
  const selectedHighlights = (resolvedState.currentScannedDemos || []).reduce(
    (total, demo) => total + (demo.streaks || []).filter((s) => s.selected === true).length,
    0
  );
  const noHighlightsSelected = selectedHighlights === 0;
  // Every capture launches hl.exe through HLAE, so with either path blank
  // Start could only fail at click time (BOTH_PATHS_REQUIRED). A first-time
  // user hit that before anything else; now the button says so up front.
  const pathsMissing = !document.querySelector('#hl-path-input')?.value?.trim()
    || !document.querySelector('#hlae-path-input')?.value?.trim();

  const noDrivesConfigured = effectiveDrivePool.length === 0;
  const noUsableSpace = !noDrivesConfigured && availableBytes === 0;
  const insufficientSpace = !noDrivesConfigured && !noUsableSpace && requiredBytes > availableBytes;
  // "Only when space is needed" (#644): finished takes on the capture drives
  // may cover the shortfall, and only the backend can tell (make_room runs
  // before the capture and stops the batch when it can't), so short of room
  // is a heads-up here, not a block.
  // Render While Capturing (#861) holds between clips for renders to free
  // room, and the backend refuses only a clip no drive could ever hold.
  const rendersRoom = insufficientSpace && rendersMakeRoom();
  const makesRoom = insufficientSpace && (rendersRoom || takeCleanupMakesRoom());
  // Pool is usable overall (at least one real drive with room) but not every
  // configured entry is — worth a heads-up, not worth blocking the batch.
  const hasPartialProblems = !noDrivesConfigured && !noUsableSpace && !insufficientSpace && problemPaths.length > 0;
  // Issue #147: surface OBS-not-reachable up front rather than failing deep
  // inside the capture engine. Reflects obs_status.js's own most recent check
  // (main.js runs it on switching into OBS mode; this module's own
  // runObsConnectionTest also runs fresh as Start Capture Batch's own
  // pre-flight, below) rather than re-querying OBS here. Warning-only, not
  // blocking: the click handler launches OBS and retries the connection
  // itself when it finds this state, so disabling the button here would
  // deny that recovery path a first click ever reaching it — OBS is not
  // expected to already be open just because dod-studio switched into OBS
  // mode or even opened at all.
  const obsMode = document.querySelector('#config-capture-mode')?.value === 'obs';
  const obsNotReady = obsMode && obsConnectionChecked() && !isObsConnected();
  // Reflects cfg_warnings.js's own most recent scan rather than fetching its
  // own — a banned command (cfg_scan::BANNED_COMMANDS) is a correctness bug
  // waiting to happen, not a resource problem, so it takes priority over
  // every check below it, OBS included.
  const bannedCount = bannedCommandCount();
  const bannedCommandsPresent = bannedCount > 0;
  const blocked = bannedCommandsPresent || pathsMissing || noHighlightsSelected || noDrivesConfigured || noUsableSpace || (insufficientSpace && !makesRoom);

  if (!capturingInFlight && !precheck?.isChecking()) {
    startBtn.disabled = blocked;
  }
  // Every change that can block Start comes through here, so the strip
  // (#443) follows it: banned commands, destinations, commands edits.
  renderCaptureSummary();

  if (warningEl) {
    // First of all, ahead of even the calm cases below: a banned command is
    // an active correctness bug, not an incomplete-but-normal state.
    if (bannedCommandsPresent) {
      warningEl.style.color = '#f44336';
      warningEl.textContent = STRINGS.CAPTURE.bannedCommandsWarning(bannedCount);
      warningEl.style.display = 'block';
    } else if (pathsMissing) {
      warningEl.style.color = '#f44336';
      warningEl.textContent = STRINGS.CAPTURE.PATHS_MISSING_WARNING;
      warningEl.style.display = 'block';
    } else if (obsNotReady) {
      warningEl.style.color = '#f44336';
      warningEl.textContent = STRINGS.CAPTURE.OBS_NOT_CONNECTED_WARNING;
      warningEl.style.display = 'block';
    } else if (noHighlightsSelected) {
      warningEl.style.color = '#64b5f6';
      warningEl.textContent = STRINGS.CAPTURE.NO_HIGHLIGHTS_SELECTED_WARNING;
      warningEl.style.display = 'block';
    } else if (noDrivesConfigured) {
      warningEl.style.color = '#f44336';
      warningEl.textContent = STRINGS.CAPTURE.NO_DRIVES_CONFIGURED_WARNING;
      warningEl.style.display = 'block';
    } else if (noUsableSpace) {
      warningEl.style.color = '#f44336';
      warningEl.textContent = problemPaths.length > 0
        ? STRINGS.CAPTURE.noUsableSpaceProblem(describeProblemPaths(problemPaths))
        : STRINGS.CAPTURE.NO_USABLE_SPACE_WARNING;
      warningEl.style.display = 'block';
    } else if (makesRoom) {
      warningEl.style.color = '#ffa726';
      warningEl.textContent = (rendersRoom ? STRINGS.CAPTURE.insufficientSpaceRendersMakeRoom : STRINGS.CAPTURE.insufficientSpaceMakesRoom)((requiredBytes / 1e9).toFixed(2), (availableBytes / 1e9).toFixed(2));
      warningEl.style.display = 'block';
    } else if (insufficientSpace) {
      warningEl.style.color = '#f44336';
      warningEl.textContent = STRINGS.CAPTURE.insufficientSpaceWarning((requiredBytes / 1e9).toFixed(2), (availableBytes / 1e9).toFixed(2));
      warningEl.style.display = 'block';
    } else if (hasPartialProblems) {
      warningEl.style.color = '#ffa726';
      const wontBeUsed = problemPaths.length === 1 ? STRINGS.CAPTURE.ENTRY_WONT_BE_USED_SINGULAR : STRINGS.CAPTURE.ENTRY_WONT_BE_USED_PLURAL;
      warningEl.textContent = STRINGS.CAPTURE.partialProblemsWarning(wontBeUsed, describeProblemPaths(problemPaths));
      warningEl.style.display = 'block';
    } else if (willBeCreatedPaths.length > 0) {
      warningEl.style.color = '#64b5f6';
      const shown = willBeCreatedPaths.slice(0, MAX_PROBLEM_PATHS_SHOWN).map((d) => `"${d.path}"`);
      const remaining = willBeCreatedPaths.length - MAX_PROBLEM_PATHS_SHOWN;
      if (remaining > 0) shown.push(STRINGS.CAPTURE.andNMore(remaining));
      const doesnt = willBeCreatedPaths.length === 1 ? STRINGS.CAPTURE.DOESNT_SINGULAR : STRINGS.CAPTURE.DOESNT_PLURAL;
      warningEl.textContent = shown.length === 1
        ? STRINGS.CAPTURE.willBeCreatedSingle(shown[0], doesnt)
        : STRINGS.CAPTURE.willBeCreatedMultiple(doesnt, shown.map((s) => `• ${s}`).join('\n'));
      warningEl.style.display = 'block';
    } else {
      warningEl.style.display = 'none';
      warningEl.textContent = '';
    }
  }

  const footerRequiredEl = document.querySelector('#footer-required-space');
  if (footerRequiredEl) {
    const requiredGb = (requiredBytes / (1024 * 1024 * 1024)).toFixed(2);
    footerRequiredEl.textContent = STRINGS.CAPTURE_CONFIG.footerRequiredSpace(requiredGb);
    footerRequiredEl.style.color = insufficientSpace ? '#f44336' : '#4caf50';
  }

  return { requiredBytes, availableBytes, blocked, noHighlightsSelected };
}

// ── Custom Engine Commands (Init / Before-After) ──────────────────────────────
// Local to the Batch Capture Config panel — nothing else in the app reads
// these, unlike the pinned folders and capture folders (demo_folders.js,
// folder_pools.js), which other panes read too.

let initCommands = [];
let customCommands = [];
let initCommandsEditor = null;
let customCommandsEditor = null;

/** Scrapes the current Init/Custom Commands state for settings persistence
 *  (raw, untrimmed — mirrors in-progress edits rather than the filtered
 *  shape `buildCapturePayload` sends to `start_capture_batch`). */
/** The live capture setup, for the summary strip (#443). */
function currentCaptureSetup() {
  const state = (currentGetState ? currentGetState() : null) || {};
  const codecEl = document.querySelector('#config-capture-codec');
  const mode = document.querySelector('#config-capture-mode')?.value || 'frame_sequence';
  const lists = getCommandsState();
  return {
    mode,
    codecLabel: codecEl?.selectedOptions?.[0]?.textContent?.trim() || '',
    obsFps: numberField('#config-obs-capture-fps', 120, { integer: true, positive: true }),
    // An empty AGR FPS records at Capture FPS, as the batch does.
    agrFps: numberField('#config-agr-fps', numberField('#config-capture-fps', 300, { integer: true, positive: true }), { integer: true, positive: true }),
    width: numberField('#config-res-width', 1280, { integer: true, positive: true }),
    height: numberField('#config-res-height', 720, { integer: true, positive: true }),
    fps: numberField('#config-capture-fps', 300, { integer: true, positive: true }),
    // The same reading the Commands tab's profile row shows.
    commandProfile: profileState(getAllCommandProfiles(), getActiveCommandProfile(), lists),
    scheduledCount: lists.custom_commands.length,
    bannedCount: bannedCommandCount(),
    decalFlush: document.querySelector('#config-decal-flush')?.checked ?? true,
    finish: finishSummary(mode),
    destinations: (state.targetDrives || []).filter(Boolean).length > 0,
  };
}

export function getCommandsState() {
  return {
    init_commands: [...initCommands],
    custom_commands: customCommands.map(c => ({
      command: c.command,
      relation: c.relation === 'After' ? 'After' : 'Before',
      offset_seconds: c.offsetSeconds,
    })),
  };
}

/** Hydrates Init/Custom Commands state from persisted settings and
 *  re-renders both lists so they appear in the UI on boot. */
export function hydrateCommandsState(persistedInitCommands, persistedCustomCommands) {
  initCommands = Array.isArray(persistedInitCommands) ? [...persistedInitCommands] : [];
  customCommands = Array.isArray(persistedCustomCommands)
    ? persistedCustomCommands.map(c => ({
        command: c.command || '',
        relation: c.relation === 'After' ? 'After' : 'Before',
        offsetSeconds: typeof c.offset_seconds === 'number' ? c.offset_seconds : 2.0,
      }))
    : [];
  initCommandsEditor?.render();
  customCommandsEditor?.render();
}

/** Replaces both lists with a command profile's (#442). Unlike boot-time
 *  hydration, this is a change the user made, so everything that reads the
 *  lists hears about it; the caller saves settings. */
export function applyCommandsState(lists) {
  hydrateCommandsState(lists?.init_commands, lists?.custom_commands);
  refreshInitCommandWarnings();
  refreshRollFloors();
}

// ── Clear Previews audit modal ─────────────────────────────────────────────
//
// Audits `<hl>/dod` for orphaned `*_preview.dem` bookmark previews (see the
// block comment above `patch_bookmark_previews` in capture_manager.rs) left
// behind across capture sessions, and lets the user purge them.

let currentPreviewScanResults = [];

function formatPreviewSize(bytes) {
  return STRINGS.CAPTURE.megabytesLabel((bytes / (1024 * 1024)).toFixed(2));
}

function updateClearPreviewsDeleteButtonState() {
  const deleteBtn = document.querySelector('#clear-previews-delete-btn');
  if (!deleteBtn) return;
  const checked = document.querySelectorAll('.clear-previews-row-cb:checked');
  deleteBtn.disabled = checked.length === 0;
  deleteBtn.textContent = checked.length > 0 ? STRINGS.CAPTURE.deleteNSelected(checked.length) : STRINGS.CAPTURE.DELETE_SELECTED_DEFAULT;
}

function renderClearPreviewsResults() {
  const tbody = document.querySelector('#clear-previews-body');
  const footerEl = document.querySelector('#clear-previews-footer');
  if (!tbody) return;

  if (currentPreviewScanResults.length === 0) {
    tbody.innerHTML = `<tr><td colspan="4" class="table-empty">${STRINGS.CAPTURE.NO_ORPHANED_PREVIEWS}</td></tr>`;
    if (footerEl) footerEl.textContent = STRINGS.CLEAR_PREVIEWS_MODAL.FOOTER_DEFAULT;
    updateClearPreviewsDeleteButtonState();
    return;
  }

  tbody.innerHTML = '';
  let totalBytes = 0;

  currentPreviewScanResults.forEach((entry) => {
    totalBytes += entry.size_bytes;

    const tr = document.createElement('tr');

    const tdCb = document.createElement('td');
    const cb = document.createElement('input');
    cb.type = 'checkbox';
    cb.className = 'clear-previews-row-cb';
    cb.dataset.path = entry.demo_path;
    cb.checked = true;
    cb.addEventListener('change', updateClearPreviewsDeleteButtonState);
    tdCb.appendChild(cb);

    const tdFile = document.createElement('td');
    tdFile.textContent = entry.file_name;

    const tdSize = document.createElement('td');
    tdSize.textContent = formatPreviewSize(entry.size_bytes);

    const tdModified = document.createElement('td');
    tdModified.textContent = entry.modified_unix_secs
      ? new Date(entry.modified_unix_secs * 1000).toLocaleString()
      : STRINGS.CAPTURE.EMPTY_DASH;

    tr.appendChild(tdCb);
    tr.appendChild(tdFile);
    tr.appendChild(tdSize);
    tr.appendChild(tdModified);
    tbody.appendChild(tr);
  });

  const totalGb = (totalBytes / (1024 * 1024 * 1024)).toFixed(2);
  if (footerEl) footerEl.textContent = STRINGS.CLEAR_PREVIEWS_MODAL.foundReclaimable(currentPreviewScanResults.length, totalGb);
  updateClearPreviewsDeleteButtonState();
}

// ── Standalone Game Launch ───────────────────────────────────────────────────
//
// Boots HLAE against hl.exe with no demo loaded. Routes through the same
// running-process guard as the per-demo preview launchers (detail_pane.js) —
// on conflict, the intent is parked behind the shared Preview Detector modal
// via `requestProcessGuardedLaunch` instead of duplicating that modal's
// click listeners here.

async function initStandaloneLaunchButton() {
  const btn = document.querySelector('#btn-launch-standalone-game');
  if (!btn) return;

  // Shown in every build: previews and reviews reuse a game Studio started,
  // and one started with other settings is offered a restart (#666), so
  // starting the game ahead of them is safe.

  async function performLaunch() {
    btn.disabled = true;
    const originalLabel = btn.textContent;
    btn.textContent = STRINGS.HIGHLIGHTS.LAUNCHING;
    try {
      await launchStandaloneGame();
      showToast(STRINGS.HIGHLIGHTS.LAUNCHING_HLAE_TOAST, 'info');
    } catch (err) {
      // Already toasted by ipc_bridge.js.
    } finally {
      btn.textContent = originalLabel;
      btn.disabled = false;
    }
  }

  btn.addEventListener('click', async () => {
    // A game started with other launch settings is closed first, if the user
    // says so (#666).
    if (!(await closeGameWithOtherSettings(null, { button: btn }))) return;
    let engineAlreadyRunning = false;
    try {
      engineAlreadyRunning = await checkEngineProcesses();
    } catch (err) {
      // Already toasted by ipc_bridge.js — fail open rather than blocking
      // a legitimate launch just because the detector itself errored.
    }

    if (engineAlreadyRunning) {
      requestProcessGuardedLaunch(performLaunch);
      return;
    }

    if (!(await ensureSteamReady())) return;
    await performLaunch();
  });
}

// ── Import Config ─────────────────────────────────────────────────────────
//
// Lets a manually exec'd movie config's lines land in Initial Commands
// directly — STUFFTEXT there reliably wins over anything a config.cfg or
// movie.cfg sets, which is what makes this the alternative to relying on an
// autoexec'd movie config the pipeline never sees (see the info tooltip on
// the Initial Commands label).

function initImportCfgButton() {
  const btn = document.querySelector('#import-cfg-btn');
  if (!btn) return;

  btn.addEventListener('click', async () => {
    const picked = await pickPath({
      field: 'capture.import_cfg',
      title: STRINGS.MAIN.SELECT_MOVIE_CFG_TITLE,
      multiple: false,
      filters: [{ name: STRINGS.MAIN.CONFIG_FILTER_NAME, extensions: ['cfg'] }],
    });
    if (!picked || Array.isArray(picked)) return;

    try {
      const lines = await readCfgCommands(picked);
      if (lines.length === 0) {
        showToast(STRINGS.CAPTURE_CONFIG.IMPORTED_CFG_EMPTY_TOAST, 'info');
        return;
      }
      lines.forEach((line) => initCommandsEditor.addItem(line));
      notifySettingsChange();
      refreshInitCommandWarnings();
      showToast(STRINGS.CAPTURE_CONFIG.importedCfgToast(lines.length), 'success');
    } catch (err) {
      // Already toasted by ipc_bridge.js.
    }
  });
}

function initClearPreviewsModal() {
  const openBtn = document.querySelector('#open-clear-previews-btn');
  const modal = document.querySelector('#clear-previews-modal');
  if (!openBtn || !modal) return;

  const statusEl = document.querySelector('#clear-previews-status');
  const closeBtn = document.querySelector('#clear-previews-close-btn');
  const deleteBtn = document.querySelector('#clear-previews-delete-btn');
  const selectAllBtn = document.querySelector('#clear-previews-select-all-btn');
  const tbody = document.querySelector('#clear-previews-body');

  openBtn.addEventListener('click', async () => {
    const gameDir = document.querySelector('#hl-path-input')?.value?.trim() || '';
    if (!gameDir) {
      showToast(STRINGS.CAPTURE.CONFIGURE_HL_PATH_FIRST, 'error');
      return;
    }

    modal.style.display = 'flex';
    currentPreviewScanResults = [];
    if (statusEl) statusEl.textContent = STRINGS.CAPTURE.SCANNING_ORPHANED_PREVIEWS;
    if (tbody) tbody.innerHTML = `<tr><td colspan="4" class="table-empty">${STRINGS.CLEAR_PREVIEWS_MODAL.SCANNING_ROW}</td></tr>`;
    updateClearPreviewsDeleteButtonState();

    try {
      currentPreviewScanResults = await scanOrphanedPreviews(gameDir);
      if (statusEl) statusEl.textContent = STRINGS.CAPTURE.SCAN_COMPLETE;
      renderClearPreviewsResults();
    } catch (e) {
      if (statusEl) statusEl.textContent = STRINGS.CAPTURE.SCAN_FAILED;
      if (tbody) tbody.innerHTML = `<tr><td colspan="4" class="table-empty">${STRINGS.CAPTURE.scanFailedRow(e)}</td></tr>`;
    }
  });

  if (closeBtn) {
    closeBtn.addEventListener('click', () => {
      modal.style.display = 'none';
    });
  }

  if (selectAllBtn) {
    selectAllBtn.addEventListener('click', () => {
      const boxes = document.querySelectorAll('.clear-previews-row-cb');
      const allChecked = boxes.length > 0 && Array.from(boxes).every(cb => cb.checked);
      boxes.forEach(cb => { cb.checked = !allChecked; });
      updateClearPreviewsDeleteButtonState();
    });
  }

  if (deleteBtn) {
    deleteBtn.addEventListener('click', async () => {
      const checked = document.querySelectorAll('.clear-previews-row-cb:checked');
      const pathsToDelete = Array.from(checked).map(cb => cb.dataset.path);
      if (pathsToDelete.length === 0) return;
      if (!(await themedConfirm(STRINGS.CAPTURE.deletePreviewsConfirm(pathsToDelete.length), { destructive: true }))) return;

      deleteBtn.disabled = true;
      try {
        const deletedCount = await deleteOrphanedPreviews(pathsToDelete);
        showToast(STRINGS.CAPTURE.deletedPreviews(deletedCount), 'success');
        currentPreviewScanResults = currentPreviewScanResults.filter(entry => !pathsToDelete.includes(entry.demo_path));
        renderClearPreviewsResults();
      } catch (e) {
        showToast(STRINGS.CAPTURE.deletionFailed(e), 'error');
        updateClearPreviewsDeleteButtonState();
      }
    });
  }
}

/**
 * Every batch setting of the `start_capture_batch` payload, from the DOM and
 * `state.targetDrives`: all but `streaks` and `session_id`. Returns `null`
 * (after toasting the reason) when a required field is missing. Shared by
 * Start Capture Batch and the OBS capture test (#646), so a test clip runs
 * with exactly the settings a real batch would.
 */
export function batchSettingsPayload(state) {
  const captureFpsVal = numberField('#config-capture-fps', 120, { integer: true, positive: true });
  const obsCaptureFpsVal = numberField('#config-obs-capture-fps', 120, { integer: true, positive: true });
  // Empty is 0, "the same as Capture FPS" — resolved by the backend.
  const agrFpsVal = numberField('#config-agr-fps', 0, { integer: true, positive: true });
  const preRollVal = numberField('#config-pre-roll', 5.0);
  const postRollVal = numberField('#config-post-roll', 1.0);
  const recordStartLeadVal = numberField('#config-record-start-lead', 5.0);
  const recordStopTrailVal = numberField('#config-record-stop-trail', 5.0);
  const initialDelayVal = numberField('#config-initial-delay', 3.0);
  // The fast-forward step (`host_framerate`), not a setting: batches skip to
  // each clip, and fast-forward is only the fallback.
  const fastForwardSpeedVal = getFastForwardStep();

  const hlaePathVal = document.querySelector("#hlae-path-input")?.value?.trim() || "";
  const hlPathVal = document.querySelector("#hl-path-input")?.value?.trim() || "";
  const ffmpegOverridePathVal = document.querySelector("#ffmpeg-override-path-input")?.value?.trim() || null;
  const goldsrcHooksDllPathVal = document.querySelector("#goldsrc-hooks-dll-path-input")?.value?.trim() || null;

  const resWidthVal = numberField('#config-res-width', 1280, { integer: true, positive: true });
  const resHeightVal = numberField('#config-res-height', 720, { integer: true, positive: true });
  // `?? true` not `|| false`: a missing element must not silently disable
  // the flush, since nothing in the captured video would show that it had.
  const decalFlushVal = document.querySelector("#config-decal-flush")?.checked ?? true;
  // The mode select is the authority; `ffmpeg_capture` is derived from it so
  // the payload cannot describe two modes at once. The backend reconciles
  // both fields again in `normalise_capture_mode`, which is what keeps an
  // older frontend working against this build.
  const captureModeVal = document.querySelector("#config-capture-mode")?.value || "frame_sequence";
  const ffmpegCaptureVal = captureModeVal === "direct_to_video";
  const ffmpegCaptureCodecVal = document.querySelector("#config-capture-codec")?.value || "utvideo";
  const obsHostVal = document.querySelector("#config-obs-host")?.value?.trim() || "127.0.0.1";
  const obsPortVal = parseInt(document.querySelector("#config-obs-port")?.value, 10) || 4455;
  const obsPasswordVal = document.querySelector("#config-obs-password")?.value || "";
  const saveLocalPatchedCopyVal = document.querySelector("#config-save-local-patched")?.checked || false;
  const hookRunsBatchVal = document.querySelector("#config-hook-runs-batch")?.checked ?? true;
  const keepGameBehindVal = document.querySelector("#config-keep-game-behind")?.checked ?? true;

  const autoClearLogsVal = document.querySelector("#config-auto-clear-logs")?.checked || false;
  const autoClearPreviewsVal = document.querySelector("#config-auto-clear-previews")?.checked || false;
  const autoClearTempDemosVal = document.querySelector("#config-auto-clear-temp-demos")?.checked || false;

  if (!hlaePathVal || !hlPathVal) {
    showToast(STRINGS.CAPTURE.BOTH_PATHS_REQUIRED, 'error');
    return null;
  }

  // capture_directories is the physical BMP/patched-demo output pool —
  // native/src/patch/builder.rs routes capture output there and
  // capture_engine.rs mklinks a junction per entry. It must be actual
  // output directories, NEVER state.scanPaths (the demo *source* files
  // the user added for scanning); mklinking a junction against a .dem
  // file aborts the batch. Sourced from Capture Output — required, no
  // fallback.
  const outputDrivePool = (state.targetDrives || []).filter(Boolean);

  if (outputDrivePool.length === 0) {
    showToast(STRINGS.CAPTURE.NO_CAPTURE_OUTPUT_DIR, 'error');
    return null;
  }

  // Blank rows (added via "+ Add" but never filled in) are dropped rather
  // than sent as no-op console commands.
  const initCommandsPayload = initCommands.map(c => c.trim()).filter(c => c.length > 0);
  const customCommandsPayload = customCommands
    .filter(c => c.command && c.command.trim().length > 0)
    .map(c => ({
      command: c.command.trim(),
      relation: c.relation === 'After' ? 'After' : 'Before',
      offset_seconds: c.offsetSeconds,
    }));

  return {
    hlae_path: hlaePathVal,
    game_path: hlPathVal,
    ffmpeg_override_path: ffmpegOverridePathVal,
    goldsrc_hooks_dll_path: goldsrcHooksDllPathVal,
    resolution_width: resWidthVal,
    resolution_height: resHeightVal,
    decal_flush: decalFlushVal,
    ffmpeg_capture: ffmpegCaptureVal,
    ffmpeg_capture_codec: ffmpegCaptureCodecVal,
    capture_mode: captureModeVal,
    obs_host: obsHostVal,
    obs_port: obsPortVal,
    obs_password: obsPasswordVal,
    obs_recording_quality: document.querySelector('#config-obs-recording-quality')?.value || '',
    save_local_patched_copy: saveLocalPatchedCopyVal,
    hook_runs_batch: hookRunsBatchVal,
    keep_game_behind: keepGameBehindVal,
    pre_roll_seconds: preRollVal,
    post_roll_seconds: postRollVal,
    capture_directories: outputDrivePool,
    capture_fps: captureFpsVal,
    obs_capture_fps: obsCaptureFpsVal,
    agr_fps: agrFpsVal,
    drives: state.targetDrives || [],
    record_start_lead: recordStartLeadVal,
    record_stop_trail: recordStopTrailVal,
    initial_delay: initialDelayVal,
    fast_forward_speed: fastForwardSpeedVal,
    kill_gaps: readKillGaps(),
    auto_clear_logs: autoClearLogsVal,
    auto_clear_previews: autoClearPreviewsVal,
    auto_clear_temp_demos: autoClearTempDemosVal,
    init_commands: initCommandsPayload,
    custom_commands: customCommandsPayload,
  };
}

/** Render While Capturing's hold line on the footer (`captureProgress.setRoom`). */
export function showCaptureRoomLine(room) {
  captureProgress?.setRoom(room);
}

/**
 * Render While Capturing (#861): a take of the running batch has settled and
 * is about to be queued for rendering. Its highlights go Captured and the
 * take index learns it now, before the render: the render's clip name and
 * its Rendered mark both resolve through the index, and the take may be
 * deleted before the batch's own verification looks. `clip` is a
 * `capture_clip_ready` payload; one from another batch is ignored.
 */
export function markClipCaptured(clip) {
  if (!clip || !lastDispatch || clip.session_id !== lastDispatch.sessionId) return;
  const takeIndex = currentGetTakeIndex ? currentGetTakeIndex() : null;
  const { advanced, markCleared } = markBlockCaptured(clip, lastDispatch, takeIndex);
  if ((advanced > 0 || markCleared) && currentOnStatusChange) currentOnStatusChange();
}

export function initCaptureUI(getState, onSettingsChange, onStatusChange, getTakeIndex, onBatchFinished, pickedDemosPresent) {
  const startBtn = document.querySelector('#start-capture-btn') || document.querySelector('#start-batch-btn');
  const cancelBtn = document.querySelector('#cancel-batch-btn');
  const statusEl = document.querySelector('#batch-status');
  const progressContainer = document.querySelector('#capture-progress-container');

  // No "keep the game focused" reminder: every batch turns on
  // ds_capture_run_in_background, so a game behind other windows or minimised
  // fast-forwards at full speed (#701, #722).
  const setBatchRunning = (running) => {
    if (progressContainer && running) progressContainer.style.display = 'block';
  };

  currentGetState = getState;
  precheck = createCapturePrecheck({ startBtn, statusEl, progressContainer, refreshStart: () => refreshLaunchGuard() });
  captureProgress = createCaptureProgress({ statusEl, container: progressContainer, log: logFrontendEvent });
  startObsDropRate({ el: document.querySelector('#obs-drop-rate'), listen, toast: (text) => showToast(text, 'warning', 12000) });
  initCaptureSummary(currentCaptureSetup);
  currentOnSettingsChange = onSettingsChange || null;
  currentOnStatusChange = onStatusChange || null;
  currentGetTakeIndex = getTakeIndex || null;
  currentOnBatchFinished = onBatchFinished || null;

  initCommandsEditor = createListEditor({
    container: document.querySelector('#init-commands-list'),
    getItems: () => initCommands,
    fields: [{
      key: 'value', type: 'text', primitive: true, placeholder: STRINGS.CAPTURE_CONFIG.INIT_COMMAND_PLACEHOLDER,
      enhance: (input) => attachCommandSuggest(input, { scheduled: false, seeks: seeksBetweenClips }),
    }],
    onChange: () => {
      // Before the save, so a profile this list now matches is saved as the
      // active one.
      syncCommandProfileRow();
      notifySettingsChange();
      // Typing a command here can silence a line in the user's own config, and
      // this is the moment they can still see both.
      refreshInitCommandWarnings();
    },
  });

  customCommandsEditor = createListEditor({
    container: document.querySelector('#custom-commands-list'),
    getItems: () => customCommands,
    fields: [
      {
        key: 'command', type: 'text', placeholder: STRINGS.CAPTURE_CONFIG.CUSTOM_COMMAND_PLACEHOLDER,
        enhance: (input) => attachCommandSuggest(input, { scheduled: true, seeks: seeksBetweenClips }),
      },
      { key: 'relation', type: 'select', options: STRINGS.CAPTURE_CONFIG.CUSTOM_COMMAND_RELATION_OPTIONS },
      { key: 'offsetSeconds', type: 'number', step: 0.1, min: 0, width: '70px' },
    ],
    onChange: () => {
      syncCommandProfileRow();
      notifySettingsChange();
      // These run last of all — after the configs and after the init commands —
      // and are the only place a cvar changes partway through a capture.
      refreshInitCommandWarnings();
      // A Scheduled Command offset is one of the terms the roll floors are
      // built from, so moving one can move the floor.
      refreshRollFloors();
    },
  });

  initClearPreviewsModal();
  initStandaloneLaunchButton();
  initImportCfgButton();

  // The pipeline turns some settings into init commands, so the warnings go
  // stale when one changes -- including by undo, which fires no 'change'
  // until blur (#535).
  FIELDS_THAT_BECOME_COMMANDS.forEach((sel) => {
    refreshAfterTyping(document.querySelector(sel), refreshInitCommandWarnings);
  });

  const addInitCommandBtn = document.querySelector('#add-init-command-btn');
  if (addInitCommandBtn) {
    addInitCommandBtn.addEventListener('click', () => {
      initCommandsEditor.addItem('');
    });
  }

  const addCustomCommandBtn = document.querySelector('#add-custom-command-btn');
  if (addCustomCommandBtn) {
    addCustomCommandBtn.addEventListener('click', () => {
      customCommandsEditor.addItem({ command: '', relation: 'Before', offsetSeconds: 2.0 });
    });
  }

  // Any config field that feeds computeRequiredCaptureBytes recomputes the
  // hard launch guard on change, so the Start button's disabled state stays
  // live instead of only being checked at click time. These (plus the rest
  // of the Timing Options tab, which doesn't feed the guard) also persist to
  // settings.json on edit — previously nothing wired them to a save at all,
  // so values only survived a restart by coincidence, if some unrelated
  // action (e.g. browsing for hlae.exe) happened to save afterward.
  // Start-lead/stop-trail belong in this list too: they define the recorded
  // window, so they change the disk estimate the guard is built on.
  ['#config-res-width', '#config-res-height', '#config-ffmpeg-capture',
   '#config-pre-roll', '#config-post-roll', '#config-capture-fps',
   '#config-record-start-lead', '#config-record-stop-trail', '#config-agr-fps'].forEach(selector => {
    const el = document.querySelector(selector);
    if (el) el.addEventListener('input', () => { refreshLaunchGuard(); notifySettingsChange(); });
  });
  // AGR mode sizes a batch in KB rather than GB, so switching mode moves the
  // disk estimate too.
  document.querySelector('#config-capture-mode')?.addEventListener('change', () => refreshLaunchGuard());
  // The codec changes what a video take is billed at (#1096).
  document.querySelector('#config-capture-codec')?.addEventListener('change', () => refreshLaunchGuard());
  // OBS takes are billed by quality and OBS fps (#1096).
  document.querySelector('#config-obs-recording-quality')?.addEventListener('change', () => refreshLaunchGuard());
  document.querySelector('#config-obs-capture-fps')?.addEventListener('change', () => refreshLaunchGuard());
  // Same missing-wiring bug as the rest of this function, just on the OBS
  // connection fields and the capture-mode selector — all three read at
  // capture/save time but never saved on their own change, so edits looked
  // like they took but silently reverted on the next launch/refresh.
  ['#config-initial-delay', '#config-obs-host', '#config-obs-port', '#config-obs-password', '#config-obs-exe-path', '#config-obs-capture-fps'].forEach(selector => {
    const el = document.querySelector(selector);
    if (el) el.addEventListener('input', () => notifySettingsChange());
  });
  document.querySelector('#config-obs-recording-quality')
    ?.addEventListener('change', () => notifySettingsChange());
  // Keeps the Timings tab's visual timeline (#150) in step with whichever
  // of these five fields was just edited.
  ['#config-pre-roll', '#config-post-roll', '#config-record-start-lead',
   '#config-record-stop-trail', '#config-initial-delay'].forEach(selector => {
    document.querySelector(selector)?.addEventListener('input', renderTimingDiagram);
  });
  // Seek Between Clips shows or hides Initial Delay, and decides whether the
  // game's demo filter drops any of the commands.
  document.querySelector('#config-hook-runs-batch')?.addEventListener('change', () => {
    renderTimingDiagram();
    refreshInitCommandWarnings();
    // A patched batch keeps its gaps (#831), which moves the disk estimate.
    refreshLaunchGuard();
  });
  wireKillGaps(() => { refreshLaunchGuard(); notifySettingsChange(); });
  renderTimingDiagram();
  // Checkboxes read by persistAppSettings/buildCapturePayload but with no
  // change listener of their own — same missing-wiring bug as the Timing
  // Options fields above, just on Path Routing / Capture Output checkboxes.
  ['#config-auto-clear-logs', '#config-auto-clear-previews',
   '#config-auto-clear-temp-demos', '#config-save-local-patched',
   '#config-notify-patching', '#config-notify-demo-loading', '#config-notify-between-clips',
   '#config-notify-captures-done', '#config-notify-renders-done', '#config-notify-error',
   // A <select> fires `change`, not `input` — it belongs here rather than in
   // the list above, which is wired for text/number/checkbox inputs.
   '#config-capture-codec', '#config-capture-mode'].forEach(selector => {
    const el = document.querySelector(selector);
    if (el) el.addEventListener('change', () => notifySettingsChange());
  });
  // Only read at capture time (buildCapturePayload), same as the checkboxes
  // above, but previously not part of AppSettings at all — reset to default
  // every restart.
  refreshLaunchGuard();

  // An error box shown by the game or HLAE after a preview or Launch Game,
  // read by the backend (native::sys::dialogs). A batch reports its own
  // through capture_status instead.
  if (!listeningForExternalErrors) {
    listeningForExternalErrors = true;
    listen('external_error', (event) => {
      showToast(String(event.payload || ''), 'error', 15000);
    });
    // Deleted takes free space on the Destinations drives: "Capture Output
    // Free" reads it again (the backend's free-space cache is already dropped).
    // currentOnBatchFinished is that figure's refresh (see its declaration).
    listen('take_cleanup_done', () => {
      refreshLaunchGuard();
      if (currentOnBatchFinished) currentOnBatchFinished();
    });
  }

  if (!unlistenCaptureStatus) {
    listen('capture_status', (event) => {
      const payload = event.payload || {};
      // An OBS capture test's clip (#646): progress only. The test's own
      // panel says how it ended, and Last Batch keeps the user's last batch.
      const testBatch = isObsTestBatch();
      if (payload.running) {
        // The first running report of a batch: the last one's results go.
        // A Cancel clears capturingInFlight before the takes are checked,
        // and that check still reports as running: it is the same batch.
        if (!testBatch && !capturingInFlight && !captureProgress.isRunning()) batchStarted();
        capturingInFlight = true;
        setBatchRunning(true);
        captureProgress.update(payload);
        if (startBtn) startBtn.disabled = true;
        // The game has closed once the takes are checked: nothing to cancel.
        if (cancelBtn) cancelBtn.disabled = payload.stage === 'checking';
      } else {
        capturingInFlight = false;
        // The line says how it ended; the toasts below say it too.
        captureProgress.end(payload);
        setBatchRunning(false);
        if (cancelBtn) cancelBtn.disabled = true;
        refreshLaunchGuard();
        if (testBatch) return;
        if (currentOnBatchFinished) currentOnBatchFinished();
        const kind = endKind(payload);
        batchEnded(kind, uiStatusText(payload.status));

        if (kind === 'error') {
          // Without the engine's pointers at the log (#534); the log has them.
          const errorBody = STRINGS.CAPTURE.captureErrorToast(uiStatusText(payload.status) || STRINGS.CAPTURE.CAPTURE_ERROR_STATUS_DEFAULT);
          showToast(errorBody, "error");
          notify('error', STRINGS.NOTIFICATIONS.CAPTURES_ERROR_TITLE, errorBody);
        } else if (kind === 'cancelled') {
          showToast(STRINGS.CAPTURE.BATCH_CANCELLED_TOAST, "info");
        } else {
          showToast(STRINGS.CAPTURE.BATCH_COMPLETED_TOAST, "success");
          notify('captures_done', STRINGS.NOTIFICATIONS.CAPTURES_DONE_TITLE, STRINGS.CAPTURE.BATCH_COMPLETED_TOAST);
        }
      }
    }).then(unlistenFn => {
      unlistenCaptureStatus = unlistenFn;
    }).catch(err => {
      console.error("Failed to register capture_status listener:", err);
    });
  }

  // Per-demo progress, from the engine's own DEMO_START console marker, which
  // -condebug produces on every launch. clips_so_far is computed server-side
  // (capture_manager.rs), so this listener does no running-total bookkeeping
  // of its own.
  if (!unlistenDemoLoading) {
    listen('capture_demo_loading', (event) => {
      const payload = event.payload || {};
      // The upcoming "fast-forwarding to clip 1" toast carries the same
      // title plus more detail — skip this one so they don't double up,
      // when that notification is actually on to cover it.
      if (isNotificationEnabled('between_clips') || isObsTestBatch()) return;
      notify(
        'demo_loading',
        STRINGS.NOTIFICATIONS.demoLoadingTitle(payload.index, payload.total),
        STRINGS.NOTIFICATIONS.demoLoadingBody(payload.clip_count, payload.clips_so_far, payload.total_batch_clips)
      );
    }).then(unlistenFn => {
      unlistenDemoLoading = unlistenFn;
    }).catch(err => {
      console.error("Failed to register capture_demo_loading listener:", err);
    });
  }

  // Fast-forward-to-clip progress, from the engine's own NEXT_CLIP console
  // marker — same console-log source as demo-loading above. Fires for
  // clip 1 too (see the demo-loading listener's own suppression above).
  if (!unlistenFastForwardToClip) {
    listen('capture_fast_forward_to_clip', (event) => {
      if (isObsTestBatch()) return;
      const payload = event.payload || {};
      notify(
        'between_clips',
        STRINGS.NOTIFICATIONS.demoLoadingTitle(payload.demo_index, payload.demo_total),
        STRINGS.NOTIFICATIONS.fastForwardToClipBody(payload.clip_index, payload.clip_count_this_demo, payload.clips_so_far, payload.total_batch_clips)
      );
    }).then(unlistenFn => {
      unlistenFastForwardToClip = unlistenFn;
    }).catch(err => {
      console.error("Failed to register capture_fast_forward_to_clip listener:", err);
    });
  }

  // Patching phase start/end -- one toast each, not per-demo (see
  // capture_demo_loading above for the per-demo one).
  if (!unlistenPatchingStarted) {
    listen('capture_patching_started', (event) => {
      const total = (event.payload || {}).total || 0;
      notify('patching', STRINGS.NOTIFICATIONS.patchingStartedTitle(total), STRINGS.NOTIFICATIONS.PATCHING_STARTED_BODY);
    }).then(unlistenFn => {
      unlistenPatchingStarted = unlistenFn;
    }).catch(err => {
      console.error("Failed to register capture_patching_started listener:", err);
    });
  }
  if (!unlistenPatchingFinished) {
    listen('capture_patching_finished', (event) => {
      const total = (event.payload || {}).total || 0;
      notify('patching', STRINGS.NOTIFICATIONS.PATCHING_FINISHED_TITLE, STRINGS.NOTIFICATIONS.patchingFinishedBody(total));
    }).then(unlistenFn => {
      unlistenPatchingFinished = unlistenFn;
    }).catch(err => {
      console.error("Failed to register capture_patching_finished listener:", err);
    });
  }

  // Post-batch take verification: advances each verified highlight to Captured.
  if (!unlistenTakesVerified) {
    listen('capture_takes_verified', (event) => {
      const payload = event.payload || {};
      batchVerified(payload, lastDispatch);
      const blocks = payload.blocks || [];
      const total = payload.total_count ?? blocks.length;
      const captured = payload.captured_count ?? 0;
      const renderable = payload.renderable_count ?? 0;

      const takeIndex = currentGetTakeIndex ? currentGetTakeIndex() : null;

      let advanced = 0;
      // A hand-set Captured confirmed on disk: no status change, but the
      // set-by-hand mark goes (#105).
      let markCleared = false;
      blocks.forEach(block => {
        if (!block.captured) return;
        const marked = markBlockCaptured(block, lastDispatch, takeIndex);
        advanced += marked.advanced;
        markCleared ||= marked.markCleared;
      });

      if (total === 0) return;

      if (advanced > 0 || markCleared) {
        // Both tables read status, and neither re-renders on its own.
        if (currentOnStatusChange) currentOnStatusChange();
      }

      const marked = advanced > 0 ? STRINGS.CAPTURE.highlightsMarkedCaptured(advanced) : '';
      if (captured < total) {
        // A take whose video stopped early (#1105) is cut_short too, but the
        // batch's end isn't why. The end cuts one clip at most: its `cut_by`.
        const stoppedEarly = blocks.filter((b) => b.short_take).length;
        const cut = blocks.filter((b) => b.cut_short && !b.short_take);
        showToast(`${STRINGS.CAPTURE.takesFoundMissing(captured, total, cut.length, stoppedEarly, cut[0]?.cut_by)}${marked}`, 'error');
      } else if (renderable < total) {
        showToast(`${STRINGS.CAPTURE.takesRenderStudioMiss(captured, total, total - renderable)}${marked}`, 'info');
      } else {
        showToast(`${STRINGS.CAPTURE.allTakesVerified(total)}${marked}`, 'success');
      }

      // "When a batch finishes" (#440) — a no-op unless that is set to finish
      // clips. After the status flip above, so the take index already maps
      // each take back to its highlights when its render lands.
      requestFinishClips(payload, lastDispatch?.captureMode);
    }).then(unlistenFn => {
      unlistenTakesVerified = unlistenFn;
    }).catch(err => {
      console.error("Failed to register capture_takes_verified listener:", err);
    });
  }

  /**
   * Reads every Batch Capture Config field from the DOM plus the given
   * cross-pane `state` (scanned demos, target drives) and assembles the
   * `start_capture_batch` IPC payload. Returns `null` (after toasting the
   * reason) if a required field is missing — callers must check for that
   * before proceeding.
   */
  function buildCapturePayload(state) {
    const selectedStreaks = [];
    // Index-aligned with selectedStreaks — the owning demo's path, needed to
    // derive each streak's durable uid (streakUid takes demoPath + streak)
    // for the take index below.
    const selectedDemoPaths = [];
    if (state.currentScannedDemos) {
      state.currentScannedDemos.forEach(demo => {
        if (demo.streaks) {
          demo.streaks.forEach(streak => {
            // Opt-in model (detail_pane.js) — see computeRequiredCaptureBytes above.
            // A Skip highlight is locked out even if something ticked it (#44).
            if (streak.selected === true && !isSkipped(streak)) {
              selectedStreaks.push(streak);
              selectedDemoPaths.push(demo.path);
            }
          });
        }
      });
    }

    // Remembered so the post-batch take verification can map each block's
    // source_streak_indices back to these exact live streak objects — the
    // backend preserves this array's order, so index N here is index N there.
    const sessionId = generateSessionId();
    const settings = batchSettingsPayload(state);
    if (!settings) return null;
    // kill_gaps: the take labels name the speed sped-up gaps played at (#831).
    lastDispatch = { sessionId, streaks: selectedStreaks, demoPaths: selectedDemoPaths, kill_gaps: settings.kill_gaps };
    // The finish step (#440) picks its codec by the mode this batch used.
    lastDispatch.captureMode = settings.capture_mode;
    return {
      ...settings,
      streaks: selectedStreaks,
      // Recorded in the batch's manifest, so a batch that outlives Studio can
      // still be mapped back to these highlights when it reopens (#545).
      streak_uids: selectedStreaks.map((streak, i) => streakUid(selectedDemoPaths[i], streak)),
      project_path: state.projectPath || null,
      session_id: sessionId,
    };
  }

  if (startBtn) {
    startCaptureClick = async () => {
      if (precheck.isChecking()) return;
      precheck.begin();
      try {
        await runChecksAndStart();
      } finally {
        // A check that threw, or returned without saying why.
        precheck.stop();
      }
    };
    startBtn.addEventListener('click', startCaptureClick);

    // Start Capture Batch's checks, in order, each on the footer line (#953).
    // A check that stops the batch calls precheck.stop() with the reason its
    // toast gives; one that passes everything hands over to the batch.
    const runChecksAndStart = async () => {
      const state = getState ? getState() : { scanPaths: [], targetDrives: [], currentScannedDemos: [] };
      // Shows `text` as a toast and on the line, and brings Start back.
      const stopWith = (text) => {
        showToast(text, 'error');
        precheck.stop(text);
      };

      // A batch from before Studio closed may still be capturing in the open
      // game, and a new one would run in that game and stop it (#20).
      if (isBatchStillRunningInGame()) {
        stopWith(STRINGS.BATCH_CATCH_UP.STILL_RUNNING_NOT_STARTED);
        return;
      }

      // OBS mode: verify live, right before committing to a batch, rather
      // than trusting whatever a check found minutes (or a whole session)
      // ago — OBS may not have even been open the last time anything
      // checked. This is the actual connectivity check now; see
      // runObsConnectionTest's own doc comment for why it isn't run
      // proactively at app startup instead. If it's not reachable, launch
      // it (much like HLAE doesn't need to already be open either) and wait
      // for it to come up before giving up.
      if (document.querySelector('#config-capture-mode')?.value === 'obs') {
        precheck.step('obs');
        await runObsConnectionTest();
        if (!isObsConnected() && !(await launchObsAndRetry())) {
          stopWith(STRINGS.CAPTURE.OBS_NOT_CONNECTED_WARNING);
          return;
        }
      }

      // Hard safety gate — recomputed fresh on every click regardless of the
      // button's current disabled state, so a stale/unrefreshed guard can
      // never let a capture start without sufficient disk space (this is
      // also what closes the old zero-drive bypass: `availableBytes === 0`
      // now blocks unconditionally instead of skipping the check).
      precheck.step('disk');
      const guard = await refreshLaunchGuard(state);
      if (guard && guard.blocked) {
        // Ahead of the disk branches, and well ahead of the running-game check
        // below: "you haven't picked anything yet" is the answer the user needs,
        // and it should not arrive behind a modal about closing their game.
        if (guard.noHighlightsSelected) {
          stopWith(STRINGS.CAPTURE.NO_HIGHLIGHTS_SELECTED_WARNING);
        } else if (guard.availableBytes === 0) {
          stopWith(STRINGS.CAPTURE.NO_CAPTURE_OUTPUT_DIR_WITH_SPACE);
        } else {
          stopWith(STRINGS.CAPTURE.insufficientDiskSpaceToast((guard.requiredBytes / 1e9).toFixed(2), (guard.availableBytes / 1e9).toFixed(2)));
        }
        return;
      }

      // A demo may have moved since the queue was loaded (#21). main.js looks
      // for it the same way Load Project does and offers the match; a demo
      // still missing after that stops the batch, since its highlights
      // cannot be captured. A picked demo with more than one map stops it
      // too (#217); main.js has said why then ('handled').
      precheck.step('demos');
      const demosReady = pickedDemosPresent
        ? await pickedDemosPresent({ onProgress: (p) => precheck.step('demos', p?.done, p?.total) })
        : true;
      if (demosReady !== true) {
        if (demosReady === false) stopWith(STRINGS.CAPTURE.DEMOS_MISSING_NOT_STARTED);
        return;
      }

      // A map a session crashed on before, for a reason the demo file can't
      // show (#207): ask before patching.
      precheck.step('maps');
      const pickedPaths = (state.currentScannedDemos || [])
        .filter((d) => (d.streaks || []).some((s) => s.selected === true))
        .map((d) => d.path);
      if (!(await confirmCrashMaps(pickedPaths))) return;
      // A demo with more entities in a snapshot than this game's engine takes
      // closes the game when it gets there (#207). Ask before patching.
      const picked = (state.currentScannedDemos || []).filter((d) => (d.streaks || []).some((s) => s.selected === true));
      const hlPathNow = document.querySelector('#hl-path-input')?.value?.trim() || '';
      if (!(await confirmOverLimit(picked, hlPathNow))) return;
      // One with no camera data captures a frozen view (#920).
      if (!(await confirmNoCameraData(picked))) return;

      precheck.step('paths');
      const activePayload = buildCapturePayload(state);
      if (!activePayload) return; // buildCapturePayload already toasted the reason

      try {
        await validatePaths(activePayload.hlae_path, activePayload.game_path);
      } catch (err) {
        console.error("Executable path validation failed:", err);
        stopWith(String(err));
        return;
      }

      // Day of Defeat allows one instance, and the batch does not find that out
      // until after it has patched every demo in the queue — the engine's own
      // "Only one instance of this game can be run at a time" box appears at
      // the end of all that work, with nothing captured. The preview and
      // standalone launches are guarded against this the same way.
      //
      // One started with other launch settings gets its own ask first, naming
      // what differs (#666).
      const launchRequest = {
        hlae_path: activePayload.hlae_path,
        game_path: activePayload.game_path,
        goldsrc_hooks_dll_path: activePayload.goldsrc_hooks_dll_path,
        resolution_width: activePayload.resolution_width,
        resolution_height: activePayload.resolution_height,
      };
      precheck.step('game');
      if (!(await closeGameWithOtherSettings(launchRequest, { button: startBtn }))) {
        precheck.stop(STRINGS.RUNNING_GAME.BATCH_NOT_STARTED_STATUS);
        return;
      }
      let engineAlreadyRunning = false;
      try {
        engineAlreadyRunning = await checkEngineProcesses();
      } catch (err) {
        // Already toasted by ipc_bridge.js — fail open rather than blocking a
        // legitimate batch because the detector itself errored.
      }

      const runBatch = () => {
        precheck.handOver();
        showToast(STRINGS.CAPTURE.INITIALIZING_CAPTURE_BATCH, "info");
        startBtn.disabled = true;
        if (cancelBtn) cancelBtn.disabled = false;
        return startBatchWithPayload(activePayload, state);
      };

      if (engineAlreadyRunning) {
        // A Seek Between Clips batch runs in an open game Studio started with
        // the same hook DLL, instead of asking to relaunch it (#725).
        const reusePid = await runningGameForBatch(
          launchRequest,
          activePayload.capture_mode,
          activePayload.hook_runs_batch,
        );
        if (reusePid) {
          showToast(STRINGS.CAPTURE.BATCH_IN_OPEN_GAME_TOAST, "info");
          activePayload.reuse_game_pid = reusePid;
          runBatch();
          return;
        }
        // Its window asks what to do and starts the batch itself; Start comes
        // back behind it in case the answer is no.
        precheck.stop(STRINGS.CAPTURE_PRECHECK.GAME_OPEN);
        requestProcessGuardedLaunch(runBatch, { forBatch: true });
        return;
      }

      // Before any patching: without Steam the game can't start.
      precheck.step('steam');
      if (!(await ensureSteamReady())) {
        precheck.stop(STRINGS.STEAM.BATCH_NOT_STARTED_STATUS);
        return;
      }
      runBatch();
    };
  }

  // Split out of the click handler so the process-conflict modal can park it
  // and run it later unchanged.
  function startBatchWithPayload(activePayload, state) {
    return startCaptureBatch(activePayload)
        // No "queued" toast: this resolves before the backend has built the
        // queue, which can still refuse the batch a moment later (#1097). The
        // footer shows the batch from its first capture_status.
        .then(() => {
          setBatchRunning(true);
        })
        .catch((err) => {
          console.error("IPC Execution Error (start_capture_batch):", err);
          showToast(STRINGS.CAPTURE.startBatchError(err), "error");
          if (cancelBtn) cancelBtn.disabled = true;
          capturingInFlight = false;
          // The batch never started, so the reminder must not be left standing.
          setBatchRunning(false);
          refreshLaunchGuard(state);
        });
  }

  if (cancelBtn) {
    cancelBtn.addEventListener('click', () => {
      showToast(STRINGS.CAPTURE.CANCELLING_BATCH_TOAST, "info");
      cancelBtn.disabled = true;
      cancelCaptureBatch()
        .then(() => {
          capturingInFlight = false;
          refreshLaunchGuard();
        })
        .catch((err) => {
          console.error("IPC Execution Error (cancel_capture_batch):", err);
          capturingInFlight = false;
          refreshLaunchGuard();
        });
    });
  }
}
