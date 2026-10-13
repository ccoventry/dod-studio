// render_selection.js
// Which Render tab rows are ticked (#70), and what each bulk button does with
// them. The tick state lives here, keyed by the job rather than by its id:
// a scan numbers its jobs from 0 again, so an id alone would hand a new
// take the tick of whatever sat in that slot before. Pure apart from the
// factory's own map, so it can be tested on its own.

import { JOB_STATUS } from './render_jobs.js';
import { busyTakeFolders } from './take_delete.js';

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

/** How a row starts out: ticked unless its take was rendered before. */
export function defaultTicked(job) {
  return !wasRendered(job);
}

/**
 * The ids and take folders each bulk button acts on, from the ticked jobs
 * and the whole list (`busy` takes come from the whole list: a take with a
 * HUD pair is two rows, and either one being queued or rendering keeps the
 * take, like the per-row Delete Take).
 */
export function bulkTargets(tickedJobs, allJobs) {
  const busy = busyTakeFolders(allJobs);
  const start = [];
  const cancel = [];
  const retry = [];
  const remove = [];
  const deleteFolders = new Set();
  for (const j of tickedJobs) {
    if (j.status === JOB_STATUS.QUEUED) start.push(j.id);
    if (j.status === JOB_STATUS.QUEUED || j.status === JOB_STATUS.RENDERING) cancel.push(j.id);
    if (j.status === JOB_STATUS.CANCELLED || j.status === JOB_STATUS.FINISHED || j.status === JOB_STATUS.ERROR) retry.push(j.id);
    if (j.status !== JOB_STATUS.RENDERING) remove.push(j.id);
    if (j.take_folder && !busy.has(j.take_folder)) deleteFolders.add(j.take_folder);
  }
  return { start, cancel, retry, remove, deleteFolders: [...deleteFolders] };
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
   * gets its default, a row already known keeps its tick (a scan that adds
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
        ticked.set(key, defaultTicked(j));
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
    tickedJobs,
    counts,
    targets: () => bulkTargets(tickedJobs(), jobs()),
  };
}
