// queue_sort_by.js
// The Master Demo Queue's Sort by dropdown: the order of the shown demos, and
// with Group by on, their order inside each group (groups keep the order
// queue_group_by.js gives them). Like grouping it only orders the rows: which
// demos are shown, and what Review highlights and the bulk actions act on,
// stay getVisibleDemos' job. The pure parts come first, so they can be tested
// on their own.

import { listedStreaks } from './queue_filters.js';

export const SORT_BY = {
  ADDED: 'added',
  NAME: 'name',
  NEWEST: 'newest',
  OLDEST: 'oldest',
  HIGHLIGHTS: 'highlights',
};

/** Where the choice is kept: a per-install view preference, not project data. */
export const SORT_BY_STORE_KEY = 'dodstudio.queueSortBy';

/** A stored or picked value as a SORT_BY value; anything unknown is Added. */
export function parseSortBy(value) {
  return Object.values(SORT_BY).includes(value) ? value : SORT_BY.ADDED;
}

const byName = (a, b) =>
  String(a.name || '').localeCompare(String(b.name || ''), undefined, { sensitivity: 'base', numeric: true });

// Demos without a date go last either way.
const dateOf = (d) => (Number.isFinite(d.modified_unix_secs) ? d.modified_unix_secs : null);
const byDate = (newestFirst) => (a, b) => {
  const x = dateOf(a);
  const y = dateOf(b);
  if (x === null || y === null) return (x === null) - (y === null);
  return newestFirst ? y - x : x - y;
};

const COMPARE = {
  [SORT_BY.NAME]: byName,
  [SORT_BY.NEWEST]: byDate(true),
  [SORT_BY.OLDEST]: byDate(false),
  [SORT_BY.HIGHLIGHTS]: (a, b) => listedStreaks(b).length - listedStreaks(a).length,
};

/**
 * `demos` in `by`'s order, as a new array. Added is the queue's own order (the
 * order demos went in). Ties keep the queue's order, then fall back to name.
 */
export function sortDemos(demos, by) {
  const list = [...(demos || [])];
  const compare = COMPARE[parseSortBy(by)];
  if (!compare) return list;
  const at = new Map(list.map((d, i) => [d, i]));
  return list.sort((a, b) => compare(a, b) || (compare === byName ? 0 : byName(a, b)) || at.get(a) - at.get(b));
}

// What master_pane.js reads at render time.
let current = SORT_BY.ADDED;

/** The queue's current order, a SORT_BY value. */
export function currentSortBy() {
  return current;
}

function readStored() {
  try { return localStorage.getItem(SORT_BY_STORE_KEY); } catch { return null; }
}

function store(value) {
  try { localStorage.setItem(SORT_BY_STORE_KEY, value); } catch { /* storage unavailable */ }
}

/** Wires the Sort by dropdown (`#master-sort-by-select`) and restores the last choice. */
export function createQueueSortBy({ renderQueue }) {
  const select = document.querySelector('#master-sort-by-select');
  current = parseSortBy(readStored());
  if (select) select.value = current;
  select?.addEventListener('change', (e) => {
    current = parseSortBy(e.target.value);
    store(current);
    renderQueue();
  });
}
