// render_jobs.js
// Sorting the Render Studio job table, and the batch's overall progress
// (#40). Pure, so it can be tested on its own.

/** The columns a header click can sort by, and how each compares. */
const SORT_KEYS = {
  name: (j) => String(j.name || '').toLowerCase(),
  stream: (j) => String(j.stream || '').toLowerCase(),
  frames: (j) => Number(j.frames) || 0,
  date: (j) => String(j.date || ''),
  status: (j) => STATUS_ORDER[j.status] ?? 99,
  size: (j) => Number(j.output_size_bytes) || 0,
};

/** Sorting by Status groups the batch the way it's worked through. */
const STATUS_ORDER = { Rendering: 0, Queued: 1, Error: 2, Cancelled: 3, Finished: 4 };

export const SORTABLE_COLUMNS = Object.keys(SORT_KEYS);

/**
 * `jobs` in the order `sort` asks for (`{ column, dir: 'asc'|'desc' }`), or
 * job order when `sort` is null. Stable: ties keep job order.
 */
export function sortJobs(jobs, sort) {
  const key = sort && SORT_KEYS[sort.column];
  if (!key) return [...jobs];
  const sign = sort.dir === 'desc' ? -1 : 1;
  return jobs
    .map((job, index) => ({ job, index, value: key(job) }))
    .sort((a, b) => {
      if (a.value < b.value) return -sign;
      if (a.value > b.value) return sign;
      return a.index - b.index;
    })
    .map((entry) => entry.job);
}

/** A header click: ascending, then descending, then back to job order. */
export function nextSort(current, column) {
  if (!current || current.column !== column) return { column, dir: 'asc' };
  if (current.dir === 'asc') return { column, dir: 'desc' };
  return null;
}

/**
 * The batch's overall progress, 0-100: every job that will or did run counts
 * equally (a Finished or failed job as done, a Rendering one at its own
 * progress, a Queued one at 0). Cancelled jobs drop out. `null` with nothing
 * left to count.
 */
export function batchProgress(jobs) {
  const counted = jobs.filter((j) => j.status !== 'Cancelled');
  if (!counted.length) return null;
  const sum = counted.reduce((total, j) => {
    if (j.status === 'Finished' || j.status === 'Error') return total + 100;
    if (j.status === 'Rendering') return total + Math.min(100, Math.max(0, Number(j.progress) || 0));
    return total;
  }, 0);
  return Math.round(sum / counted.length);
}
