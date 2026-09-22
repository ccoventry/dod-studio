import { listen } from '@tauri-apps/api/event';
import {
  queueRenderBatch,
  startQueuedRender,
  cancelRenderBatch,
  cancelRenderJob,
  resetRenderJob,
  resetAllRenderJobs,
  removeRenderJob,
  removeNonRenderingRenderJobs,
  setRenderJobCodec,
  getExportPoolFreeGb,
  getRenderRequiredEstimateGb,
  checkRenderAutosave,
  discardRenderAutosave,
  recoverRenderBatch,
  revealInExplorer,
} from './ipc_bridge.js';
import { showToast } from './toast.js';
import { streakUid, resolveTake } from './take_index.js';
import { STRINGS } from './strings.js';
import { notify } from './os_notifications.js';

let jobs = []; // RenderJobView[] — latest snapshot from 'render_jobs_snapshot'
// id:status pairs from the last snapshot Export Pool Free/Required
// (Estimated) were refreshed against — lets the snapshot listener tell "a
// job was added/removed/changed status" (worth a refresh) apart from "only
// progress% ticked" (not worth one), without a blind polling interval.
let lastJobsFingerprint = null;

function esc(s) {
  return String(s ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

/** The batch panel's currently-selected codec, defaulting like the select's own first option. */
function getSelectedCodec() {
  return document.querySelector('#render-codec-select')?.value || 'prores';
}

function statusColor(status) {
  switch (status) {
    case 'Finished': return '#4caf50';
    case 'Error': return '#f44336';
    case 'Cancelled': return '#ffeb3b';
    case 'Rendering': return '#7ec8e3';
    default: return '#ffffff';
  }
}

/**
 * One sentence describing how a finished batch actually ended.
 *
 * Reports every non-zero outcome rather than picking a single label, because
 * mixed batches are the normal case: cancelling the takes you did not want and
 * letting the rest run should read as a success with a note, not as "the batch
 * was cancelled". Falls back to the single-outcome wording when only one kind
 * of thing happened, so the common all-finished case stays as short as before.
 */
function summarizeBatchOutcome(finished, failed, cancelled) {
  const parts = [];
  if (finished > 0) parts.push(STRINGS.RENDER.countRendered(finished));
  if (failed > 0) parts.push(STRINGS.RENDER.countFailed(failed));
  if (cancelled > 0) parts.push(STRINGS.RENDER.countCancelled(cancelled));

  if (parts.length === 0) return STRINGS.RENDER.BATCH_COMPLETED;
  if (parts.length === 1) {
    if (failed === 0 && cancelled === 0) return STRINGS.RENDER.BATCH_COMPLETED;
    if (finished === 0 && failed === 0) return STRINGS.RENDER.BATCH_CANCELLED;
  }
  return STRINGS.RENDER.batchSummary(parts.join(', '));
}

function updateFooterQueueSummary() {
  const el = document.querySelector('#render-footer-queue-summary');
  if (!el) return;
  const queued = jobs.filter((j) => j.status === 'Queued').length;
  const rendering = jobs.filter((j) => j.status === 'Rendering').length;
  const done = jobs.filter((j) => j.status === 'Finished' || j.status === 'Error' || j.status === 'Cancelled').length;
  el.textContent = STRINGS.RENDER.queueSummary(queued, rendering, done);
}

/**
 * The Actions cell's buttons, as HTML — kept separate from the row template
 * so it can be rebuilt on its own (see `updateJobRow`).
 */
function actionsCellHtml(j) {
  let html = '';
  if (j.status === 'Rendering' || j.status === 'Queued') {
    html += `<button class="render-job-cancel-btn" data-job-id="${esc(j.id)}" title="${STRINGS.RENDER.CANCEL_JOB_TITLE}">✖</button>`;
  } else if (j.status === 'Cancelled' || j.status === 'Finished' || j.status === 'Error') {
    html += `<button class="render-job-reset-btn" data-job-id="${esc(j.id)}" title="${STRINGS.RENDER.RESET_JOB_TITLE}">🔄</button>`;
  }
  // Coexists with either button above — a Queued job can be removed directly
  // without cancelling it first, same as a Cancelled/Finished/Error one. Only
  // withheld while actually Rendering, where removing the row out from under
  // a live ffmpeg process has no defined outcome.
  if (j.status !== 'Rendering') {
    html += `<button class="render-job-remove-btn" data-job-id="${esc(j.id)}" title="${STRINGS.RENDER.REMOVE_JOB_TITLE}">🗑</button>`;
  }
  if (j.error_log) {
    html += `<button class="render-job-view-log-btn" data-job-id="${esc(j.id)}" title="${STRINGS.RENDER.VIEW_LOG_TITLE}">${STRINGS.RENDER.VIEW_LOG_BUTTON}</button>`;
  }
  if ((j.status === 'Finished' && j.output_path) || j.take_folder) {
    const useOutput = j.status === 'Finished' && j.output_path;
    html += `<button class="render-job-reveal-btn" data-job-id="${esc(j.id)}" title="${useOutput ? STRINGS.RENDER.OPEN_OUTPUT_FOLDER_TITLE : STRINGS.RENDER.OPEN_TAKE_FOLDER_TITLE}">${useOutput ? STRINGS.RENDER.OPEN_OUTPUT_BUTTON : STRINGS.RENDER.OPEN_TAKE_FOLDER_BUTTON}</button>`;
  }
  return html;
}

function wireActionsCell(cell) {
  cell.querySelectorAll('.render-job-cancel-btn').forEach((btn) => {
    btn.addEventListener('click', () => cancelRenderJob(btn.dataset.jobId).catch(() => {}));
  });
  cell.querySelectorAll('.render-job-reset-btn').forEach((btn) => {
    btn.addEventListener('click', () => resetRenderJob(btn.dataset.jobId).catch(() => {}));
  });
  cell.querySelectorAll('.render-job-remove-btn').forEach((btn) => {
    btn.addEventListener('click', () => removeRenderJob(btn.dataset.jobId).catch(() => {}));
  });
  cell.querySelectorAll('.render-job-reveal-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      const job = jobs.find((j) => j.id === btn.dataset.jobId);
      if (!job) return;
      const target = (job.status === 'Finished' && job.output_path) ? job.output_path : job.take_folder;
      if (target) revealInExplorer(target).catch(() => {});
    });
  });
  cell.querySelectorAll('.render-job-view-log-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      const job = jobs.find((j) => j.id === btn.dataset.jobId);
      const modal = document.querySelector('#render-error-log-modal');
      const title = document.querySelector('#render-error-log-title');
      const body = document.querySelector('#render-error-log-body');
      if (title) title.textContent = job ? STRINGS.RENDER.errorLogTitleForJob(job.name) : STRINGS.RENDER.ERROR_LOG_TITLE_DEFAULT;
      if (body) body.textContent = (job && job.error_log) || '';
      if (modal) modal.style.display = 'flex';
    });
  });
}

function settingsCellHtml(j) {
  const showSkipToggle = j.skip_available && j.status === 'Queued';
  const toggle = showSkipToggle
    ? `<label class="render-skip-toggle" title="${STRINGS.RENDER.SKIP_TOGGLE_TITLE}" style="margin-left:6px; font-size:11px; white-space:nowrap;">
         <input type="checkbox" class="render-job-skip-checkbox" data-job-id="${esc(j.id)}" ${j.codec_id === 'source_copy' ? 'checked' : ''} /> ${STRINGS.RENDER.SKIP_TOGGLE_LABEL}
       </label>`
    : '';
  return `${esc(j.settings_summary)}${toggle}`;
}

function wireSettingsCell(cell) {
  cell.querySelectorAll('.render-job-skip-checkbox').forEach((cb) => {
    cb.addEventListener('change', () => {
      const jobId = cb.dataset.jobId;
      const wasChecked = !cb.checked;
      // Unchecking restores *this job's own* codec from before Skip was
      // ticked on (tracked in `updateJobRow` as `row.dataset.lastCodec`) —
      // not whatever the batch panel's dropdown happens to show right now,
      // which may have been changed since this job was queued. Matches the
      // VirtualDub-style guarantee `reset_render_job` already documents: a
      // queued job's settings never silently pick up a later panel change.
      const row = cb.closest('tr');
      const codecVal = cb.checked ? 'source_copy' : (row?.dataset.lastCodec || getSelectedCodec());
      cb.disabled = true;
      setRenderJobCodec(jobId, codecVal)
        .catch((err) => {
          showToast(STRINGS.RENDER.setJobCodecFailed(err), 'error');
          cb.checked = wasChecked;
        })
        .finally(() => { cb.disabled = false; });
    });
  });
}

/** The real encoded size once a job is Finished — a dash otherwise. Never an
 *  estimate: see TABLE_HEADER_FILE_SIZE_TITLE for why. */
function formatFileSize(bytes) {
  if (bytes == null) return STRINGS.RENDER.FILE_SIZE_UNKNOWN;
  const gb = bytes / (1024 * 1024 * 1024);
  return gb >= 1
    ? STRINGS.RENDER.fileSizeGb(gb.toFixed(2))
    : STRINGS.RENDER.fileSizeMb((bytes / (1024 * 1024)).toFixed(0));
}

/** Builds one job's `<tr>` fresh — only ever called once per job id. */
function createJobRow(j) {
  const row = document.createElement('tr');
  row.dataset.jobId = j.id;
  row.innerHTML = `
    <td class="rj-name"></td>
    <td class="rj-stream"></td>
    <td class="rj-frames"></td>
    <td class="rj-date"></td>
    <td class="rj-settings"></td>
    <td class="rj-status"></td>
    <td class="rj-speed"></td>
    <td><div class="progress-bar-container" style="margin-top:0;"><div class="progress-bar-fill rj-progress-fill" style="width:0%;"></div></div></td>
    <td class="rj-file-size"></td>
    <td class="rj-actions"></td>`;
  updateJobRow(row, j);
  return row;
}

/**
 * Patches one job's existing `<tr>` in place rather than replacing it.
 *
 * #80: rebuilding every row's `innerHTML` on every `render_jobs_snapshot` —
 * which fires ~6-7 times a second while anything is actively rendering —
 * destroyed and recreated the Cancel/Reset/etc. buttons that often, so a
 * click landing mid-rebuild could target a button that no longer existed.
 * The settings (skip toggle) and actions cells are the only ones holding
 * interactive elements, so those are the only ones conditionally rebuilt —
 * and even then, only when what they need to show actually changed. Every
 * other cell is plain text/style, which is safe to overwrite unconditionally
 * since it never had a click in flight to lose.
 */
function updateJobRow(row, j) {
  row.querySelector('.rj-name').textContent = j.name;
  row.querySelector('.rj-stream').textContent = j.stream;
  row.querySelector('.rj-frames').textContent = j.frames;
  row.querySelector('.rj-date').textContent = j.date;

  const statusCell = row.querySelector('.rj-status');
  statusCell.textContent = j.status;
  statusCell.style.color = statusColor(j.status);
  row.querySelector('.rj-speed').textContent = j.speed;
  row.querySelector('.rj-progress-fill').style.width = `${j.progress}%`;
  row.querySelector('.rj-file-size').textContent = formatFileSize(j.output_size_bytes);

  // Remembers the last codec this job actually rendered under (i.e. never
  // "source_copy" itself) so unchecking Skip later can restore exactly that,
  // not whatever the batch panel currently shows — see `wireSettingsCell`.
  if (j.codec_id !== 'source_copy') {
    row.dataset.lastCodec = j.codec_id;
  }

  const settingsKey = `${j.settings_summary}|${j.skip_available}|${j.status === 'Queued'}|${j.codec_id}`;
  if (settingsKey !== row.dataset.settingsKey) {
    const cell = row.querySelector('.rj-settings');
    cell.innerHTML = settingsCellHtml(j);
    wireSettingsCell(cell);
    row.dataset.settingsKey = settingsKey;
  }

  const actionsKey = `${j.status}|${!!j.error_log}|${j.output_path}|${j.take_folder}`;
  if (actionsKey !== row.dataset.actionsKey) {
    const cell = row.querySelector('.rj-actions');
    cell.innerHTML = actionsCellHtml(j);
    wireActionsCell(cell);
    row.dataset.actionsKey = actionsKey;
  }
}

function renderJobsTable() {
  updateFooterQueueSummary();
  const tbody = document.querySelector('#render-jobs-tbody');
  if (!tbody) return;

  if (jobs.length === 0) {
    tbody.innerHTML = `<tr><td colspan="10" class="table-empty">${STRINGS.RENDER.TABLE_EMPTY}</td></tr>`;
    return;
  }

  // The empty-state placeholder is not a job row and has to go before keyed
  // reconciliation below looks for `tr[data-job-id]` elements.
  if (tbody.querySelector('td.table-empty')) {
    tbody.innerHTML = '';
  }

  const existingRows = new Map();
  tbody.querySelectorAll('tr[data-job-id]').forEach((tr) => existingRows.set(tr.dataset.jobId, tr));

  // Job order is stable once a batch starts (`RenderJobRuntime`s are created
  // once, at fixed indices, and never reordered) — so existing rows never
  // need to move, only new ones need appending. That keeps every row that
  // already existed untouched by this pass, not just its buttons.
  jobs.forEach((j) => {
    const row = existingRows.get(j.id);
    if (row) {
      existingRows.delete(j.id);
      updateJobRow(row, j);
    } else {
      tbody.appendChild(createJobRow(j));
    }
  });
  // Anything left in the map is a row for a job id no longer in the
  // snapshot — should not normally happen within a batch, but a leftover
  // stale row would be worse than the cost of checking.
  existingRows.forEach((row) => row.remove());
}

function initErrorLogModal() {
  const modal = document.querySelector('#render-error-log-modal');
  const closeBtn = document.querySelector('#render-error-log-close-btn');
  closeBtn?.addEventListener('click', () => { if (modal) modal.style.display = 'none'; });
}

// ── JIT multi-drive export pool ──────────────────────────────────────────────

async function refreshExportPoolFree(getExportDirs) {
  const dirs = getExportDirs ? getExportDirs() : [];
  const freeEl = document.querySelector('#render-export-pool-free');
  const footerFreeEl = document.querySelector('#render-footer-pool-free');
  if (dirs.length === 0) {
    if (freeEl) freeEl.textContent = STRINGS.RENDER.EXPORT_POOL_FREE_DEFAULT;
    if (footerFreeEl) footerFreeEl.textContent = STRINGS.RENDER.RENDER_POOL_FREE_DEFAULT;
  } else {
    const gb = await getExportPoolFreeGb(dirs);
    if (freeEl) freeEl.textContent = STRINGS.RENDER.exportPoolFreeGb(gb.toFixed(1));
    if (footerFreeEl) footerFreeEl.textContent = STRINGS.RENDER.exportPoolFreeFooter(gb.toFixed(1));
  }

  // Issue #119: mirrors Capture's Free+Required footer pair. Independent of
  // getExportDirs — sums every Queued/Rendering job's own estimate
  // regardless of how many export directories are configured, so this isn't
  // gated on dirs.length the way the free-space half above is.
  const footerRequiredEl = document.querySelector('#render-footer-required-space');
  if (footerRequiredEl) {
    const requiredGb = (await getRenderRequiredEstimateGb()) ?? 0;
    footerRequiredEl.textContent = STRINGS.RENDER.requiredEstimatedFooter(requiredGb.toFixed(2));
    footerRequiredEl.title = STRINGS.RENDER.REQUIRED_ESTIMATED_FOOTER_TITLE;
  }
}

// ── Startup crash-recovery prompt ────────────────────────────────────────────

export async function checkRenderRecoveryOnStartup(onRecovered) {
  const summary = await checkRenderAutosave();
  if (!summary) return;

  const modal = document.querySelector('#render-recovery-modal');
  if (!modal) return;
  document.querySelector('#render-recovery-source').textContent = summary.source_folder || STRINGS.RENDER.UNKNOWN_SOURCE_FOLDER;
  document.querySelector('#render-recovery-completed').textContent = String(summary.completed_count);
  document.querySelector('#render-recovery-pending').textContent = String(summary.pending_count);
  modal.style.display = 'flex';

  document.querySelector('#render-recovery-recover-btn').addEventListener('click', async () => {
    try {
      jobs = await recoverRenderBatch();
      renderJobsTable();
      showToast(STRINGS.RENDER.recoveredJobsToast(summary.completed_count, summary.pending_count), 'info');
      if (onRecovered) onRecovered();
    } catch (err) {
      console.error('Error recovering render batch:', err);
      showToast(STRINGS.RENDER.recoverFailed(err), 'error');
    } finally {
      modal.style.display = 'none';
    }
  }, { once: true });

  document.querySelector('#render-recovery-discard-btn').addEventListener('click', async () => {
    try {
      await discardRenderAutosave();
    } catch (err) {
      console.error('Error discarding render autosave:', err);
    } finally {
      modal.style.display = 'none';
    }
  }, { once: true });
}

export function initRenderUI(getCaptureLocations, getExportDirs, onSettingsChange, takeTracking) {
  const scanRenderBtn = document.querySelector('#scan-render-btn');
  const startRenderBtn = document.querySelector('#start-render-btn');
  const cancelRenderBtn = document.querySelector('#cancel-render-btn');
  const resetAllRenderBtn = document.querySelector('#reset-all-render-btn');
  const removeAllRenderBtn = document.querySelector('#remove-all-render-btn');
  const renderStatusEl = document.querySelector('#render-status');

  const getTakeIndex = takeTracking?.getTakeIndex || null;
  const getAllDemos = takeTracking?.getAllDemos || null;
  const onTakeStatusChange = takeTracking?.onStatusChange || null;

  initErrorLogModal();

  // A take finishing renders — advances every highlight the take index says
  // fed that take_key to Rendered. The take index is what makes this work
  // even after a restart or re-scan replaced the original streak objects:
  // it was recorded by uid at capture time, not by a live object reference.
  listen('render_take_finished', (event) => {
    const { take_key: takeKey, take_folder: takeFolder } = event.payload || {};
    if (!takeKey) {
      // Folder has no session parent to derive a key from (e.g. copied out
      // of its session folder) — documented limitation, not an error.
      console.warn(`[render] Take finished with no resolvable take_key: ${takeFolder}`);
      return;
    }
    const takeIndex = getTakeIndex ? getTakeIndex() : null;
    const demos = getAllDemos ? getAllDemos() : null;
    if (!takeIndex || !demos) return;

    const uids = new Set(resolveTake(takeIndex, takeKey));
    // Logged unconditionally (hit or miss) so a lookup against a real,
    // already-loaded index is visible even when it resolves nothing — the
    // index's total entry count here is exactly what the previous
    // "[take-index] Loaded from ..." log reported, proving this lookup runs
    // against that same loaded data rather than something rebuilt in memory.
    console.log(`[take-index] Resolving ${takeKey} against ${Object.keys(takeIndex).length} loaded take(s) — found ${uids.size} uid(s)`, Array.from(uids));
    if (uids.size === 0) return;

    let advanced = 0;
    demos.forEach(demo => {
      (demo.streaks || []).forEach(streak => {
        if (!uids.has(streakUid(demo.path, streak))) return;
        // Idempotent: separate_hud produces two render jobs (all + hudcolor)
        // sharing one take_key, so this fires twice per take — the second
        // pass is just a no-op instead of a double-toast.
        if (streak.status === 'Rendered') return;
        streak.status = 'Rendered';
        advanced += 1;
      });
    });

    if (advanced > 0) {
      showToast(STRINGS.RENDER.highlightsMarkedRendered(advanced), 'success');
      if (onTakeStatusChange) onTakeStatusChange();
    }
  });

  // Real-time per-job state, pushed by the backend scheduler. This is the
  // single source of truth for whether a batch is actually running — driving
  // Start/Cancel off it (rather than only off the Start button's own click
  // handler and render_batch_finished) matters because reset_render_job can
  // resume the scheduler on its own, bypassing both of those. Without this,
  // Start Render Batch stayed clickable during a reset-triggered resume and
  // just repeatedly failed with "Render batch already in progress".
  listen('render_jobs_snapshot', (event) => {
    jobs = event.payload || [];
    renderJobsTable();
    const queuedCount = jobs.filter((j) => j.status === 'Queued').length;
    const renderingCount = jobs.filter((j) => j.status === 'Rendering').length;
    const activeOrQueued = queuedCount + renderingCount;
    if (renderStatusEl) {
      renderStatusEl.textContent = renderingCount > 0
        ? STRINGS.RENDER.renderingStatus(jobs.length - activeOrQueued, jobs.length)
        : (queuedCount > 0 ? STRINGS.RENDER.scanCompleteStatus(queuedCount) : STRINGS.RENDER.STATUS_WAITING);
    }
    // Start only makes sense on a staged-but-not-started batch: something
    // Queued, and nothing already Rendering. Scan is blocked for the same
    // window so a re-scan can't clobber jobs the user may have already
    // toggled Skip on — see queue_render_batch's own guard against this.
    if (scanRenderBtn) scanRenderBtn.disabled = activeOrQueued > 0;
    if (startRenderBtn) startRenderBtn.disabled = !(queuedCount > 0 && renderingCount === 0);
    if (cancelRenderBtn) cancelRenderBtn.disabled = activeOrQueued === 0;
    // Mirror each bulk button's per-row equivalent: Reset All wants at least
    // one Cancelled/Finished/Error row, Remove All wants at least one row
    // that isn't actively Rendering.
    const resettableCount = jobs.filter((j) => j.status === 'Cancelled' || j.status === 'Finished' || j.status === 'Error').length;
    if (resetAllRenderBtn) resetAllRenderBtn.disabled = resettableCount === 0;
    if (removeAllRenderBtn) removeAllRenderBtn.disabled = jobs.length - renderingCount === 0;

    // Export Pool Free / Required (Estimated) only change when a job is
    // added, removed, or crosses a status boundary (started, finished,
    // cancelled, reset) — not on every progress-percentage tick a snapshot
    // can carry. Comparing id:status fingerprints skips the refresh (and its
    // two IPC round-trips) on the ticks that can't have moved either figure.
    const fingerprint = jobs.map((j) => `${j.id}:${j.status}`).join('|');
    if (fingerprint !== lastJobsFingerprint) {
      lastJobsFingerprint = fingerprint;
      refreshExportPoolFree(getExportDirs);
    }
  });

  listen('render_batch_finished', () => {
    if (scanRenderBtn) scanRenderBtn.disabled = false;
    if (startRenderBtn) startRenderBtn.disabled = true;
    if (cancelRenderBtn) cancelRenderBtn.disabled = true;
    const resettableCount = jobs.filter((j) => j.status === 'Cancelled' || j.status === 'Finished' || j.status === 'Error').length;
    if (resetAllRenderBtn) resetAllRenderBtn.disabled = resettableCount === 0;
    if (removeAllRenderBtn) removeAllRenderBtn.disabled = jobs.length === 0;
    // Counted, not `some()`. The old check asked "was anything cancelled?"
    // before "did anything finish?", so one cancelled job in a batch of any
    // size reported the whole batch as cancelled — including the common case
    // of cancelling the takes you did not want and letting the rest run, where
    // it read as though nothing had rendered at all.
    const finished = jobs.filter((j) => j.status === 'Finished').length;
    const failed = jobs.filter((j) => j.status === 'Error').length;
    const cancelled = jobs.filter((j) => j.status === 'Cancelled').length;

    if (jobs.length > 0) {
      // Severity follows what actually happened rather than what is present:
      // a deliberate partial cancel where the rest rendered is a success, not
      // an "info" event. Only a real failure is an error.
      const level = failed > 0 ? 'error' : (finished > 0 ? 'success' : 'info');
      const summary = summarizeBatchOutcome(finished, failed, cancelled);
      showToast(summary, level);
      if (level !== 'info') {
        const kind = level === 'error' ? 'error' : 'renders_done';
        const title = level === 'error' ? STRINGS.NOTIFICATIONS.RENDERS_ERROR_TITLE : STRINGS.NOTIFICATIONS.RENDERS_DONE_TITLE;
        notify(kind, title, summary);
      }
    }
    if (renderStatusEl) renderStatusEl.textContent = STRINGS.RENDER.STATUS_FINISHED;
  });

  if (scanRenderBtn) {
    let scanFoundCount = 0;
    listen('render_scan_status', () => {
      // Payload is just "Found take: <name>" per take — the count is what
      // the status line needs, not the name, so this stays a cheap counter
      // rather than parsing/echoing every take's name into the DOM.
      scanFoundCount += 1;
      if (renderStatusEl) renderStatusEl.textContent = STRINGS.RENDER.scanFoundSoFar(scanFoundCount);
    });

    scanRenderBtn.addEventListener('click', () => {
      const captureLocations = getCaptureLocations ? getCaptureLocations() : [];
      if (!captureLocations || captureLocations.length === 0) {
        showToast(STRINGS.RENDER.ADD_RENDER_DIR_REQUIRED, 'error');
        return;
      }

      const codecVal = getSelectedCodec();
      const fpsVal = parseInt(document.querySelector('#render-fps-input')?.value, 10) || 300;
      const maxConcurrentVal = Math.min(8, Math.max(1, parseInt(document.querySelector('#render-max-concurrent-input')?.value, 10) || 2));
      checkNvencConcurrencyWarning();
      const exportDirs = (getExportDirs ? getExportDirs() : []).filter(Boolean);
      // FFmpeg override is shared with the capture config panel.
      const ffmpegPathVal = document.querySelector('#ffmpeg-override-path-input')?.value?.trim() || null;

      scanFoundCount = 0;
      showToast(STRINGS.RENDER.SCANNING_TOAST, 'info');
      scanRenderBtn.disabled = true;
      if (renderStatusEl) renderStatusEl.textContent = STRINGS.RENDER.STATUS_SCANNING;

      const renderPayload = {
        render_directories: captureLocations,
        codec: codecVal,
        fps: fpsVal,
        ffmpeg_path: ffmpegPathVal || null,
        export_directories: exportDirs,
        max_concurrent_renders: maxConcurrentVal,
      };

      // Populates the real job table (below) as Queued rows via the
      // render_jobs_snapshot event this emits — not a preview. Start Render
      // Batch is what actually kicks off the scheduler; this only stages it,
      // so there's a real window to review the batch or toggle Skip on an
      // OBS-shaped job before anything runs.
      queueRenderBatch(renderPayload)
        .then((count) => {
          showToast(STRINGS.RENDER.scannedTakesToast(count), count > 0 ? 'success' : 'info');
          if (renderStatusEl) {
            renderStatusEl.textContent = count > 0
              ? STRINGS.RENDER.scanCompleteStatus(count)
              : STRINGS.MAIN.statusGeneric(STRINGS.RENDER.NO_RENDER_TAKES_DETECTED);
          }
          if (count === 0) scanRenderBtn.disabled = false;
        })
        .catch((err) => {
          showToast(STRINGS.RENDER.scanDirError(err), 'error');
          if (renderStatusEl) renderStatusEl.textContent = STRINGS.RENDER.STATUS_SCAN_FAILED;
          scanRenderBtn.disabled = false;
        });
    });
  }

  if (startRenderBtn) {
    startRenderBtn.addEventListener('click', () => {
      showToast(STRINGS.RENDER.INITIALIZING_RENDER_BATCH, 'info');
      startRenderBtn.disabled = true;
      if (cancelRenderBtn) cancelRenderBtn.disabled = false;

      startQueuedRender()
        .then(() => {
          showToast(STRINGS.RENDER.RENDER_BATCH_QUEUED, 'success');
        })
        .catch((err) => {
          showToast(STRINGS.RENDER.renderBatchError(err), 'error');
          if (startRenderBtn) startRenderBtn.disabled = false;
          if (cancelRenderBtn) cancelRenderBtn.disabled = true;
        });
    });
  }

  if (cancelRenderBtn) {
    cancelRenderBtn.addEventListener('click', () => {
      showToast(STRINGS.RENDER.CANCELLING_RENDER_BATCH, 'info');
      cancelRenderBtn.disabled = true;
      cancelRenderBatch().catch((err) => {
        console.error('IPC Execution Error (cancelRenderBatch):', err);
        if (startRenderBtn) startRenderBtn.disabled = false;
      });
    });
  }

  if (resetAllRenderBtn) {
    resetAllRenderBtn.addEventListener('click', () => {
      resetAllRenderBtn.disabled = true;
      resetAllRenderJobs().catch((err) => {
        console.error('IPC Execution Error (resetAllRenderJobs):', err);
        resetAllRenderBtn.disabled = false;
      });
    });
  }

  if (removeAllRenderBtn) {
    removeAllRenderBtn.addEventListener('click', () => {
      removeAllRenderBtn.disabled = true;
      removeNonRenderingRenderJobs().catch((err) => {
        console.error('IPC Execution Error (removeNonRenderingRenderJobs):', err);
        removeAllRenderBtn.disabled = false;
      });
    });
  }

  // Codec/FPS/max-concurrent aren't read until Start Render Batch is
  // clicked, but they're still persisted settings — nothing previously
  // wired their edits to a save.
  const codecEl = document.querySelector('#render-codec-select');
  const maxConcurrentEl = document.querySelector('#render-max-concurrent-input');
  // NVENC's concurrent-session limit is unlocked on Quadro/RTX 40-series+
  // but commonly capped at 3-5 on consumer GeForce cards — exceeding it
  // doesn't fail upfront, it surfaces as an opaque FFmpeg error buried in
  // a job's error log. Warn eagerly rather than let that be a mystery.
  function checkNvencConcurrencyWarning() {
    const isNvenc = (codecEl?.value || '') === 'h264_nvenc';
    const maxConcurrent = parseInt(maxConcurrentEl?.value, 10) || 0;
    if (isNvenc && maxConcurrent > 3) {
      showToast(
        STRINGS.RENDER.nvencWarning(maxConcurrent),
        'warning',
        6000
      );
    }
  }
  if (onSettingsChange) {
    if (codecEl) codecEl.addEventListener('change', () => onSettingsChange());
    const fpsEl = document.querySelector('#render-fps-input');
    if (fpsEl) fpsEl.addEventListener('input', () => onSettingsChange());
    if (maxConcurrentEl) maxConcurrentEl.addEventListener('input', () => onSettingsChange());
  }
  if (codecEl) codecEl.addEventListener('change', checkNvencConcurrencyWarning);
  if (maxConcurrentEl) maxConcurrentEl.addEventListener('change', checkNvencConcurrencyWarning);

  // Paints once at startup (before any snapshot has arrived, so before
  // anything could have changed) — the `render_jobs_snapshot` listener above
  // takes over from here, refreshing on every add/remove/status change
  // instead of blindly polling.
  refreshExportPoolFree(getExportDirs);
}
