// obs_capture_test.js
// "Test OBS capture" and "Find my highest steady FPS" (#646), beside the OBS
// settings. Each test clip is a normal one-clip OBS batch of the demo that
// ships with Studio (`obs_test_clip`), at the current settings, so what it
// measures is what a real batch would do. While it records, the backend
// watches OBS's frame counters; afterwards it reads the file for repeated
// frames and sound (`obs_test_analyze`), and the clip is deleted. The
// decisions live in obs_capture_test_logic.js.

import { listen } from '@tauri-apps/api/event';
import {
  startCaptureBatch, cancelCaptureBatch, checkEngineProcesses,
  obsTestClip, obsTestWatchStart, obsTestAnalyze, obsTestCancel, obsTestDeleteTake,
} from './ipc_bridge.js';
import { batchSettingsPayload, runObsConnectionTest, launchObsAndRetry, isCaptureRunning } from './capture_pane.js';
import { isObsConnected } from './obs_status.js';
import { ensureSteamReady } from './steam_guard.js';
import { setObsTestBatch } from './obs_test_flag.js';
import { numberField } from './number_field.js';
import { STRINGS } from './strings.js';
import {
  FPS_LADDER, CLIP_SECONDS, pct, levelName, ladderStops, highestSteady, ladderSummary, stepLines, testSessionId,
} from './obs_capture_test_logic.js';

/** How long after the batch's last status line its takes may still arrive. */
const TAKES_GRACE_MS = 5000;

/**
 * Wires the test buttons. `getState()` gives `{ targetDrives }` (Capture
 * Output, where the test clip is recorded before it's deleted); `persist()`
 * saves settings after "Use N fps".
 */
export function createObsCaptureTest({ getState, persist }) {
  const el = (id) => document.getElementById(id);
  const testBtn = el('obs-capture-test-btn');
  const ladderBtn = el('obs-fps-ladder-btn');
  const stopBtn = el('obs-test-stop-btn');
  const longBox = el('obs-test-long');
  const panel = el('obs-test-panel');
  const stepEl = el('obs-test-step');
  const bar = el('obs-test-bar');
  const barFill = el('obs-test-bar-fill');
  const resultEl = el('obs-test-result');
  const tbody = el('obs-test-rows');
  const table = el('obs-test-table');
  const applyBtn = el('obs-test-apply-btn');
  if (!testBtn || !ladderBtn || !panel) return { isRunning: () => false };

  let running = false;
  let stopRequested = false;
  let batchActive = false;
  let applyRate = null;

  // Each step names itself; `done`/`total` give the bar real progress, and
  // without them it slides (the step can't be measured).
  const step = (text, done = null, total = null) => {
    stepEl.textContent = text;
    bar.hidden = false;
    const measured = Number.isFinite(done) && Number.isFinite(total) && total > 0;
    bar.classList.toggle('indeterminate', !measured);
    barFill.style.width = measured ? `${Math.min(100, (done / total) * 100)}%` : '';
  };

  const setRunning = (value) => {
    running = value;
    testBtn.disabled = value;
    ladderBtn.disabled = value;
    if (longBox) longBox.disabled = value;
    stopBtn.hidden = !value;
    stopBtn.disabled = false;
    if (!value) {
      bar.hidden = true;
      stepEl.textContent = '';
    }
  };

  const showResult = (lines, kind = 'info') => {
    resultEl.textContent = lines.join('\n');
    resultEl.dataset.kind = kind;
  };

  const addRow = ({ rate, verdict, file }) => {
    table.hidden = false;
    const tr = document.createElement('tr');
    tr.dataset.level = verdict.level;
    const sound = verdict.sound === 'ok' ? STRINGS.OBS_TEST.SOUND_CELL_OK
      : verdict.sound === 'missing' ? STRINGS.OBS_TEST.SOUND_CELL_MISSING : STRINGS.OBS_TEST.SOUND_CELL_SILENT;
    [String(rate), levelName(verdict.level), pct(verdict.skipped_share), pct(verdict.missed_share),
      pct(verdict.repeated_share), file?.audio_peak_db != null && verdict.sound === 'ok' ? `${sound} (${Number(file.audio_peak_db).toFixed(1)} dB)` : sound]
      .forEach((text) => {
        const td = document.createElement('td');
        td.textContent = text;
        tr.appendChild(td);
      });
    tbody.appendChild(tr);
  };

  const obsConnection = () => ({
    host: document.querySelector('#config-obs-host')?.value?.trim() || '127.0.0.1',
    port: parseInt(document.querySelector('#config-obs-port')?.value, 10) || 4455,
    password: document.querySelector('#config-obs-password')?.value || '',
  });

  // The checks Start Capture Batch makes that apply here. Returns the reason
  // the test can't run, or null.
  const precheck = async () => {
    if (isCaptureRunning()) return STRINGS.OBS_TEST.BATCH_RUNNING;
    step(STRINGS.OBS_TEST.CHECKING_OBS);
    await runObsConnectionTest({ auto: true });
    if (!isObsConnected() && !(await launchObsAndRetry())) return STRINGS.OBS_TEST.OBS_NOT_CONNECTED;
    step(STRINGS.OBS_TEST.CHECKING_GAME);
    let gameOpen = false;
    try {
      gameOpen = await checkEngineProcesses();
    } catch {
      // Toasted by ipc_bridge.js; the batch's own check still runs.
    }
    if (gameOpen) return STRINGS.OBS_TEST.GAME_OPEN;
    if (!(await ensureSteamReady())) return STRINGS.STEAM.BATCH_NOT_STARTED_STATUS;
    return null;
  };

  // Runs the clip's batch and resolves with `{ status, takes }` once it ends:
  // `takes` is its `obs_test_takes_verified` payload, null when none came.
  const runBatch = async (payload, rate, prefix) => {
    const unlisten = [];
    let resolveEnd;
    const ended = new Promise((resolve) => { resolveEnd = resolve; });
    let lastStatus = null;
    let grace = null;
    const finish = (takes) => {
      clearTimeout(grace);
      resolveEnd({ status: lastStatus, takes });
    };
    try {
      unlisten.push(await listen('obs_test_takes_verified', (event) => {
        if (event.payload?.session_id === payload.session_id) finish(event.payload);
      }));
      unlisten.push(await listen('capture_status', (event) => {
        const p = event.payload || {};
        if (p.running) {
          step(prefix + STRINGS.OBS_TEST.recording(rate, p.status));
        } else {
          lastStatus = p;
          grace = setTimeout(() => finish(null), TAKES_GRACE_MS);
        }
      }));
      setObsTestBatch(true);
      batchActive = true;
      await startCaptureBatch(payload);
      return await ended;
    } finally {
      batchActive = false;
      setObsTestBatch(false);
      unlisten.forEach((u) => u());
    }
  };

  // One test clip at `rate`: `{ rate, verdict, file, recording }`, or
  // `{ rate, error }`.
  const runClip = async (rate, prefix) => {
    step(prefix + STRINGS.OBS_TEST.PREPARING_CLIP);
    const settings = batchSettingsPayload(getState ? getState() : {});
    if (!settings) return { rate, error: STRINGS.CAPTURE.NO_CAPTURE_OUTPUT_DIR };
    const seconds = longBox?.checked ? CLIP_SECONDS.long : CLIP_SECONDS.short;
    let takeFolder = null;
    try {
      // `data-demo` on the panel swaps in another demo: not in the UI, it is
      // how a run is compared against a real match's demo.
      const clip = await obsTestClip(seconds, panel.dataset.demo || null);
      const payload = {
        ...settings,
        capture_mode: 'obs',
        obs_capture_fps: rate,
        streaks: [clip.streak],
        pre_roll_seconds: clip.pre_roll,
        post_roll_seconds: clip.post_roll,
        session_id: testSessionId(rate),
      };
      const conn = obsConnection();
      await obsTestWatchStart(conn.host, conn.port, conn.password);
      const { status, takes } = await runBatch(payload, rate, prefix);
      if (stopRequested) return { rate, error: STRINGS.OBS_TEST.STOPPED };
      const block = (takes?.blocks || [])[0];
      takeFolder = block?.take_folder || null;
      if (!block?.captured) {
        return { rate, error: takes ? STRINGS.OBS_TEST.NO_TAKE : STRINGS.OBS_TEST.batchEnded(status?.status || '') };
      }
      step(prefix + STRINGS.OBS_TEST.readingClip(0, 0));
      const unlistenProgress = await listen('obs_test_progress', (event) => {
        const p = event.payload || {};
        step(prefix + STRINGS.OBS_TEST.readingClip(p.done || 0, p.total || 0), p.done, p.total);
      });
      let result;
      try {
        result = await obsTestAnalyze(takeFolder, document.querySelector('#ffmpeg-override-path-input')?.value?.trim() || null);
      } finally {
        unlistenProgress();
      }
      return { rate, ...result };
    } catch (err) {
      return { rate, error: stopRequested ? STRINGS.OBS_TEST.STOPPED : String(err) };
    } finally {
      obsTestCancel().catch(() => {});
      if (takeFolder) {
        step(prefix + STRINGS.OBS_TEST.DELETING);
        await obsTestDeleteTake(takeFolder).catch((err) => console.error('OBS test: delete the clip:', err));
      }
    }
  };

  const quality = () => document.querySelector('#config-obs-recording-quality')?.value || 'standard';

  const begin = async () => {
    stopRequested = false;
    applyRate = null;
    applyBtn.hidden = true;
    tbody.replaceChildren();
    table.hidden = true;
    resultEl.textContent = '';
    panel.hidden = false;
    setRunning(true);
    const refusal = await precheck();
    if (refusal) {
      showResult([refusal], 'error');
      setRunning(false);
      return false;
    }
    return true;
  };

  testBtn.addEventListener('click', async () => {
    if (running || !(await begin())) return;
    const rate = numberField('#config-obs-capture-fps', 120, { integer: true, positive: true });
    try {
      const r = await runClip(rate, '');
      if (r.error) {
        showResult([STRINGS.OBS_TEST.failed(r.error)], 'error');
      } else {
        addRow(r);
        showResult(stepLines(r, quality()), r.verdict.level);
      }
    } finally {
      setRunning(false);
    }
  });

  ladderBtn.addEventListener('click', async () => {
    if (running || !(await begin())) return;
    const q = quality();
    const results = [];
    try {
      for (let i = 0; i < FPS_LADDER.length && !stopRequested; i++) {
        const rate = FPS_LADDER[i];
        const r = await runClip(rate, STRINGS.OBS_TEST.ladderStep(i + 1, FPS_LADDER.length, rate));
        results.push(r);
        if (r.verdict) addRow(r);
        if (ladderStops(results)) break;
      }
      const lines = ladderSummary(results, q);
      const err = results.find((r) => r.error);
      if (err) lines.push(STRINGS.OBS_TEST.failed(err.error));
      const best = highestSteady(results);
      showResult(lines, best == null ? 'not_steady' : 'steady');
      const current = numberField('#config-obs-capture-fps', 120, { integer: true, positive: true });
      if (best != null && best !== current) {
        applyRate = best;
        applyBtn.textContent = STRINGS.OBS_TEST.applyButton(best);
        applyBtn.hidden = false;
      }
    } finally {
      // OBS's profile was set to each rate in turn: back to the user's.
      step(STRINGS.OBS_TEST.RESTORING);
      await runObsConnectionTest({ auto: true }).catch(() => {});
      setRunning(false);
    }
  });

  stopBtn.addEventListener('click', () => {
    stopRequested = true;
    stopBtn.disabled = true;
    obsTestCancel().catch(() => {});
    if (batchActive) cancelCaptureBatch().catch(() => {});
  });

  applyBtn.addEventListener('click', () => {
    const input = document.querySelector('#config-obs-capture-fps');
    if (!input || applyRate == null) return;
    input.value = String(applyRate);
    ['input', 'change'].forEach((t) => input.dispatchEvent(new Event(t, { bubbles: true })));
    if (persist) persist();
    showResult([...resultEl.textContent.split('\n'), STRINGS.OBS_TEST.applied(applyRate)], 'steady');
    applyBtn.hidden = true;
    runObsConnectionTest({ auto: true }).catch(() => {});
  });

  return { isRunning: () => running };
}
