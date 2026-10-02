// finish_clips.js
//
// "When a batch finishes" (#440): after a capture batch verifies its takes,
// queue one Render Studio job per take and start it, so every clip ends up as
// one video with audio in the export folder without a Render tab visit.
//
// Deliberately not a second renderer. It stages the batch's own take folders
// through the very same `queue_render_batch` / `start_queued_render` pair the
// Render tab's Scan and Start buttons call, so the rows, Cancel/Reset, the
// `render_take_finished` -> Rendered status flip, and `.render_autosave.json`
// crash recovery all behave exactly as they do for a hand-started batch.
//
// Everything above `initFinishClips` is DOM-free so it can be unit tested
// (finish_clips.test.js); `initFinishClips` is the thin wiring to the page.

import { listen } from '@tauri-apps/api/event';
import { queueRenderBatch, startQueuedRender, revealInExplorer } from './ipc_bridge.js';
import { showToast } from './toast.js';
import { notify } from './os_notifications.js';
import { STRINGS } from './strings.js';
import { switchNavTab, setCaptureDetailSubtab } from './nav.js';

/** Finish-codec value meaning "whatever Render Settings' own codec is". */
export const FINISH_CODEC_RENDER_TAB = 'render_tab';

/**
 * The codec a batch captured in `mode` is finished with.
 *
 * `codecs` is `{ obs, video, frames }` (each a `RenderCodec` string id or
 * `render_tab`) plus the Render tab's own `renderCodec`/`renderCustomArgs`.
 * "Keep as captured" (`source_copy`) only exists for an OBS take — it is the
 * only mode whose take already carries its own audio — so it falls back to
 * the Render tab's codec for any other mode rather than queueing jobs that
 * `run_render_job` is certain to refuse.
 */
export function finishCodecFor(mode, codecs) {
  const pick = mode === 'obs' ? codecs.obs
    : mode === 'direct_to_video' ? codecs.video
      : codecs.frames;
  const renderTab = {
    codec: codecs.renderCodec || 'prores',
    customArgs: codecs.renderCodec === 'custom' ? (codecs.renderCustomArgs || '') : '',
  };
  if (!pick || pick === FINISH_CODEC_RENDER_TAB) return renderTab;
  if (pick === 'source_copy' && mode !== 'obs') return renderTab;
  return { codec: pick, customArgs: '' };
}

/** The take folders a verified batch left that Render Studio can render, once each. */
export function finishFolders(blocks) {
  const seen = new Set();
  const folders = [];
  (blocks || []).forEach((block) => {
    if (!block || !block.renderable || !block.take_folder) return;
    const key = normPath(block.take_folder);
    if (seen.has(key)) return;
    seen.add(key);
    folders.push(block.take_folder);
  });
  return folders;
}

function normPath(p) {
  return String(p || '').replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();
}

/**
 * Whether a Render job came from one of `folders`. A job's `take_folder` is
 * where the scanner found the take, which is usually the `take0000` folder
 * HLAE creates *inside* the block folder the batch planned, so this is a
 * path-prefix test rather than equality.
 */
export function jobInFolders(job, folders) {
  const jobPath = normPath(job?.take_folder);
  if (!jobPath) return false;
  return folders.some((folder) => {
    const f = normPath(folder);
    return f && (jobPath === f || jobPath.startsWith(`${f}\\`));
  });
}

/** Counts of this finish's own jobs by status, out of a Render jobs snapshot. */
export function finishProgress(jobs, folders) {
  const counts = { total: 0, queued: 0, rendering: 0, finished: 0, failed: 0, cancelled: 0 };
  (jobs || []).forEach((job) => {
    if (!jobInFolders(job, folders)) return;
    counts.total += 1;
    if (job.status === 'Queued') counts.queued += 1;
    else if (job.status === 'Rendering') counts.rendering += 1;
    else if (job.status === 'Finished') counts.finished += 1;
    else if (job.status === 'Error') counts.failed += 1;
    else if (job.status === 'Cancelled') counts.cancelled += 1;
  });
  counts.done = counts.finished + counts.failed + counts.cancelled;
  return counts;
}

/** Whether the Render tab has anything staged or running that a new batch would clobber. */
export function renderQueueBusy(jobs) {
  return (jobs || []).some((j) => j.status === 'Queued' || j.status === 'Rendering');
}

/**
 * Why a verified batch will not be finished automatically — `null` when it
 * will. `off` and `nothing` are silent (the setting is off, or no take is
 * renderable, which the verification toast already reported); `cancelled`
 * and `no_export_dir` are worth telling the user about.
 */
export function finishSkipReason({ enabled, outcome, folders, exportDirs }) {
  if (!enabled) return 'off';
  // A cancelled batch is one the user stopped; starting minutes of encoding
  // behind it is not what stopping asked for. Its takes stay for the Render tab.
  if (outcome === 'cancelled') return 'cancelled';
  if (!folders || folders.length === 0) return 'nothing';
  // With no export folder, run_render_job writes into the app's own working
  // directory — somewhere nobody would think to look.
  if (!exportDirs || exportDirs.length === 0) return 'no_export_dir';
  return null;
}

/**
 * The finish step's state machine, with every side effect injected so it can
 * be tested without Tauri or a DOM.
 *
 * Requests wait in order while the Render tab is busy — a running batch, or
 * one staged but not started (starting that would start somebody else's
 * batch, and queueing over it would throw it away). The next request goes the
 * moment a jobs snapshot shows the queue idle.
 *
 * deps:
 *   queue(payload) -> Promise<number>   queue_render_batch
 *   start() -> Promise<void>            start_queued_render
 *   onWaiting(request)                  a request is waiting on a busy Render tab
 *   onStarted(total)                    a request's jobs are queued and started
 *   onProgress(counts)                  an active finish's counts changed
 *   onDone(counts, jobs)                every job of the active finish ended
 *   onFailed(err)                       queueing or starting failed
 *   onNothingFound()                    the scan found no takes after all
 */
export function createFinishController(deps) {
  const pending = [];
  let active = null; // { folders, seen } — seen once a snapshot has shown its rows
  let completed = null; // folders of the last finish that ended
  let launching = false;
  let lastJobs = [];

  async function launch(request) {
    launching = true;
    let count = 0;
    try {
      count = await deps.queue(request.payload);
    } catch (err) {
      launching = false;
      deps.onFailed(err);
      pump();
      return;
    }
    if (!count) {
      launching = false;
      deps.onNothingFound();
      pump();
      return;
    }
    // Set before starting, not after: a batch of plain copies can finish
    // before start's reply comes back, and its snapshots must count.
    active = { folders: request.payload.render_directories, seen: false };
    try {
      await deps.start();
    } catch (err) {
      // The jobs stay Queued in the Render tab, where Start still works.
      active = null;
      launching = false;
      deps.onFailed(err);
      return;
    }
    launching = false;
    deps.onStarted(count);
    observe(lastJobs);
    pump();
  }

  function pump() {
    if (launching || active || pending.length === 0) return;
    if (renderQueueBusy(lastJobs)) {
      deps.onWaiting(pending[0]);
      return;
    }
    launch(pending.shift());
  }

  function observe(jobs) {
    if (!active || launching) return;
    const counts = finishProgress(jobs, active.folders);
    if (counts.total === 0) {
      // Every row was removed from the Render tab: nothing left to report on.
      // Only once they have been seen, though — the snapshot carrying the
      // new rows can arrive after queue_render_batch's own reply does.
      if (active.seen) active = null;
      return;
    }
    active.seen = true;
    if (counts.queued + counts.rendering === 0) {
      completed = active.folders;
      active = null;
      deps.onDone(counts, jobs);
      return;
    }
    deps.onProgress(counts);
  }

  return {
    /** `request` is `{ payload }`, the `queue_render_batch` payload to run. */
    request(request) {
      pending.push(request);
      pump();
    },
    /** Feed every `render_jobs_snapshot` payload here. */
    onJobsSnapshot(jobs) {
      lastJobs = jobs || [];
      // A finished batch's row reset from the Render tab is the Render tab's
      // own business again: its end-of-batch toast must not be skipped.
      if (!active && completed && renderQueueBusy(lastJobs)) completed = null;
      observe(lastJobs);
      pump();
    },
    /**
     * Whether every job in `jobs` belongs to the finish that just ended —
     * lets the Render tab skip its own end-of-batch toast when this one has
     * already said the same thing in the finish step's words.
     */
    ownsJobs(jobs) {
      if (!completed || !jobs || jobs.length === 0) return false;
      return jobs.every((j) => jobInFolders(j, completed));
    },
    isActive() {
      return !!active || launching || pending.length > 0;
    },
  };
}

// ── Page wiring ──────────────────────────────────────────────────────────────

let controller = null;
let getExportDirsFn = null;
let captureRunning = false;

/** Reads the finish settings and the Render tab's shared ones from the page. */
function readSettings() {
  const value = (sel, fallback) => document.querySelector(sel)?.value || fallback;
  return {
    enabled: value('#config-finish-clips', 'off') === 'finish',
    codecs: {
      obs: value('#config-finish-codec-obs', 'source_copy'),
      video: value('#config-finish-codec-video', FINISH_CODEC_RENDER_TAB),
      frames: value('#config-finish-codec-frames', FINISH_CODEC_RENDER_TAB),
      renderCodec: value('#render-codec-select', 'prores'),
      renderCustomArgs: document.querySelector('#render-custom-codec-input')?.value?.trim() || '',
    },
    renderFps: parseInt(document.querySelector('#render-fps-input')?.value, 10) || 300,
    maxConcurrent: Math.min(8, Math.max(1, parseInt(document.querySelector('#render-max-concurrent-input')?.value, 10) || 2)),
    ffmpegPath: document.querySelector('#ffmpeg-override-path-input')?.value?.trim() || null,
  };
}

/**
 * The Capture footer carries the finish step as its second phase — the same
 * status line and progress bar the capture itself used. Left alone while a
 * capture is running, which owns the footer until it ends.
 */
function setFooter(text, pct) {
  if (captureRunning) return;
  const statusEl = document.querySelector('#batch-status');
  const container = document.querySelector('#capture-progress-container');
  const bar = document.querySelector('#capture-progress-bar');
  if (statusEl) statusEl.textContent = text;
  if (pct != null) {
    if (container) container.style.display = 'block';
    if (bar) bar.style.width = `${Math.max(0, Math.min(100, Math.round(pct)))}%`;
  }
}

function openRenderTab() {
  switchNavTab('workspace');
  setCaptureDetailSubtab('render');
}

/**
 * Called by capture_pane.js's `capture_takes_verified` handler with that
 * event's payload plus the batch's capture mode. Does nothing unless "When a
 * batch finishes" is set to finish clips.
 */
export function requestFinishClips(verified, captureMode) {
  if (!controller) return;
  const settings = readSettings();
  const folders = finishFolders(verified?.blocks);
  const exportDirs = (getExportDirsFn ? getExportDirsFn() : []).filter(Boolean);
  const reason = finishSkipReason({ enabled: settings.enabled, outcome: verified?.outcome, folders, exportDirs });
  if (reason === 'cancelled') {
    showToast(STRINGS.FINISH.SKIPPED_CANCELLED, 'info', 6000);
    return;
  }
  if (reason === 'no_export_dir') {
    showToast(STRINGS.FINISH.SKIPPED_NO_EXPORT_DIR, 'warning', 8000);
    return;
  }
  if (reason) return;

  const { codec, customArgs } = finishCodecFor(captureMode, settings.codecs);
  if (codec === 'custom' && !customArgs) {
    showToast(STRINGS.FINISH.SKIPPED_CUSTOM_ARGS, 'warning', 8000);
    return;
  }
  const captureFps = Number(verified?.capture_fps) || 0;
  controller.request({
    payload: {
      render_directories: folders,
      codec,
      custom_codec_args: customArgs,
      // A frame sequence is timed by this number, so it must be the rate the
      // batch captured at, not whatever the Render tab happens to show.
      fps: captureFps > 0 ? captureFps : settings.renderFps,
      ffmpeg_path: settings.ffmpegPath,
      export_directories: exportDirs,
      max_concurrent_renders: settings.maxConcurrent,
    },
  });
}

/** See `createFinishController`'s `ownsJobs`. */
export function finishClipsOwnsJobs(jobs) {
  return controller ? controller.ownsJobs(jobs) : false;
}

/**
 * `getExportDirs` is main.js's Render export pool; `onSettingsChange`
 * persists settings when one of the finish fields changes.
 */
export function initFinishClips({ getExportDirs, onSettingsChange } = {}) {
  getExportDirsFn = getExportDirs || null;

  controller = createFinishController({
    queue: (payload) => queueRenderBatch(payload),
    start: () => startQueuedRender(),
    onWaiting: () => setFooter(STRINGS.FINISH.WAITING_STATUS, null),
    onStarted: (total) => {
      setFooter(STRINGS.FINISH.progressStatus(0, total, 0), 0);
      showToast(STRINGS.FINISH.startedToast(total), 'info', 8000, {
        action: { label: STRINGS.FINISH.VIEW_IN_RENDER_TAB, onClick: openRenderTab },
      });
    },
    onProgress: (c) => setFooter(STRINGS.FINISH.progressStatus(c.finished, c.total, c.rendering), (c.done / c.total) * 100),
    onDone: (c, jobs) => {
      const summary = STRINGS.FINISH.doneSummary(c.finished, c.failed, c.cancelled);
      setFooter(summary, 100);
      const firstOutput = (jobs || []).find((j) => j.status === 'Finished' && j.output_path)?.output_path;
      const level = c.failed > 0 ? 'error' : (c.finished > 0 ? 'success' : 'info');
      showToast(summary, level, 10000, firstOutput ? {
        action: { label: STRINGS.FINISH.OPEN_EXPORT_FOLDER, onClick: () => revealInExplorer(firstOutput).catch(() => {}) },
      } : {});
      if (level === 'error') notify('error', STRINGS.NOTIFICATIONS.RENDERS_ERROR_TITLE, summary);
      else if (level === 'success') notify('renders_done', STRINGS.FINISH.NOTIFY_TITLE, summary);
    },
    onFailed: (err) => {
      setFooter(STRINGS.FINISH.FAILED_STATUS, null);
      showToast(STRINGS.FINISH.failedToast(err), 'error', 8000);
    },
    onNothingFound: () => showToast(STRINGS.FINISH.NOTHING_FOUND, 'info'),
  });

  listen('render_jobs_snapshot', (event) => {
    controller.onJobsSnapshot(event.payload || []);
  }).catch((err) => console.error('Failed to register render_jobs_snapshot listener (finish):', err));

  listen('capture_status', (event) => {
    captureRunning = !!(event.payload || {}).running;
  }).catch((err) => console.error('Failed to register capture_status listener (finish):', err));

  if (onSettingsChange) {
    ['#config-finish-clips', '#config-finish-codec-obs', '#config-finish-codec-video', '#config-finish-codec-frames']
      .forEach((sel) => document.querySelector(sel)?.addEventListener('change', () => onSettingsChange()));
  }
}
