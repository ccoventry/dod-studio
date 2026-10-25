// render_selection.js
// Which Render tab rows are ticked (#70), and what each bulk button does with
// them. Start Render Batch ignores the ticks: every Queued row renders, and a
// row to leave out is cancelled or removed. The ticks scope Cancel, Retry,
// Remove and Delete. The tick state lives here, keyed by the job rather than by its id:
// a recovered batch numbers its jobs from 0 again, so an id alone would hand a new
// take the tick of whatever sat in that slot before. Pure apart from the
// factory's own map, so it can be tested on its own.

import { JOB_STATUS } from './render_jobs.js';
import { renderingTakeFolders } from './take_delete.js';

/** The key a job's tick is kept under: stable across snapshots of one
 *  queue, different for the job a later scan puts at the same id. */
export function selectionKey(job) {
  return `${job.id}|${job.take_folder || ''}|${job.stream || ''}`;
}

/** A take with a successful render in its history (#438), this session or
 *  an earlier one. */
export function wasRendered(job) {
  return (job.history || []).some((h) => h.outcome === 'finished');
}

/**
 * When a take's clip was finished, from its rows in the list, or `null` when
 * it isn't (#644). The same rule as #716's automatic cleanup
 * (`take_cleanup::finished_at`): every stream of the take (both halves of a
 * HUD pair) has a last render attempt, by start time, that finished, and that
 * clip file is still on disk. A row waiting or rendering (a Retry queues a
 * finished take again) keeps the take unfinished. The time is the latest of
 * those attempts' start times. #716 reads the streams from the take folder;
 * here they are the take's rows, since this only picks rows to tick and
 * Delete Take still asks before anything goes.
 */
export function takeFinishedAt(takeJobs) {
  if (!takeJobs?.length) return null;
  let latest = 0;
  for (const j of takeJobs) {
    if (j.status === JOB_STATUS.QUEUED || j.status === JOB_STATUS.RENDERING) return null;
    let last = null;
    for (const h of j.history || []) {
      if (!last || (h.started_unix_ms || 0) > (last.started_unix_ms || 0)) last = h;
    }
    if (!last || last.outcome !== 'finished' || !last.output_exists) return null;
    latest = Math.max(latest, last.started_unix_ms || 0);
  }
  return latest;
}

/**
 * The take folders whose clips are finished (`takeFinishedAt`), and, with
 * `minAgeMs`, finished at least that long before `nowMs`.
 */
export function finishedTakeFolders(allJobs, { minAgeMs = 0, nowMs = Date.now() } = {}) {
  const byTake = new Map();
  for (const j of allJobs || []) {
    if (!j.take_folder) continue;
    if (!byTake.has(j.take_folder)) byTake.set(j.take_folder, []);
    byTake.get(j.take_folder).push(j);
  }
  const out = new Set();
  for (const [folder, takeJobs] of byTake) {
    const at = takeFinishedAt(takeJobs);
    // No age asked: a start time ahead of this clock (another PC's, or
    // before the clock was set back) still counts.
    if (at !== null && (minAgeMs <= 0 || nowMs - at >= minAgeMs)) out.add(folder);
  }
  return out;
}

/**
 * The ids and take folders each bulk button acts on, from the ticked jobs
 * and the whole list (`busy` takes come from the whole list: a take with a
 * HUD pair is two rows, and either one rendering keeps the take, like the
 * per-row Delete Take; a queued take goes, and its render leaves the queue).
 */
export function bulkTargets(tickedJobs, allJobs) {
  const busy = renderingTakeFolders(allJobs);
  const cancel = [];
  const retry = [];
  const remove = [];
  const deleteFolders = new Set();
  for (const j of tickedJobs) {
    if (j.status === JOB_STATUS.QUEUED || j.status === JOB_STATUS.RENDERING) cancel.push(j.id);
    if (j.status === JOB_STATUS.CANCELLED || j.status === JOB_STATUS.FINISHED || j.status === JOB_STATUS.ERROR) retry.push(j.id);
    if (j.status !== JOB_STATUS.RENDERING) remove.push(j.id);
    if (j.take_folder && !busy.has(j.take_folder)) deleteFolders.add(j.take_folder);
  }
  return { cancel, retry, remove, deleteFolders: [...deleteFolders] };
}

/**
 * The tick state. `getJobs()` is the latest snapshot; `onChange()` fires
 * after any tick changes so the table and the buttons can follow.
 */
export function createRenderSelection({ getJobs, onChange } = {}) {
  const ticked = new Map(); // selectionKey -> boolean

  function jobs() {
    return (getJobs ? getJobs() : []) || [];
  }

  /**
   * Brings the map in line with a snapshot: a row seen for the first time
   * starts ticked, a row already known keeps its tick (a scan that adds
   * takes to an existing queue leaves the old rows as they were), and a row
   * that is gone is forgotten. Returns true when a tick changed.
   */
  function sync(snapshot) {
    const keep = new Set();
    let changed = false;
    for (const j of snapshot || []) {
      const key = selectionKey(j);
      keep.add(key);
      if (!ticked.has(key)) {
        ticked.set(key, true);
        changed = true;
      }
    }
    for (const key of [...ticked.keys()]) {
      if (!keep.has(key)) {
        ticked.delete(key);
        changed = true;
      }
    }
    return changed;
  }

  function isTicked(job) {
    return ticked.get(selectionKey(job)) === true;
  }

  function setTicked(job, value) {
    const key = selectionKey(job);
    if (ticked.get(key) === !!value) return;
    ticked.set(key, !!value);
    onChange?.();
  }

  function setAll(predicate) {
    let changed = false;
    for (const j of jobs()) {
      const key = selectionKey(j);
      const next = !!predicate(j);
      if (ticked.get(key) !== next) {
        ticked.set(key, next);
        changed = true;
      }
    }
    if (changed) onChange?.();
  }

  /** The ticked jobs, in snapshot order. */
  function tickedJobs() {
    return jobs().filter(isTicked);
  }

  /** `{ ticked, total }`, exact, for "3 of 12 selected". */
  function counts() {
    const all = jobs();
    return { ticked: all.filter(isTicked).length, total: all.length };
  }

  return {
    sync,
    isTicked,
    setTicked,
    selectAll: () => setAll(() => true),
    selectNone: () => setAll(() => false),
    selectUnrendered: () => setAll((j) => !wasRendered(j)),
    /** Ticks every row of each finished take, and only those (#644). */
    selectFinished: (opts) => {
      const folders = finishedTakeFolders(jobs(), opts);
      setAll((j) => folders.has(j.take_folder));
    },
    tickedJobs,
    counts,
    targets: () => bulkTargets(tickedJobs(), jobs()),
  };
}
