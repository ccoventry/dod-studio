// vegas_relink_map.js
// The VEGAS relink list (#1514): where each rendered video used to be and
// where it is now, for `vegas/DoD Studio Relink Clips.cs`, which swaps every
// old file on an existing VEGAS project for its new one and keeps the edit.
// Pure, so it can be tested on its own; vegas_relink_export.js reads the
// disk and saves it.
//
// The format only grows, like the clip list's (clip_manifest.js): a reader
// ignores fields it doesn't know, and `version` goes up only when a field
// changes meaning.

import { rendersOf, movedFromOf } from './take_renders.js';
import { openProjectText } from './project_file.js';
import { edlDefaultPath, EDL_DEFAULT_FILE_NAME } from './vegas_edl.js';
import { pathKey } from './path_display.js';

export const RELINK_FORMAT = 'dodstudio-relink-map';
export const RELINK_VERSION = 1;
export const RELINK_DEFAULT_FILE_NAME = 'dod_highlights.relink.json';
export const RELINK_SAVE_FILTER = { name: 'VEGAS relink list (*.json)', extensions: ['json'] };

/** Two lengths further apart than this (seconds, about 3 frames at 60 fps)
 *  are a re-render the edit's trims may no longer fit. */
export const LENGTH_TOLERANCE_SECONDS = 0.05;

/** Every render the project records: `{ takeKey, render }`. */
function allRenders(takeIndex) {
  const map = takeIndex?.takeRenders || {};
  return Object.keys(map)
    .filter((key) => Object.prototype.hasOwnProperty.call(takeIndex, key))
    .flatMap((takeKey) => rendersOf(takeIndex, takeKey).map((render) => ({ takeKey, render })));
}

const renderId = (takeKey, render) => `${takeKey}\u0000${render.started_unix_ms}\u0000${render.stream}`;

/**
 * A candidate move. `mover` is the render whose file `to` is; a render
 * rendered to `from` after `mover` left it holds that path now (see
 * `mergeMoves`).
 */
function candidate(from, to, why, takeKey, mover) {
  return { from, to, why, moverId: renderId(takeKey, mover), moverStarted: mover.started_unix_ms };
}

/** One move per path the project's renders were moved from (`moved_from`). */
export function movesFromRecord(takeIndex) {
  return allRenders(takeIndex).flatMap(({ takeKey, render }) => movedFromOf(render)
    .map((from) => candidate(from, render.output_path, 'moved', takeKey, render)));
}

/**
 * The moves an older copy of the project file (`backupText`, its text) gives:
 * each render it records, matched to this project's by take key, start and
 * stream, from its path there (and any `moved_from` it had) to its path now.
 * Returns `{ moves, matched, unmatched }`: renders found in both, and the
 * backup's renders this project no longer has. Throws on text that isn't a
 * project file.
 */
export function movesFromBackup(takeIndex, backupText) {
  const { data } = openProjectText(backupText);
  if (!data || typeof data !== 'object' || Array.isArray(data)) throw new Error('not a project file');
  const saved = data.takeRenders && typeof data.takeRenders === 'object' ? data.takeRenders : {};
  const moves = [];
  let matched = 0;
  let unmatched = 0;
  Object.entries(saved).forEach(([takeKey, list]) => {
    if (!Array.isArray(list)) return;
    const now = rendersOf(takeIndex, takeKey);
    list.forEach((old) => {
      if (!old || typeof old.output_path !== 'string' || old.output_path === '') return;
      const current = now.find((r) => r.started_unix_ms === old.started_unix_ms && r.stream === old.stream);
      if (!current) {
        unmatched += 1;
        return;
      }
      matched += 1;
      [old.output_path, ...movedFromOf(old)].forEach((from) => {
        moves.push(candidate(from, current.output_path, 'moved', takeKey, current));
      });
    });
  });
  return { moves, matched, unmatched };
}

/** The highlight a take holds, so a recapture (a new take of the same
 *  highlights) counts as a re-render of it: its uids, or its key. */
function highlightOf(takeIndex, takeKey) {
  const uids = takeIndex?.[takeKey];
  return Array.isArray(uids) && uids.length > 0 ? [...uids].map(String).sort().join('\u0001') : `key:${takeKey}`;
}

/**
 * Every older render of a highlight mapped to its newest render of the same
 * stream (`why: 're-rendered'`): each path the older one had (now, before a
 * move, and in `backupPaths`, a map of render id to its path in a backup) to
 * where the newest one is.
 */
export function reRenderMoves(takeIndex, backupPaths = new Map()) {
  const groups = new Map();
  allRenders(takeIndex).forEach((entry) => {
    const key = `${highlightOf(takeIndex, entry.takeKey)}\u0000${entry.render.stream}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(entry);
  });
  const moves = [];
  groups.forEach((list) => {
    if (list.length < 2) return;
    const newest = list.reduce((a, b) => (b.render.started_unix_ms > a.render.started_unix_ms ? b : a));
    list.forEach(({ takeKey, render }) => {
      if (render === newest.render) return;
      const paths = [render.output_path, ...movedFromOf(render), ...(backupPaths.get(renderId(takeKey, render)) || [])];
      paths.forEach((from) => moves.push({
        ...candidate(from, newest.render.output_path, 're-rendered', newest.takeKey, newest.render),
        oldId: renderId(takeKey, render),
      }));
    });
  });
  return moves;
}

/** Each backup render's paths by render id, for `reRenderMoves`. */
export function backupPathsOf(backupText) {
  const { data } = openProjectText(backupText);
  const paths = new Map();
  Object.entries(data?.takeRenders || {}).forEach(([takeKey, list]) => {
    if (!Array.isArray(list)) return;
    list.forEach((old) => {
      if (!old || typeof old.output_path !== 'string' || old.output_path === '') return;
      paths.set(renderId(takeKey, old), [old.output_path, ...movedFromOf(old)]);
    });
  });
  return paths;
}

/**
 * The moves to write, from `candidates` (any of the builders above, in
 * order of preference): one per `from` path (case and slashes ignored), none
 * from a path to itself, and none from a path a later render was saved to
 * after the video there moved away (that path holds the later render now).
 * A `moved` move wins over a `re-rendered` one for the same path; between
 * two of a kind, the newer render's. Returns `[{ from, to, why }]`.
 */
export function mergeMoves(takeIndex, candidates) {
  const holders = new Map();
  allRenders(takeIndex).forEach(({ takeKey, render }) => {
    const key = pathKey(render.output_path);
    if (!holders.has(key)) holders.set(key, []);
    holders.get(key).push({ id: renderId(takeKey, render), started: render.started_unix_ms });
  });
  const rank = (move) => (move.why === 'moved' ? 1 : 0);
  const chosen = new Map();
  (candidates || []).forEach((move) => {
    if (!move.from || !move.to) return;
    const key = pathKey(move.from);
    if (key === pathKey(move.to)) return;
    const holdsIt = (holders.get(key) || []).some((h) => h.id !== move.moverId && h.id !== move.oldId
      && h.started > move.moverStarted);
    if (holdsIt) return;
    const known = chosen.get(key);
    if (!known || rank(move) > rank(known)
      || (rank(move) === rank(known) && move.moverStarted > known.moverStarted)) {
      chosen.set(key, move);
    }
  });
  return [...chosen.values()].map(({ from, to, why }) => ({ from, to, why }));
}

/**
 * Puts the lengths read for re-renders on their moves (`seconds_from`,
 * `seconds_to`, from `lengths`, a Map of path to seconds) and returns the
 * re-renders whose lengths differ by more than LENGTH_TOLERANCE_SECONDS.
 */
export function addLengths(moves, lengths) {
  const differ = [];
  moves.forEach((move) => {
    if (move.why !== 're-rendered') return;
    const a = lengths?.get(move.from);
    const b = lengths?.get(move.to);
    if (Number.isFinite(a)) move.seconds_from = Math.round(a * 1000) / 1000;
    if (Number.isFinite(b)) move.seconds_to = Math.round(b * 1000) / 1000;
    if (Number.isFinite(a) && Number.isFinite(b) && Math.abs(a - b) > LENGTH_TOLERANCE_SECONDS) differ.push(move);
  });
  return differ;
}

/** The file's contents, ready for JSON. */
export function relinkMap(moves) {
  return { format: RELINK_FORMAT, version: RELINK_VERSION, moves: moves || [] };
}

/** Counts of each kind of move, for the log and the toast. */
export function moveCounts(moves) {
  const moved = (moves || []).filter((m) => m.why === 'moved').length;
  return { moved, reRendered: (moves || []).length - moved, total: (moves || []).length };
}

/** Where the save dialog opens: the folder holding most of the project's
 *  videos now, with the default name. */
export function relinkDefaultPath(takeIndex) {
  const clips = allRenders(takeIndex).map(({ render }) => ({ path: render.output_path }));
  const edl = edlDefaultPath(clips);
  return edl.slice(0, edl.length - EDL_DEFAULT_FILE_NAME.length) + RELINK_DEFAULT_FILE_NAME;
}
