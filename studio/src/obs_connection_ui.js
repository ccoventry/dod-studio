import { launchObs, obsTestConnection } from './ipc_bridge.js';
import { isObsConnected, setObsConnected } from './obs_status.js';
import { numberField } from './number_field.js';
import { STRINGS } from './strings.js';

// Start Capture Batch's gate (capture_pane.js refreshLaunchGuard) is handed in
// by initCaptureUI: importing it here would make this module and capture_pane.js
// import each other.
let afterCheck = () => {};
export function setObsCheckListener(fn) {
  afterCheck = typeof fn === 'function' ? fn : () => {};
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
 * wired up) specifically so Start Capture Batch's click handler in
 * capture_pane.js can call it directly — main.js already imports from
 * capture_pane.js, so importing this back the other way would be circular.
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
    // trigger the next refresh of the gate.
    afterCheck();
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
