// queue_group_by.js
// The Master Demo Queue's Group by dropdown (#823, grew out of #685's Group by
// match checkbox): None, Match, Player, Map, Folder, Date or Demo type, each
// group under a `.queue-match-header` line with its counts. Grouping only
// orders the shown demos: which demos are shown is the filters' job
// (master_pane.js's getVisibleDemos), and Review highlights and the bulk
// actions read that, so a grouping can never change what they act on. The
// pure parts come first, so they can be tested on their own.

import { groupedQueue, matchHeaderText } from './queue_match_groups.js';
import { groupPlayers } from './player_filter.js';
import { folderOf, shortFolder } from './path_display.js';
import { localDate } from './clip_name.js';
import { listedStreaks } from './queue_filters.js';
import { STRINGS } from './strings.js';

export const GROUP_BY = {
  NONE: 'none',
  MATCH: 'match',
  PLAYER: 'player',
  MAP: 'map',
  FOLDER: 'folder',
  DATE: 'date',
  TYPE: 'type',
};

/** Where the choice is kept: a per-install view preference, not project data. */
export const GROUP_BY_STORE_KEY = 'dodstudio.queueGroupBy';

/** A stored or picked value as a GROUP_BY value; anything unknown is None. */
export function parseGroupBy(value) {
  return Object.values(GROUP_BY).includes(value) ? value : GROUP_BY.NONE;
}

/** "label · 3 demos, 12 highlights", counted over the demos shown under it. */
function countedHeader(label, demos) {
  const highlights = demos.reduce((n, d) => n + listedStreaks(d).length, 0);
  return STRINGS.WORKSPACE.groupHeader(label, demos.length, highlights);
}

/**
 * Puts `demos` into groups by `keyOf(demo)` -> `{ key, label, title?, last? }`.
 * Groups sort by label (case-insensitive, numbers in order), the `last` ones
 * (HLTV, not known) after the rest in the order they first appear; demos
 * keep the queue's order inside a group.
 */
function groupByKey(demos, keyOf, sortKey = (g) => g.label) {
  const groups = new Map();
  for (const demo of demos) {
    const k = keyOf(demo);
    if (!groups.has(k.key)) groups.set(k.key, { ...k, demos: [] });
    groups.get(k.key).demos.push(demo);
  }
  const all = [...groups.values()];
  const named = all
    .filter((g) => !g.last)
    .sort((a, b) => sortKey(a).localeCompare(sortKey(b), undefined, { sensitivity: 'base', numeric: true }));
  const last = all.filter((g) => g.last).sort((a, b) => a.last - b.last);
  return [...named, ...last].map((g) => ({ text: countedHeader(g.label, g.demos), title: g.title || '', demos: g.demos }));
}

/**
 * The shown demos in groups, `[{ text, title, demos }]` in display order:
 * `text` is the header line, `title` its tooltip (may be empty). Null for
 * GROUP_BY.NONE, which shows the queue's own order.
 *
 * `matches`: the session-numbered match summaries (queue_match_groups.js).
 * `recorderOf(demo)`: who recorded a POV demo (`{ id, name }`) or null.
 */
export function groupDemos(demos, by, { matches = [], recorderOf = () => null } = {}) {
  const W = STRINGS.WORKSPACE;
  const list = demos || [];
  switch (parseGroupBy(by)) {
    case GROUP_BY.MATCH:
      return groupedQueue(list, matches).map((g) => {
        // Which signal called the half, from the first demo that has one.
        const detail = g.demos.find((d) => d.match_info?.detail)?.match_info.detail;
        return { text: matchHeaderText(g.match, g.demos), title: detail ? W.halfTooltip(detail) : '', demos: g.demos };
      });
    case GROUP_BY.PLAYER: {
      // One group per player id, labelled the way the Player filter lists
      // them: every name the id was seen with, and its SteamID.
      const labels = new Map(groupPlayers(list.map(recorderOf).filter(Boolean)).map((o) => [o.id, o.label]));
      return groupByKey(list, (demo) => {
        const id = recorderOf(demo)?.id;
        if (id) return { key: `p:${id}`, label: labels.get(id) || id };
        if (demo.is_pov === false) return { key: 'hltv', label: W.GROUP_HLTV, last: 1 };
        return { key: 'none', label: W.GROUP_NO_RECORDER, last: 2 };
      });
    }
    case GROUP_BY.MAP:
      return groupByKey(list, (demo) => {
        const map = String(demo.map_name || '').trim();
        return map ? { key: `m:${map.toLowerCase()}`, label: map } : { key: 'none', label: W.GROUP_NO_MAP, last: 1 };
      });
    case GROUP_BY.FOLDER:
      // Windows paths: either slash, any case is the same folder.
      return groupByKey(list, (demo) => {
        const folder = folderOf(demo.path);
        return { key: folder.replace(/\//g, '\\').toLowerCase(), label: shortFolder(folder, 80), title: folder, full: folder };
      }, (g) => g.full);
    case GROUP_BY.DATE:
      // YYYY-MM-DD sorts oldest first as text.
      return groupByKey(list, (demo) => {
        const day = localDate(demo.modified_unix_secs);
        return day ? { key: day, label: day, title: W.groupDateTitle(day) } : { key: 'none', label: W.GROUP_NO_DATE, last: 1 };
      });
    case GROUP_BY.TYPE:
      return groupByKey(list, (demo) => {
        if (demo.is_pov === true) return { key: 'pov', label: W.GROUP_POV, last: 1 };
        if (demo.is_pov === false) return { key: 'hltv', label: W.GROUP_HLTV, last: 2 };
        return { key: 'none', label: W.GROUP_NO_TYPE, last: 3 };
      });
    default:
      return null;
  }
}

// What master_pane.js reads at render time.
let current = GROUP_BY.NONE;

/** The queue's current grouping, a GROUP_BY value. */
export function currentGroupBy() {
  return current;
}

/** Whether the queue is shown in match order, so every demo should be placed. */
export function isGroupedByMatch() {
  return current === GROUP_BY.MATCH;
}

function readStored() {
  try { return localStorage.getItem(GROUP_BY_STORE_KEY); } catch { return null; }
}

function store(value) {
  try { localStorage.setItem(GROUP_BY_STORE_KEY, value); } catch { /* storage unavailable */ }
}

/**
 * Wires the Group by dropdown (`#master-group-by-select`) and restores the
 * last choice. `getDemos()` is the queue; `renderQueue()` redraws it;
 * `regroupMatches(opts)` is queue_match_groups.js's regroup. Choosing Match
 * reads the demos the match grouping hasn't placed yet (a project loaded
 * with a cold cache, a split demo), since that is when the order matters.
 */
export function createQueueGroupBy({ getDemos, renderQueue, regroupMatches }) {
  const select = document.querySelector('#master-group-by-select');
  current = parseGroupBy(readStored());
  if (select) select.value = current;

  select?.addEventListener('change', (e) => {
    current = parseGroupBy(e.target.value);
    store(current);
    renderQueue();
    if (isGroupedByMatch() && getDemos().some((d) => !d.missing && !d.match_info)) {
      regroupMatches({ parseMissing: true });
    }
  });
}
