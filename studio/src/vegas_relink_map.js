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

/**
 * The moves to write, from `candidates` (any of the builders above, in
 * order of preference): one per `from` path (case and slashes ignored), none
 * from a path to itself, and none from a path a later render was saved to
 * after the video there moved away (that path holds the later render now).
 * For the same path, the newer render's move wins. Returns `[{ from, to, why }]`.
 */
export function mergeMoves(takeIndex, candidates) {
  const holders = new Map();
  allRenders(takeIndex).forEach(({ takeKey, render }) => {
    const key = pathKey(render.output_path);
    if (!holders.has(key)) holders.set(key, []);
    holders.get(key).push({ id: renderId(takeKey, render), started: render.started_unix_ms });
  });
  const chosen = new Map();
  (candidates || []).forEach((move) => {
    if (!move.from || !move.to) return;
    const key = pathKey(move.from);
    if (key === pathKey(move.to)) return;
    const holdsIt = (holders.get(key) || []).some((h) => h.id !== move.moverId && h.started > move.moverStarted);
    if (holdsIt) return;
    const known = chosen.get(key);
    if (!known || move.moverStarted > known.moverStarted) {
      chosen.set(key, move);
    }
  });
  return [...chosen.values()].map(({ from, to, why }) => ({ from, to, why }));
}

/** The file's contents, ready for JSON. */
export function relinkMap(moves) {
  return { format: RELINK_FORMAT, version: RELINK_VERSION, moves: moves || [] };
}

/** Count of moves, for the log and the toast. */
export function moveCounts(moves) {
  return { moved: (moves || []).length };
}

/** Where the save dialog opens: the folder holding most of the project's
 *  videos now, with the default name. */
export function relinkDefaultPath(takeIndex) {
  const clips = allRenders(takeIndex).map(({ render }) => ({ path: render.output_path }));
  const edl = edlDefaultPath(clips);
  return edl.slice(0, edl.length - EDL_DEFAULT_FILE_NAME.length) + RELINK_DEFAULT_FILE_NAME;
}
