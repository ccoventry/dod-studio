// take_checks.js
// Take checks (#1084): whether each take on the Render tab is good, shown
// as a chip under its picture: OK, or a warning count, with each warning in
// the tooltip and on the contact sheet, its time in the clip, and whether
// capturing again helps or the demo itself is the reason.
//
// The backend runs one FFmpeg pass per take stream and keeps the result
// beside its pictures (`native::hlcr::take_checks`). Here, each take on the
// list is checked in turn, one at a time, after the picture pass has made
// every picture (the pictures are quick, the checks read the whole take);
// the line under the table says how far it is. Delete Take pauses it, as it
// does the pictures; a render or a capture batch stops it until it ends,
// since the pass reads a whole take and decodes it on several cores.
import { STRINGS } from './strings.js';
import { previewKey } from './take_preview.js';

export const CHECK_STATE = { WAITING: 'waiting', CHECKING: 'checking', DONE: 'done', FAILED: 'failed' };

/** The error a cancelled check returns (`messages::TAKE_FRAMES_CANCELLED`). */
const CANCELLED = 'cancelled';

const secsText = (v) => (Number.isFinite(v) ? v.toFixed(1) : '?');

/** One finding as a line of text, and whether it's a warning. */
export function findingLine(f) {
  const R = STRINGS.RENDER;
  const secs = secsText(f?.secs);
  const at = secsText(f?.at_secs);
  let text;
  switch (f?.check) {
    case 'black': text = R.takeCheckBlack(secs, at); break;
    case 'frozen': text = R.takeCheckFrozen(secs, at); break;
    case 'holds': text = R.takeCheckHolds(f.count ?? 0, secs, at); break;
    case 'no_sound': text = R.TAKE_CHECK_NO_SOUND; break;
    case 'silent': text = R.TAKE_CHECK_SILENT; break;
    case 'clipping': text = R.takeCheckClipping(((f.share ?? 0) * 100).toFixed(1)); break;
    default: return null;
  }
  const cause = f.cause === 'demo' ? R.TAKE_CHECK_CAUSE_DEMO : R.TAKE_CHECK_CAUSE_CAPTURE;
  return { text, cause, warn: !!f.warn };
}

/** Every finding as a line, warnings first. */
export function findingLines(result) {
  const lines = (result?.findings || []).map(findingLine).filter(Boolean);
  return [...lines.filter((l) => l.warn), ...lines.filter((l) => !l.warn)];
}

/** What a take's chip says: `null` when there is nothing to show. */
export function checkChip(state) {
  const R = STRINGS.RENDER;
  if (!state) return null;
  if (state.status === CHECK_STATE.WAITING) return { label: '', cls: '', title: R.TAKE_CHECK_WAITING_TITLE };
  if (state.status === CHECK_STATE.CHECKING) return { label: R.TAKE_CHECK_CHECKING, cls: 'rj-check-busy', title: R.TAKE_CHECK_CHECKING_TITLE };
  if (state.status === CHECK_STATE.FAILED) return { label: R.TAKE_CHECK_FAILED, cls: 'rj-check-failed', title: R.takeCheckFailed(state.error) };
  // A take the backend gave nothing for (none to check) shows nothing.
  if (!state.result) return null;
  const lines = findingLines(state.result);
  const warnings = lines.filter((l) => l.warn).length;
  const title = lines.length
    ? lines.map((l) => `${l.warn ? '⚠ ' : ''}${l.text} (${l.cause})`).join('\n')
    : R.TAKE_CHECK_ALL_GOOD;
  return warnings
    ? { label: R.takeCheckWarnings(warnings), cls: 'rj-check-warn', title }
    : { label: R.TAKE_CHECK_OK, cls: 'rj-check-ok', title };
}

/** The pass's progress, `null` while nothing is waiting or being checked:
 *  takes done of all, and the bar's percent, counting the part of the take
 *  being checked now. */
export function checksProgress(states, current = null) {
  const all = [...states.values()];
  const busy = all.some((s) => s.status === CHECK_STATE.WAITING || s.status === CHECK_STATE.CHECKING);
  if (!busy) return null;
  const done = all.filter((s) => s.status === CHECK_STATE.DONE || s.status === CHECK_STATE.FAILED).length;
  const part = current && current.total > 0 ? Math.min(1, current.done / current.total) : 0;
  return { done, total: all.length, pct: Math.round(((done + part) / all.length) * 100) };
}

/**
 * @param {object} deps
 * @param {(takeFolder: string, stream: string, fps: number|null) => Promise<object>} deps.check
 * @param {() => Promise<void>} deps.cancel  stops the backend check running now
 * @param {(event: string, cb: Function) => void} [deps.listen]
 * @param {() => number|null} deps.getFps  a frame sequence's rate, when the take doesn't record one
 * @param {() => boolean} [deps.isBlocked]  true while the picture pass or a render runs: no check starts
 * @param {() => boolean} [deps.mustStop]  true while a render runs: the check running now stops too
 * @param {() => void} deps.onChange  a result or the progress changed
 */
export function createTakeChecks({ check, cancel, listen, getFps, isBlocked = () => false, mustStop = () => false, onChange }) {
  const states = new Map();
  let running = false;
  let paused = false;
  // A capture batch is running (`capture_status`): the pass reads a whole
  // take at full speed and decodes it on several cores, which would take disk
  // and CPU from the game writing its takes, or from OBS (dropped frames).
  let capturing = false;
  let current = null;

  const held = () => paused || capturing || mustStop();
  /** Stops the check running now when it must give way. Called again on
   *  every progress event, so a cancel that reached the backend just before
   *  the check claimed its flag still stops it. */
  function yieldIfHeld() {
    if (running && current && held()) cancel().catch(() => {});
  }

  listen?.('take_checks_progress', (event) => {
    const { take_folder: take, done, total } = event?.payload || {};
    if (!running || !current || take !== current.take) return;
    current = { ...current, done, total };
    yieldIfHeld();
    onChange();
  });

  listen?.('capture_status', (event) => {
    const now = !!event?.payload?.running;
    if (now === capturing) return;
    capturing = now;
    if (capturing) {
      yieldIfHeld();
      onChange();
    } else {
      run();
    }
  });

  /** Lists every row's take stream, drops the ones no longer listed, and
   *  starts the pass when there is something to check. */
  function sync(jobs) {
    const keys = new Map();
    for (const j of jobs || []) {
      const key = previewKey(j);
      if (key && !keys.has(key)) keys.set(key, j);
    }
    for (const key of [...states.keys()]) if (!keys.has(key)) states.delete(key);
    for (const [key, j] of keys) {
      if (!states.has(key)) states.set(key, { status: CHECK_STATE.WAITING, take: j.take_folder, stream: j.stream_folder });
    }
    // A take no longer listed (another project, removed) isn't read on.
    if (running && current && !states.has(current.key)) cancel().catch(() => {});
    yieldIfHeld();
    run();
  }

  async function run() {
    if (running || held() || isBlocked()) return;
    running = true;
    try {
      for (;;) {
        if (held() || isBlocked()) break;
        const entry = [...states.entries()].find(([, s]) => s.status === CHECK_STATE.WAITING);
        if (!entry) break;
        const [key, state] = entry;
        state.status = CHECK_STATE.CHECKING;
        current = { key, take: state.take, done: 0, total: 0 };
        onChange();
        try {
          state.result = (await check(state.take, state.stream, getFps())) || null;
          state.status = CHECK_STATE.DONE;
          state.error = '';
        } catch (err) {
          if (String(err) === CANCELLED) {
            // Checked again later: the loop's top stops while held, and goes
            // on to the next take when this one was dropped from the list.
            state.status = CHECK_STATE.WAITING;
            continue;
          }
          state.status = CHECK_STATE.FAILED;
          state.error = String(err);
        }
      }
    } finally {
      running = false;
      current = null;
      onChange();
    }
  }

  function pause() {
    paused = true;
    if (running) cancel().catch(() => {});
  }

  function resume() {
    paused = false;
    run();
  }

  /** Fills a row's chip for `job`, touching the DOM only when it changed. */
  function fillChip(el, job) {
    if (!el) return;
    const key = previewKey(job);
    const state = key ? states.get(key) : null;
    const chip = checkChip(state);
    const shown = chip ? `${key}|${chip.label}|${chip.cls}|${chip.title}` : '';
    if (el.dataset.shown === shown) return;
    el.dataset.shown = shown;
    el.textContent = chip?.label || '';
    el.className = `rj-check ${chip?.cls || ''}`.trim();
    el.title = chip?.title || '';
  }

  return {
    sync,
    pause,
    resume,
    run,
    fillChip,
    stateFor: (key) => states.get(key) || null,
    progress: () => checksProgress(states, current),
    isRunning: () => running,
    /** Waiting for a capture, a render or a Delete Take to end. */
    isHeld: held,
  };
}

/** The checks on the contact sheet: the list under its frames, filled when a
 *  take's picture is clicked, from what was already checked. */
export function wireSheetChecks({ table, checks, modal }) {
  const list = modal?.querySelector('.take-sheet-checks');
  if (!table || !list) return;
  table.addEventListener('click', (e) => {
    const img = e.target.closest?.('img.rj-thumb');
    if (!img) return;
    const state = checks.stateFor(img.dataset.previewKey);
    const chip = checkChip(state);
    list.replaceChildren();
    if (!chip || state.status !== CHECK_STATE.DONE) {
      list.hidden = true;
      return;
    }
    const head = document.createElement('h3');
    head.textContent = STRINGS.RENDER.TAKE_SHEET_CHECKS_TITLE;
    const ul = document.createElement('ul');
    const lines = findingLines(state.result);
    if (!lines.length) {
      const li = document.createElement('li');
      li.textContent = STRINGS.RENDER.TAKE_CHECK_ALL_GOOD;
      ul.appendChild(li);
    }
    for (const l of lines) {
      const li = document.createElement('li');
      li.className = l.warn ? 'take-check-warn' : 'take-check-note';
      li.textContent = `${l.text} `;
      const cause = document.createElement('span');
      cause.className = 'take-check-cause';
      cause.textContent = `(${l.cause})`;
      li.appendChild(cause);
      ul.appendChild(li);
    }
    list.append(head, ul);
    list.hidden = false;
  });
}

/**
 * The Render tab's take checks, wired to its table: each row's chip
 * (`.rj-check`), the progress line and the contact sheet's list.
 * `render_pane.js` hands it the rows (`sync`), pauses it around a Delete
 * Take, and calls `run` when the picture pass finishes.
 */
export function startTakeChecks({ check, cancel, listen, getJobs, getFps, isBlocked, mustStop }) {
  const tbody = document.querySelector('#render-jobs-tbody');
  const progressWrap = document.querySelector('#render-take-checks-progress');
  const checks = createTakeChecks({
    check,
    cancel,
    listen,
    getFps,
    isBlocked,
    mustStop,
    onChange: () => {
      tbody?.querySelectorAll('tr[data-job-id]').forEach((row) => {
        const job = (getJobs() || []).find((j) => j.id === row.dataset.jobId);
        if (job) checks.fillChip(row.querySelector('.rj-check'), job);
      });
      // Waiting behind the pictures, a render or a capture is not checking yet.
      const p = isBlocked?.() || checks.isHeld() ? null : checks.progress();
      if (!progressWrap) return;
      progressWrap.style.display = p ? 'flex' : 'none';
      if (!p) return;
      const fill = progressWrap.querySelector('.progress-bar-fill');
      if (fill) fill.style.width = `${p.pct}%`;
      const label = progressWrap.querySelector('#render-take-checks-progress-label');
      if (label) label.textContent = STRINGS.RENDER.takeChecksProgress(p.done, p.total);
    },
  });
  wireSheetChecks({
    table: document.querySelector('#render-jobs-table'),
    checks,
    modal: document.querySelector('#take-sheet-modal'),
  });
  return checks;
}
