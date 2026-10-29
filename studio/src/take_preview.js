// take_preview.js
// Take pictures (#1069): a thumbnail on each Render tab row, the same frame
// bigger on hover, and a contact sheet of nine frames across the take on
// click, so a black, frozen or wrong take shows before a long render.
//
// The backend makes the frames once and keeps them beside the take
// (`native::hlcr::take_frames`). Here, each take on the list is made in
// turn, one at a time, in the background; the line under the table says how
// far it is. A take with a HUD pair is two rows with one picture each
// (`stream_folder`). Delete Take pauses the pass, so FFmpeg isn't reading a
// take that is moving to the Recycle Bin.
import { STRINGS } from './strings.js';

export const PREVIEW_STATE = { WAITING: 'waiting', MAKING: 'making', DONE: 'done', FAILED: 'failed' };

/** The error a cancelled pass returns (`messages::TAKE_FRAMES_CANCELLED`). */
const CANCELLED = 'cancelled';

/** One picture per take stream; '' for a row that has none to show. */
export function previewKey(job) {
  return job?.take_folder && job?.stream_folder ? `${job.take_folder}|${job.stream_folder}` : '';
}

/** A contact-sheet frame's caption: its time and frame number, as known. */
export function frameLabel({ secs, frame } = {}) {
  const parts = [];
  if (Number.isFinite(secs)) parts.push(STRINGS.RENDER.takeSheetSeconds(secs.toFixed(1)));
  if (Number.isFinite(frame)) parts.push(STRINGS.RENDER.takeSheetFrame(frame));
  return parts.join(' · ');
}

/** The pass's progress, `null` while nothing is waiting or being made:
 *  pictures done of all, and the bar's percent, counting the frames of the
 *  take being made now. */
export function passProgress(states, current = null) {
  const all = [...states.values()];
  const busy = all.some((s) => s.status === PREVIEW_STATE.WAITING || s.status === PREVIEW_STATE.MAKING);
  if (!busy) return null;
  const done = all.filter((s) => s.status === PREVIEW_STATE.DONE || s.status === PREVIEW_STATE.FAILED).length;
  const part = current && current.total > 0 ? current.done / current.total : 0;
  return { done, total: all.length, pct: Math.round(((done + part) / all.length) * 100) };
}

/**
 * @param {object} deps
 * @param {(takeFolder: string, stream: string, fps: number|null) => Promise<{uri: string}>} deps.thumb
 * @param {(takeFolder: string, stream: string, fps: number|null) => Promise<object>} deps.sheet
 * @param {() => Promise<void>} deps.cancel  stops the backend pass running now
 * @param {(event: string, cb: Function) => void} [deps.listen]
 * @param {() => number|null} deps.getFps  a frame sequence's rate, for its times
 * @param {() => void} deps.onChange  a picture or the progress changed
 */
export function createTakePreview({ thumb, sheet, cancel, listen, getFps, onChange }) {
  const states = new Map();
  let running = false;
  let paused = false;
  let current = null;

  listen?.('take_preview_progress', (event) => {
    const { take_folder: take, done, total } = event?.payload || {};
    // A contact sheet being made reports too; only the pass's own take counts.
    if (!running || !current || take !== current.take) return;
    current = { ...current, done, total };
    onChange();
  });

  /** Lists every row's take, drops the ones no longer listed, and starts
   *  the pass when there is something to make. */
  function sync(jobs) {
    const keys = new Map();
    for (const j of jobs || []) {
      const key = previewKey(j);
      if (key && !keys.has(key)) keys.set(key, j);
    }
    for (const key of [...states.keys()]) if (!keys.has(key)) states.delete(key);
    for (const [key, j] of keys) {
      if (!states.has(key)) states.set(key, { status: PREVIEW_STATE.WAITING, take: j.take_folder, stream: j.stream_folder });
    }
    run();
  }

  async function run() {
    if (running || paused) return;
    running = true;
    try {
      for (;;) {
        if (paused) break;
        const entry = [...states.entries()].find(([, s]) => s.status === PREVIEW_STATE.WAITING);
        if (!entry) break;
        const [key, state] = entry;
        state.status = PREVIEW_STATE.MAKING;
        current = { key, take: state.take, done: 0, total: 0 };
        onChange();
        try {
          const result = await thumb(state.take, state.stream, getFps());
          state.status = PREVIEW_STATE.DONE;
          state.uri = result?.uri || '';
          state.error = '';
        } catch (err) {
          if (String(err) === CANCELLED) {
            // Paused for a delete: made again on resume, if still listed.
            state.status = PREVIEW_STATE.WAITING;
            break;
          }
          state.status = PREVIEW_STATE.FAILED;
          state.error = String(err);
        }
      }
    } finally {
      running = false;
      current = null;
      onChange();
    }
  }

  /** Stops the pass (Delete Take is about to move takes). */
  function pause() {
    paused = true;
    if (running) cancel().catch(() => {});
  }

  function resume() {
    paused = false;
    run();
  }

  /** Fills a row's picture cell for `job`, touching the DOM only when what
   *  it shows changed. */
  function fillCell(cell, job) {
    if (!cell) return;
    const key = previewKey(job);
    const state = key ? states.get(key) : null;
    const status = state?.status || '';
    const shown = `${key}|${status}`;
    if (cell.dataset.shown === shown) return;
    cell.dataset.shown = shown;
    cell.replaceChildren();
    cell.title = '';
    if (!key) return;
    if (status === PREVIEW_STATE.DONE && state.uri) {
      const img = document.createElement('img');
      img.className = 'rj-thumb';
      img.src = state.uri;
      img.alt = '';
      img.dataset.previewKey = key;
      img.title = STRINGS.RENDER.TAKE_THUMB_TITLE;
      cell.appendChild(img);
      return;
    }
    const box = document.createElement('span');
    box.className = 'rj-thumb rj-thumb-empty';
    if (status === PREVIEW_STATE.FAILED) {
      box.textContent = '—';
      cell.title = STRINGS.RENDER.takeThumbFailed(state.error);
    } else {
      box.textContent = '…';
      cell.title = status === PREVIEW_STATE.MAKING ? STRINGS.RENDER.TAKE_THUMB_MAKING : STRINGS.RENDER.TAKE_THUMB_WAITING;
    }
    cell.appendChild(box);
  }

  /** The thumbnail made for `key`, or ''. */
  function thumbFor(key) {
    return states.get(key)?.uri || '';
  }

  /** Every frame of a take's sheet; made first if the pass hasn't reached
   *  it yet. */
  function sheetFor(job) {
    return sheet(job.take_folder, job.stream_folder, getFps());
  }

  return {
    sync,
    pause,
    resume,
    fillCell,
    thumbFor,
    stateFor: (key) => states.get(key) || null,
    sheetFor,
    progress: () => passProgress(states, current),
    isRunning: () => running,
  };
}

/** The hover preview and the contact sheet window, on the Render table.
 *  `getJob(key)` is the row a thumbnail belongs to. */
export function wireTakePreviewViews({ table, preview, getJob, hover, modal }) {
  if (!table) return;
  const hoverImg = hover?.querySelector('img');

  table.addEventListener('mouseover', (e) => {
    const img = e.target.closest?.('img.rj-thumb');
    if (!img || !hover || !hoverImg) return;
    hoverImg.src = img.src;
    hover.hidden = false;
    place(e);
  });
  table.addEventListener('mousemove', (e) => {
    if (hover && !hover.hidden) place(e);
  });
  table.addEventListener('mouseout', (e) => {
    if (e.target.closest?.('img.rj-thumb') && hover) hover.hidden = true;
  });

  // Beside the cursor, flipped to the other side near the window's edge.
  function place(e) {
    const w = hover.offsetWidth || 480;
    const h = hover.offsetHeight || 270;
    const x = e.clientX + 16 + w > window.innerWidth ? e.clientX - 16 - w : e.clientX + 16;
    const y = Math.min(Math.max(8, e.clientY - h / 2), window.innerHeight - h - 8);
    hover.style.left = `${Math.max(8, x)}px`;
    hover.style.top = `${Math.max(8, y)}px`;
  }

  table.addEventListener('click', async (e) => {
    const img = e.target.closest?.('img.rj-thumb');
    if (!img || !modal) return;
    const job = getJob(img.dataset.previewKey);
    if (!job) return;
    if (hover) hover.hidden = true;
    const title = modal.querySelector('.take-sheet-title');
    const grid = modal.querySelector('.take-sheet-grid');
    const status = modal.querySelector('.take-sheet-status');
    if (title) title.textContent = STRINGS.RENDER.takeSheetTitle(job.name);
    grid?.replaceChildren();
    if (status) status.textContent = STRINGS.RENDER.TAKE_SHEET_LOADING;
    modal.style.display = 'flex';
    modal.dataset.key = img.dataset.previewKey;
    try {
      const result = await preview.sheetFor(job);
      // Closed, or another take opened, while it loaded.
      if (modal.style.display === 'none' || modal.dataset.key !== img.dataset.previewKey) return;
      for (const f of result?.frames || []) {
        const fig = document.createElement('figure');
        const pic = document.createElement('img');
        pic.src = f.uri;
        pic.alt = '';
        const cap = document.createElement('figcaption');
        cap.textContent = frameLabel(f);
        fig.append(pic, cap);
        grid?.appendChild(fig);
      }
      if (status) status.textContent = '';
    } catch (err) {
      if (status) status.textContent = STRINGS.RENDER.takeSheetFailed(err);
    }
  });

  modal?.querySelector('.take-sheet-close')?.addEventListener('click', () => {
    modal.style.display = 'none';
  });
}

/**
 * The Render tab's take pictures, wired to its table: each row's picture
 * cell (`.rj-thumb-cell`), the progress line, the hover and the contact
 * sheet. `render_pane.js` hands it the rows (`sync`) and pauses it around a
 * Delete Take.
 */
export function startTakePreview({ thumb, sheet, cancel, listen, getJobs, getFps }) {
  const tbody = document.querySelector('#render-jobs-tbody');
  const progressWrap = document.querySelector('#render-take-preview-progress');
  const jobFor = (key) => (getJobs() || []).find((j) => previewKey(j) === key);

  const preview = createTakePreview({
    thumb,
    sheet,
    cancel,
    listen,
    getFps,
    onChange: () => {
      tbody?.querySelectorAll('tr[data-job-id]').forEach((row) => {
        const job = (getJobs() || []).find((j) => j.id === row.dataset.jobId);
        if (job) preview.fillCell(row.querySelector('.rj-thumb-cell'), job);
      });
      const p = preview.progress();
      if (!progressWrap) return;
      progressWrap.style.display = p ? 'flex' : 'none';
      if (!p) return;
      const fill = progressWrap.querySelector('.progress-bar-fill');
      if (fill) fill.style.width = `${p.pct}%`;
      const label = progressWrap.querySelector('#render-take-preview-progress-label');
      if (label) label.textContent = STRINGS.RENDER.takePicturesProgress(p.done, p.total);
    },
  });

  wireTakePreviewViews({
    table: document.querySelector('#render-jobs-table'),
    preview,
    getJob: jobFor,
    hover: document.querySelector('#take-preview-hover'),
    modal: document.querySelector('#take-sheet-modal'),
  });
  return preview;
}
