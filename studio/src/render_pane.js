import { listen } from '@tauri-apps/api/event';
import {
  queueRenderBatch,
  startQueuedRender,
  cancelRenderJob,
  resetRenderJob,
  removeRenderJob,
  cancelRenderJobs,
  resetRenderJobs,
  removeRenderJobs,
  clearRenderQueue,
  setRenderJobCodec,
  getExportPoolFreeGb,
  getRenderRequiredEstimateGb,
  checkRenderAutosave,
  discardRenderAutosave,
  recoverRenderBatch,
  getRenderJobs,
  revealInExplorer,
  writeTextFile,
  measureTakeFolders,
  deleteTakeFolders,
} from './ipc_bridge.js';
import { showToast } from './toast.js';
import { streakUid, resolveTake, setVerifiedStatus } from './take_index.js';
import { clipNamesForTakes, maxNameLength } from './clip_name.js';
import { getClipNameTemplate, clipNameTemplateErrors } from './clip_name_ui.js';
import { STRINGS } from './strings.js';
import { historySummary, historyLines } from './render_history.js';
import { sortJobs, nextSort, batchProgress, JOB_STATUS } from './render_jobs.js';
import { queueSummaryText, batchEndStatus } from './render_batch_status.js';
import { scanMergeMessage } from './render_scan_summary.js';
import { markerCsv, markerRows } from './marker_list.js';
import { save } from '@tauri-apps/plugin-dialog';
import { notify } from './os_notifications.js';
import { finishClipsOwnsJobs } from './finish_clips.js';
import { escapeHtml as esc } from './html.js';
import { HIGHLIGHT_STATUS } from './status_colors.js';
import { createTakeDelete, renderingTakeFolders, progressLine } from './take_delete.js';
import { themedConfirm } from './themed_confirm.js';
import { createRenderSelection } from './render_selection.js';
import { createRenderJobsRestore } from './render_jobs_restore.js';

let jobs = []; // RenderJobView[] — latest snapshot from 'render_jobs_snapshot'
// The ticked rows (#70), set up by initRenderUI.
let selection = null;
// id:status pairs from the last snapshot Export Pool Free/Required
// (Estimated) were refreshed against — lets the snapshot listener tell "a
// job was added/removed/changed status" (worth a refresh) apart from "only
// progress% ticked" (not worth one), without a blind polling interval.
let lastJobsFingerprint = null;
// The rows this batch rendered or tried to (#70): every row seen Rendering
// or Queued while it runs. The finish toast counts only these, so a take
// that scanned in as Finished, or was cancelled before Start, isn't
// reported as rendered by this batch. Emptied when the batch finishes.
const batchJobIds = new Set();
// Delete Take (#644), set up by initRenderUI.
let takeDelete = null;
// initRenderUI's getExportDirs, for the Start button's check.
let exportDirsSource = null;

/** The batch panel's currently-selected codec, defaulting like the select's own first option. */
function getSelectedCodec() {
  return document.querySelector('#render-codec-select')?.value || 'prores';
}

/** The batch panel's current Custom-codec FFmpeg args text, if any. */
function getCustomCodecArgs() {
  return document.querySelector('#render-custom-codec-input')?.value?.trim() || '';
}

/** Shows the Custom-args input only while Custom is the selected codec. */
function syncCustomCodecVisibility() {
  const group = document.querySelector('#render-custom-codec-group');
  if (group) group.style.display = getSelectedCodec() === 'custom' ? '' : 'none';
}

function statusColor(status) {
  switch (status) {
    case JOB_STATUS.FINISHED: return '#4caf50';
    case JOB_STATUS.ERROR: return '#f44336';
    case JOB_STATUS.CANCELLED: return '#ffeb3b';
    case JOB_STATUS.RENDERING: return '#7ec8e3';
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
  updateBatchProgress();
  const el = document.querySelector('#render-footer-queue-summary');
  if (!el) return;
  el.textContent = queueSummaryText(jobs);
}

/** The status line, in the colour of the row status it reports (#818);
 *  `status` null puts back the default colour. */
function setRenderStatus(el, text, status = null) {
  if (!el) return;
  el.textContent = text;
  el.style.color = status ? statusColor(status) : '';
}

/** The footer's whole-batch bar (#40): hidden until there's a batch. */
function updateBatchProgress() {
  const wrap = document.querySelector('#render-batch-progress');
  if (!wrap) return;
  const pct = batchProgress(jobs);
  wrap.style.display = pct === null ? 'none' : 'flex';
  if (pct === null) return;
  const fill = wrap.querySelector('.progress-bar-fill');
  if (fill) fill.style.width = `${pct}%`;
  const label = wrap.querySelector('#render-batch-progress-label');
  if (label) label.textContent = STRINGS.RENDER.batchProgress(pct);
}

// ── Sortable columns (#40) ───────────────────────────────────────────────────

/** `{ column, dir }` from a header click, or null for job order. */
let jobSort = null;

function renderSortHeaders() {
  document.querySelectorAll('#render-jobs-table th[data-sort]').forEach((th) => {
    if (th.dataset.label === undefined) th.dataset.label = th.textContent.trim();
    const arrow = jobSort && jobSort.column === th.dataset.sort ? (jobSort.dir === 'asc' ? ' ▲' : ' ▼') : '';
    th.textContent = th.dataset.label + arrow;
  });
}

function initSortableHeaders() {
  document.querySelectorAll('#render-jobs-table th[data-sort]').forEach((th) => {
    th.title = STRINGS.RENDER.SORT_HEADER_TITLE;
    th.addEventListener('click', () => {
      jobSort = nextSort(jobSort, th.dataset.sort);
      renderSortHeaders();
      renderJobsTable();
    });
  });
  renderSortHeaders();
}

/**
 * The Actions cell's buttons, as HTML — kept separate from the row template
 * so it can be rebuilt on its own (see `updateJobRow`).
 */
function actionsCellHtml(j, rowState = {}) {
  let html = '';
  if (j.status === JOB_STATUS.RENDERING || j.status === JOB_STATUS.QUEUED) {
    html += `<button class="render-job-cancel-btn" data-job-id="${esc(j.id)}" title="${STRINGS.RENDER.CANCEL_JOB_TITLE}">✖</button>`;
  } else if (j.status === JOB_STATUS.CANCELLED || j.status === JOB_STATUS.FINISHED || j.status === JOB_STATUS.ERROR) {
    html += `<button class="render-job-reset-btn" data-job-id="${esc(j.id)}" title="${STRINGS.RENDER.RESET_JOB_TITLE}">🔄</button>`;
  }
  // Coexists with either button above — a Queued job can be removed directly
  // without cancelling it first, same as a Cancelled/Finished/Error one. Only
  // withheld while actually Rendering, where removing the row out from under
  // a live ffmpeg process has no defined outcome.
  if (j.status !== JOB_STATUS.RENDERING) {
    html += `<button class="render-job-remove-btn" data-job-id="${esc(j.id)}" title="${STRINGS.RENDER.REMOVE_JOB_TITLE}">🗑</button>`;
  }
  if (j.error_log) {
    html += `<button class="render-job-view-log-btn" data-job-id="${esc(j.id)}" title="${STRINGS.RENDER.VIEW_LOG_TITLE}">${STRINGS.RENDER.VIEW_LOG_BUTTON}</button>`;
  }
  if ((j.status === JOB_STATUS.FINISHED && j.output_path) || j.take_folder) {
    const useOutput = j.status === JOB_STATUS.FINISHED && j.output_path;
    html += `<button class="render-job-reveal-btn" data-job-id="${esc(j.id)}" title="${useOutput ? STRINGS.RENDER.OPEN_OUTPUT_FOLDER_TITLE : STRINGS.RENDER.OPEN_TAKE_FOLDER_TITLE}">${useOutput ? STRINGS.RENDER.OPEN_OUTPUT_BUTTON : STRINGS.RENDER.OPEN_TAKE_FOLDER_BUTTON}</button>`;
  }
  // #644: off while the take (either half of a HUD pair) is rendering, and
  // while another delete runs; the backend refuses both too. The tooltip
  // says which row is the busy one. A queued take stays deletable: the
  // confirm says its render leaves the queue.
  if (j.take_folder) {
    let title = STRINGS.RENDER.DELETE_TAKE_TITLE;
    if (rowState.deleting) title = STRINGS.RENDER.DELETE_TAKE_RUNNING_TITLE;
    else if (j.status === JOB_STATUS.RENDERING) title = STRINGS.RENDER.DELETE_TAKE_RENDERING_TITLE;
    else if (rowState.takeBusy) title = STRINGS.RENDER.DELETE_TAKE_PAIR_RENDERING_TITLE;
    const disabled = rowState.deleting || rowState.takeBusy ? ' disabled' : '';
    html += `<button class="render-job-delete-take-btn" data-job-id="${esc(j.id)}" title="${title}"${disabled}>${STRINGS.RENDER.DELETE_TAKE_BUTTON}</button>`;
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
      const target = (job.status === JOB_STATUS.FINISHED && job.output_path) ? job.output_path : job.take_folder;
      if (target) revealInExplorer(target).catch(() => {});
    });
  });
  cell.querySelectorAll('.render-job-delete-take-btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      const job = jobs.find((j) => j.id === btn.dataset.jobId);
      if (job?.take_folder) takeDelete?.deleteTakes([job.take_folder]);
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
  const showSkipToggle = j.skip_available && j.status === JOB_STATUS.QUEUED;
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
      // Only carries meaning for codecVal === 'custom'; harmless otherwise.
      const customArgsVal = cb.checked ? '' : (row?.dataset.lastCustomArgs ?? getCustomCodecArgs());
      cb.disabled = true;
      setRenderJobCodec(jobId, codecVal, customArgsVal)
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
function createJobRow(j, rowState) {
  const row = document.createElement('tr');
  row.dataset.jobId = j.id;
  row.innerHTML = `
    <td class="rj-select col-center"><input type="checkbox" class="render-job-select-cb" title="${STRINGS.RENDER.SELECT_ROW_TITLE}" /></td>
    <td class="rj-name"></td>
    <td class="rj-stream"></td>
    <td class="rj-frames"></td>
    <td class="rj-date"></td>
    <td class="rj-settings"></td>
    <td class="rj-status"></td>
    <td class="rj-speed"></td>
    <td><div class="progress-bar-container" style="margin-top:0;"><div class="progress-bar-fill rj-progress-fill" style="width:0%;"></div></div></td>
    <td class="rj-file-size"></td>
    <td class="rj-history"></td>
    <td class="rj-actions"></td>`;
  // The tick is looked up by the job the row shows, not kept on the row:
  // a sort moves rows and a snapshot patches them, and neither touches it.
  row.querySelector('.render-job-select-cb').addEventListener('change', (e) => {
    const job = jobs.find((x) => x.id === row.dataset.jobId);
    if (job) selection?.setTicked(job, e.target.checked);
  });
  updateJobRow(row, j, rowState);
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
function updateJobRow(row, j, rowState = {}) {
  // Snapshots arrive every scheduler tick while anything renders, and most
  // rows (queued, finished) haven't changed; skip those outright. The row
  // state rides along: a HUD pair's other half can change this row's Delete
  // Take (#644) without this row changing.
  const signature = JSON.stringify([j, rowState]);
  if (row.dataset.signature === signature) return;
  row.dataset.signature = signature;

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
  // lastCustomArgs rides along for the same reason: codec_id alone is just
  // "custom", not the args string that made it mean something.
  if (j.codec_id !== 'source_copy') {
    row.dataset.lastCodec = j.codec_id;
    row.dataset.lastCustomArgs = j.custom_codec_args || '';
  }

  const settingsKey = `${j.settings_summary}|${j.skip_available}|${j.status === JOB_STATUS.QUEUED}|${j.codec_id}`;
  if (settingsKey !== row.dataset.settingsKey) {
    const cell = row.querySelector('.rj-settings');
    cell.innerHTML = settingsCellHtml(j);
    wireSettingsCell(cell);
    row.dataset.settingsKey = settingsKey;
  }

  // #438: rebuilt only when the history changes; the open/closed state is
  // kept on the row, so a rebuild doesn't fold it shut.
  const historyKey = `${JSON.stringify(j.history || [])}|${j.current_attempt ?? ''}`;
  if (historyKey !== row.dataset.historyKey) {
    renderHistoryCell(row, j.history || [], j.current_attempt);
    row.dataset.historyKey = historyKey;
  }

  const actionsKey = `${j.status}|${!!j.error_log}|${j.output_path}|${j.take_folder}|${!!rowState.takeBusy}|${!!rowState.deleting}`;
  if (actionsKey !== row.dataset.actionsKey) {
    const cell = row.querySelector('.rj-actions');
    cell.innerHTML = actionsCellHtml(j, rowState);
    wireActionsCell(cell);
    row.dataset.actionsKey = actionsKey;
  }
}

/** The History cell (#438): the summary, and a ▸ that lists every attempt. */
function renderHistoryCell(row, history, currentAttempt) {
  const cell = row.querySelector('.rj-history');
  const { label, bytes } = historySummary(history, currentAttempt);
  const summary = document.createElement('span');
  summary.textContent = bytes > 0 ? `${label}, ${formatFileSize(bytes)}` : label;
  if (!history.length) {
    cell.replaceChildren(summary);
    return;
  }
  const toggle = document.createElement('button');
  toggle.type = 'button';
  toggle.className = 'rj-history-toggle';
  toggle.title = STRINGS.RENDER.HISTORY_TOGGLE_TITLE;
  toggle.style.cssText = 'background: transparent; border: none; color: inherit; cursor: pointer; padding: 0 4px 0 0;';
  const list = document.createElement('div');
  list.className = 'rj-history-list';
  list.style.cssText = 'font-size: 0.85em; opacity: 0.85; margin-top: 2px; white-space: nowrap;';
  historyLines(history, formatFileSize, currentAttempt).forEach((line) => {
    const div = document.createElement('div');
    div.textContent = line;
    list.appendChild(div);
  });
  const sync = () => {
    const open = row.dataset.historyOpen === '1';
    toggle.textContent = open ? '▾' : '▸';
    list.style.display = open ? '' : 'none';
  };
  toggle.addEventListener('click', () => {
    row.dataset.historyOpen = row.dataset.historyOpen === '1' ? '0' : '1';
    sync();
  });
  sync();
  cell.replaceChildren(toggle, summary, list);
}

function renderJobsTable() {
  updateFooterQueueSummary();
  // New rows get their default tick, known rows keep theirs (#70).
  selection?.sync(jobs);
  const tbody = document.querySelector('#render-jobs-tbody');
  if (!tbody) return;

  if (jobs.length === 0) {
    tbody.innerHTML = `<tr><td colspan="12" class="table-empty">${STRINGS.RENDER.TABLE_EMPTY}</td></tr>`;
    syncSelectionControls();
    return;
  }

  // The empty-state placeholder is not a job row and has to go before keyed
  // reconciliation below looks for `tr[data-job-id]` elements.
  if (tbody.querySelector('td.table-empty')) {
    tbody.innerHTML = '';
  }

  const existingRows = new Map();
  tbody.querySelectorAll('tr[data-job-id]').forEach((tr) => existingRows.set(tr.dataset.jobId, tr));
  const busyTakes = renderingTakeFolders(jobs);
  const deleting = !!takeDelete?.isRunning();

  // Rows are patched in place and only moved when the chosen sort (#40)
  // puts them somewhere else — a move keeps the row's own nodes, buttons
  // included, so a click in flight still lands (#80). With no sort, job
  // order is stable once a batch starts, so nothing ever moves.
  sortJobs(jobs, jobSort).forEach((j, i) => {
    const rowState = { takeBusy: busyTakes.has(j.take_folder), deleting };
    let row = existingRows.get(j.id);
    if (row) {
      existingRows.delete(j.id);
      updateJobRow(row, j, rowState);
    } else {
      row = createJobRow(j, rowState);
    }
    const cb = row.querySelector('.render-job-select-cb');
    if (cb) cb.checked = !!selection?.isTicked(j);
    if (tbody.children[i] !== row) tbody.insertBefore(row, tbody.children[i] || null);
  });
  // Anything left in the map is a row for a job id no longer in the
  // snapshot — should not normally happen within a batch, but a leftover
  // stale row would be worse than the cost of checking.
  existingRows.forEach((row) => row.remove());
  syncSelectionControls();
}

/**
 * The selection's own controls (#70): the header tick, the count, and the
 * footer buttons, which act on the ticked rows and say on how many. Start
 * waits while anything renders (the scheduler is already running) and
 * Delete Take folders while a delete runs, like the per-row button.
 */
/** Whether Render Settings has at least one export folder. */
function hasExportDir() {
  return (exportDirsSource?.() || []).some(Boolean);
}

/** Re-checks the Render tab's buttons, for a change made elsewhere (an export
 *  folder added or removed in Configuration). */
export function refreshRenderButtons() {
  syncSelectionControls();
}

function syncSelectionControls() {
  if (!selection) return;
  const { ticked, total } = selection.counts();
  const targets = selection.targets();
  const rendering = jobs.filter((j) => j.status === JOB_STATUS.RENDERING).length;

  const allCb = document.querySelector('#render-select-all-cb');
  if (allCb) {
    allCb.checked = total > 0 && ticked === total;
    allCb.indeterminate = ticked > 0 && ticked < total;
    allCb.disabled = total === 0;
  }
  const countEl = document.querySelector('#render-selection-count');
  if (countEl) countEl.textContent = STRINGS.RENDER.selectedCount(ticked, total);
  const unrenderedBtn = document.querySelector('#render-select-unrendered-btn');
  if (unrenderedBtn) unrenderedBtn.disabled = total === 0;
  for (const id of ['#render-select-finished-btn', '#render-finished-age-select']) {
    const el = document.querySelector(id);
    if (el) el.disabled = total === 0;
  }

  const setButton = (id, count, label, title, disabled = count === 0) => {
    const btn = document.querySelector(id);
    if (!btn) return;
    btn.textContent = label(count);
    btn.title = title;
    btn.disabled = disabled;
  };
  const R = STRINGS.RENDER;
  // Start renders every Queued row, ticked or not (#70).
  const queued = jobs.filter((j) => j.status === JOB_STATUS.QUEUED).length;
  // No export folder: the renderer refuses, so Start says where to add one.
  const noExportDir = !hasExportDir();
  setButton('#start-render-btn', queued, R.startRenderBatch,
    noExportDir ? R.START_RENDER_NO_EXPORT_DIR_TITLE
      : queued ? R.START_RENDER_TITLE : R.START_RENDER_NONE_TITLE,
    queued === 0 || rendering > 0 || noExportDir);
  setButton('#cancel-render-btn', targets.cancel.length, R.cancelSelected, R.CANCEL_SELECTED_TITLE);
  setButton('#retry-render-btn', targets.retry.length, R.retrySelected, R.RETRY_SELECTED_TITLE);
  setButton('#remove-render-btn', targets.remove.length, R.removeSelected, R.REMOVE_SELECTED_TITLE);
  setButton('#delete-takes-render-btn', targets.deleteFolders.length, R.deleteTakesSelected,
    R.DELETE_TAKES_SELECTED_TITLE, targets.deleteFolders.length === 0 || !!takeDelete?.isRunning());
  const clearBtn = document.querySelector('#clear-render-queue-btn');
  if (clearBtn) clearBtn.disabled = total === 0;
}

/** Delete Take's bar (#644): which step, and takes done of total. */
function showTakeDeleteProgress(progress) {
  const wrap = document.querySelector('#render-take-delete-progress');
  if (!wrap) return;
  wrap.style.display = progress ? 'flex' : 'none';
  if (!progress) return;
  const pct = progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0;
  const fill = wrap.querySelector('.progress-bar-fill');
  if (fill) fill.style.width = `${pct}%`;
  const label = wrap.querySelector('#render-take-delete-progress-label');
  if (label) label.textContent = progressLine(progress.step, progress.done, progress.total);
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

/** `showRenderTab` runs on the Recover click, so the status line it sets is
 *  on screen while the batch comes back. */
export async function checkRenderRecoveryOnStartup(showRenderTab) {
  const summary = await checkRenderAutosave();
  if (!summary) return;

  const modal = document.querySelector('#render-recovery-modal');
  if (!modal) return;
  document.querySelector('#render-recovery-source').textContent = summary.source_folder || STRINGS.RENDER.UNKNOWN_SOURCE_FOLDER;
  document.querySelector('#render-recovery-completed').textContent = String(summary.completed_count);
  document.querySelector('#render-recovery-pending').textContent = String(summary.pending_count);
  modal.style.display = 'flex';

  // Both buttons close the dialog on the click: the reply can take seconds
  // at start-up (#853), and the status line shows the recovery meanwhile.
  document.querySelector('#render-recovery-recover-btn').addEventListener('click', async () => {
    modal.style.display = 'none';
    const statusEl = document.querySelector('#render-status');
    setRenderStatus(statusEl, STRINGS.RENDER.STATUS_RECOVERING);
    if (showRenderTab) showRenderTab();
    // A scan started meanwhile owns the status line; leave its text alone.
    const stillRecovering = () => statusEl?.textContent === STRINGS.RENDER.STATUS_RECOVERING;
    try {
      jobs = await recoverRenderBatch();
      renderJobsTable();
      if (stillRecovering()) {
        const queued = jobs.filter((j) => j.status === JOB_STATUS.QUEUED).length;
        setRenderStatus(statusEl, STRINGS.RENDER.recoveredStatus(queued));
      }
      showToast(STRINGS.RENDER.recoveredJobsToast(summary.completed_count, summary.pending_count), 'info');
    } catch (err) {
      console.error('Error recovering render batch:', err);
      if (stillRecovering()) setRenderStatus(statusEl, STRINGS.RENDER.STATUS_WAITING);
      showToast(STRINGS.RENDER.recoverFailed(err), 'error');
    }
  }, { once: true });

  document.querySelector('#render-recovery-discard-btn').addEventListener('click', () => {
    modal.style.display = 'none';
    discardRenderAutosave().catch((err) => {
      console.error('Error discarding render autosave:', err);
    });
  }, { once: true });
}

/** The output files of this session's finished render jobs, in table order,
 *  for Combine Clips (#107). */
export function finishedRenderOutputs() {
  return jobs.filter((j) => j.status === JOB_STATUS.FINISHED && j.output_path).map((j) => j.output_path);
}

/** Export Marker List (#110): a CSV of every captured highlight. */
async function exportMarkerList(getTakeIndex, getAllDemos) {
  const demos = getAllDemos ? getAllDemos() : [];
  const takeIndex = getTakeIndex ? getTakeIndex() : {};
  const count = markerRows(demos, takeIndex).length;
  if (count === 0) {
    showToast(STRINGS.RENDER.EXPORT_MARKERS_NONE, 'info');
    return;
  }
  const path = await save({
    defaultPath: 'dod_markers.csv',
    filters: [{ name: 'CSV', extensions: ['csv'] }],
  }).catch((err) => {
    console.error('Save dialog failed:', err);
    return null;
  });
  if (!path) return;
  try {
    await writeTextFile(path, markerCsv(demos, takeIndex));
    showToast(STRINGS.RENDER.exportMarkersDone(count), 'success');
  } catch (err) {
    showToast(STRINGS.RENDER.exportMarkersFailed(err), 'error');
  }
}

export function initRenderUI(getCaptureLocations, getExportDirs, onSettingsChange, takeTracking) {
  exportDirsSource = getExportDirs;
  initSortableHeaders();
  const scanRenderBtn = document.querySelector('#scan-render-btn');
  const startRenderBtn = document.querySelector('#start-render-btn');
  const cancelRenderBtn = document.querySelector('#cancel-render-btn');
  const retryRenderBtn = document.querySelector('#retry-render-btn');
  const removeRenderBtn = document.querySelector('#remove-render-btn');
  const deleteTakesRenderBtn = document.querySelector('#delete-takes-render-btn');
  const clearQueueBtn = document.querySelector('#clear-render-queue-btn');
  const renderStatusEl = document.querySelector('#render-status');

  // #70: a tick change repaints the rows' boxes and the buttons; the ticks
  // themselves follow each snapshot in the listener below.
  selection = createRenderSelection({ getJobs: () => jobs, onChange: () => renderJobsTable() });
  document.querySelector('#render-select-all-cb')?.addEventListener('change', (e) => {
    if (e.target.checked) selection.selectAll(); else selection.selectNone();
  });
  document.querySelector('#render-select-unrendered-btn')?.addEventListener('click', () => selection.selectUnrendered());
  // #644: tick the finished takes, of any age or older than the age picked.
  document.querySelector('#render-select-finished-btn')?.addEventListener('click', () => {
    const minAgeMs = Number(document.querySelector('#render-finished-age-select')?.value) || 0;
    selection.selectFinished({ minAgeMs });
  });

  const getTakeIndex = takeTracking?.getTakeIndex || null;
  const getAllDemos = takeTracking?.getAllDemos || null;
  const getProjectTeams = takeTracking?.getProjectTeams || (() => null);
  document.querySelector('#export-marker-list-btn')
    ?.addEventListener('click', () => exportMarkerList(getTakeIndex, getAllDemos));
  const onTakeStatusChange = takeTracking?.onStatusChange || null;

  initErrorLogModal();
  takeDelete = createTakeDelete({
    measure: measureTakeFolders,
    remove: deleteTakeFolders,
    confirm: themedConfirm,
    showToast,
    formatSize: formatFileSize,
    showProgress: showTakeDeleteProgress,
    onBusyChange: () => renderJobsTable(),
    listen,
    getJobs: () => jobs,
  });

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
    // A hand-set Rendered confirmed by this render: no status change, but
    // the set-by-hand mark goes (#105).
    let markCleared = false;
    demos.forEach(demo => {
      (demo.streaks || []).forEach(streak => {
        if (!uids.has(streakUid(demo.path, streak))) return;
        // Idempotent: separate_hud produces two render jobs (all + hudcolor)
        // sharing one take_key, so this fires twice per take — the second
        // pass is just a no-op instead of a double-toast.
        if (streak.statusByHand) markCleared = true;
        if (setVerifiedStatus(streak, HIGHLIGHT_STATUS.RENDERED)) advanced += 1;
      });
    });

    if (advanced > 0) {
      showToast(STRINGS.RENDER.highlightsMarkedRendered(advanced), 'success');
    }
    if ((advanced > 0 || markCleared) && onTakeStatusChange) onTakeStatusChange();
  });

  // Real-time per-job state, pushed by the backend scheduler. This is the
  // single source of truth for whether a batch is actually running — Start
  // and Cancel follow it rather than only the Start click and
  // render_batch_finished.
  // A page reload keeps the backend's queue (#771): the list it holds is
  // asked for once, and painted unless a snapshot got here first.
  const jobsRestore = createRenderJobsRestore({ fetchJobs: getRenderJobs, applyJobs: applyJobsSnapshot });
  listen('render_jobs_snapshot', (event) => {
    jobsRestore.noteSnapshot();
    applyJobsSnapshot(event.payload || []);
  }).catch((err) => console.error('Failed to register render_jobs_snapshot listener:', err));
  jobsRestore.restore();

  // A scan merges into the queue (#772), so Scan stays enabled whatever the
  // rows are doing and is off only while a scan runs; the status line shows
  // the scan's progress until it ends.
  let scanRunning = false;

  function paintQueueStatus() {
    const queuedCount = jobs.filter((j) => j.status === JOB_STATUS.QUEUED).length;
    const renderingCount = jobs.filter((j) => j.status === JOB_STATUS.RENDERING).length;
    const activeOrQueued = queuedCount + renderingCount;
    if (renderingCount > 0) {
      setRenderStatus(renderStatusEl, STRINGS.RENDER.renderingStatus(jobs.length - activeOrQueued, jobs.length), JOB_STATUS.RENDERING);
    } else {
      setRenderStatus(renderStatusEl, queuedCount > 0 ? STRINGS.RENDER.scanCompleteStatus(queuedCount) : STRINGS.RENDER.STATUS_WAITING);
    }
  }

  function applyJobsSnapshot(list) {
    jobs = list;
    if (jobs.some((j) => j.status === JOB_STATUS.RENDERING)) {
      for (const j of jobs) {
        if (j.status === JOB_STATUS.RENDERING || j.status === JOB_STATUS.QUEUED) batchJobIds.add(j.id);
      }
    }
    renderJobsTable();
    if (!scanRunning) paintQueueStatus();
    // Start and the other footer buttons follow the ticked rows
    // (renderJobsTable above).
    if (scanRenderBtn) scanRenderBtn.disabled = scanRunning;

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
  }

  listen('render_batch_finished', () => {
    // A row that was Retried during the batch is still Queued, so Start
    // comes back for it; the buttons follow the ticks.
    if (scanRenderBtn) scanRenderBtn.disabled = scanRunning;
    syncSelectionControls();
    // Counted, not `some()`. The old check asked "was anything cancelled?"
    // before "did anything finish?", so one cancelled job in a batch of any
    // size reported the whole batch as cancelled — including the common case
    // of cancelling the takes you did not want and letting the rest run, where
    // it read as though nothing had rendered at all.
    const batch = jobs.filter((j) => batchJobIds.has(j.id));
    batchJobIds.clear();
    const finished = batch.filter((j) => j.status === JOB_STATUS.FINISHED).length;
    const failed = batch.filter((j) => j.status === JOB_STATUS.ERROR).length;
    const cancelled = batch.filter((j) => j.status === JOB_STATUS.CANCELLED).length;

    // A batch the finish step (#440) queued has already said how it ended, in
    // its own words ("12 clips ready") — a second toast would repeat it.
    if (batch.length > 0 && !finishClipsOwnsJobs(jobs)) {
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
    const end = batchEndStatus(finished, failed, cancelled);
    if (!scanRunning) setRenderStatus(renderStatusEl, end.text, end.status);
  });

  if (scanRenderBtn) {
    let scanFoundCount = 0;
    listen('render_scan_status', () => {
      // Payload is just "Found take: <name>" per take — the count is what
      // the status line needs, not the name, so this stays a cheap counter
      // rather than parsing/echoing every take's name into the DOM.
      scanFoundCount += 1;
      setRenderStatus(renderStatusEl, STRINGS.RENDER.scanFoundSoFar(scanFoundCount));
    });

    scanRenderBtn.addEventListener('click', () => {
      const captureLocations = getCaptureLocations ? getCaptureLocations() : [];
      if (!captureLocations || captureLocations.length === 0) {
        showToast(STRINGS.RENDER.ADD_RENDER_DIR_REQUIRED, 'error');
        return;
      }

      const codecVal = getSelectedCodec();
      const customCodecArgsVal = getCustomCodecArgs();
      if (codecVal === 'custom' && !customCodecArgsVal) {
        showToast(STRINGS.RENDER.CUSTOM_CODEC_ARGS_REQUIRED, 'error');
        return;
      }
      const fpsVal = parseInt(document.querySelector('#render-fps-input')?.value, 10) || 300;
      const maxConcurrentVal = Math.min(8, Math.max(1, parseInt(document.querySelector('#render-max-concurrent-input')?.value, 10) || 2));
      checkNvencConcurrencyWarning();
      const exportDirs = (getExportDirs ? getExportDirs() : []).filter(Boolean);
      // #441: a bad template is caught here, before anything is queued, not
      // halfway through a batch.
      const templateErrors = clipNameTemplateErrors();
      if (templateErrors.length) {
        showToast(STRINGS.CLIP_NAME.renderTemplateInvalid(templateErrors[0]), 'error');
        return;
      }
      const clipNames = clipNamesForTakes(
        getTakeIndex ? getTakeIndex() : null,
        getAllDemos ? getAllDemos() : [],
        getClipNameTemplate(),
        { maxLength: maxNameLength(exportDirs), projectTeams: getProjectTeams() },
      );
      // FFmpeg override is shared with the capture config panel.
      const ffmpegPathVal = document.querySelector('#ffmpeg-override-path-input')?.value?.trim() || null;

      scanFoundCount = 0;
      showToast(STRINGS.RENDER.SCANNING_TOAST, 'info');
      scanRunning = true;
      scanRenderBtn.disabled = true;
      setRenderStatus(renderStatusEl, STRINGS.RENDER.STATUS_SCANNING);

      const renderPayload = {
        render_directories: captureLocations,
        codec: codecVal,
        custom_codec_args: customCodecArgsVal,
        fps: fpsVal,
        ffmpeg_path: ffmpegPathVal || null,
        export_directories: exportDirs,
        max_concurrent_renders: maxConcurrentVal,
        clip_names: clipNames,
      };

      // Merges into the real job table (below) via the render_jobs_snapshot
      // event this emits (#772): new takes join as Queued rows, rows already
      // there keep their state and tick. Start Render Batch is what kicks
      // off the scheduler when none runs, so there's a window to review the
      // batch or toggle Skip on an OBS-shaped job before anything renders.
      queueRenderBatch(renderPayload)
        .then((summary) => {
          scanRunning = false;
          scanRenderBtn.disabled = false;
          const message = scanMergeMessage(summary);
          showToast(message.toast, message.level);
          // While a batch renders, its own progress line takes over again.
          if (jobs.some((j) => j.status === JOB_STATUS.RENDERING)) paintQueueStatus();
          else setRenderStatus(renderStatusEl, message.status);
        })
        .catch((err) => {
          scanRunning = false;
          scanRenderBtn.disabled = false;
          showToast(STRINGS.RENDER.scanDirError(err), 'error');
          setRenderStatus(renderStatusEl, STRINGS.RENDER.STATUS_SCAN_FAILED, JOB_STATUS.ERROR);
        });
    });
  }

  // The footer buttons act on the ticked rows (#70). Each one is off until
  // the next snapshot repaints it, so a double click can't send twice; on a
  // refusal the buttons are repainted from the rows as they are.
  if (startRenderBtn) {
    startRenderBtn.addEventListener('click', () => {
      if (!jobs.some((j) => j.status === JOB_STATUS.QUEUED)) return;
      if (!hasExportDir()) {
        showToast(STRINGS.RENDER.START_RENDER_NO_EXPORT_DIR_TITLE, 'error');
        syncSelectionControls();
        return;
      }
      // A batch of Skip copies can end before any snapshot shows Rendering.
      for (const j of jobs) if (j.status === JOB_STATUS.QUEUED) batchJobIds.add(j.id);
      showToast(STRINGS.RENDER.INITIALIZING_RENDER_BATCH, 'info');
      startRenderBtn.disabled = true;
      if (cancelRenderBtn) cancelRenderBtn.disabled = false;

      startQueuedRender()
        .then(() => {
          showToast(STRINGS.RENDER.RENDER_BATCH_QUEUED, 'success');
        })
        .catch((err) => {
          showToast(STRINGS.RENDER.renderBatchError(err), 'error');
          syncSelectionControls();
        });
    });
  }

  if (cancelRenderBtn) {
    cancelRenderBtn.addEventListener('click', () => {
      const ids = selection.targets().cancel;
      if (!ids.length) return;
      showToast(STRINGS.RENDER.CANCELLING_RENDER_BATCH, 'info');
      cancelRenderBtn.disabled = true;
      cancelRenderJobs(ids).catch((err) => {
        console.error('IPC Execution Error (cancelRenderJobs):', err);
        syncSelectionControls();
      });
    });
  }

  if (retryRenderBtn) {
    retryRenderBtn.addEventListener('click', () => {
      const ids = selection.targets().retry;
      if (!ids.length) return;
      retryRenderBtn.disabled = true;
      resetRenderJobs(ids).catch((err) => {
        console.error('IPC Execution Error (resetRenderJobs):', err);
        syncSelectionControls();
      });
    });
  }

  if (removeRenderBtn) {
    removeRenderBtn.addEventListener('click', () => {
      const ids = selection.targets().remove;
      if (!ids.length) return;
      removeRenderBtn.disabled = true;
      removeRenderJobs(ids).catch((err) => {
        console.error('IPC Execution Error (removeRenderJobs):', err);
        syncSelectionControls();
      });
    });
  }

  if (deleteTakesRenderBtn) {
    deleteTakesRenderBtn.addEventListener('click', () => {
      const folders = selection.targets().deleteFolders;
      if (folders.length) takeDelete?.deleteTakes(folders);
    });
  }

  // Clear Queue: the whole list goes. With renders running, ask first; the
  // backend stops them and empties the list once they have stopped.
  if (clearQueueBtn) {
    clearQueueBtn.addEventListener('click', async () => {
      const rendering = jobs.filter((j) => j.status === JOB_STATUS.RENDERING).length;
      if (rendering > 0) {
        const ok = await themedConfirm(STRINGS.RENDER.clearQueueConfirmMessage(rendering), {
          title: STRINGS.RENDER.CLEAR_QUEUE_CONFIRM_TITLE,
          confirmLabel: STRINGS.RENDER.CLEAR_QUEUE_CONFIRM_BUTTON,
        });
        if (!ok) return;
      }
      clearQueueBtn.disabled = true;
      clearRenderQueue().catch((err) => {
        showToast(STRINGS.RENDER.renderBatchError(err), 'error');
        syncSelectionControls();
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
  // Reflects whatever the select already shows at setup time too — it may
  // have just been restored from settings.render_codec to "custom" before
  // this runs, and the args input needs to be visible from the first paint.
  if (codecEl) codecEl.addEventListener('change', syncCustomCodecVisibility);
  syncCustomCodecVisibility();

  // Paints once at startup (before any snapshot has arrived, so before
  // anything could have changed) — the `render_jobs_snapshot` listener above
  // takes over from here, refreshing on every add/remove/status change
  // instead of blindly polling.
  refreshExportPoolFree(getExportDirs);
  syncSelectionControls();
}
