// project_fov.js
// The project's own field of view (#1085, decisions 3 and 4): a project
// always has one, on Project Settings › Picture. One made before it had the
// setting takes, the first time it opens with the New UI on, what the game
// would use for it today, so its look doesn't change: a `mirv_fov <n>` in
// its Initial Commands, or one in a cfg those lines `exec` (read by the
// backend's read-only scan, native picture_fov.rs); else what the game's
// start-up configs set (a movie.cfg's `mirv_fov`, else `default_fov`); only
// with none of those, the game's 90. A batch then sends it after every line.
//
// When that can't be told (no install picked, no game folder, an `exec`'d
// cfg this install hasn't got, or the scan failed), nothing is filled: the
// project keeps no field of view, captures as before, and the next open tries
// again. The session fills only once the "made with another install" offer
// is answered, so it reads the install the project will capture with.

import { projectFovSources, logFrontendEvent } from './ipc_bridge.js';
import { DEFAULT_FOV, isUsableFov } from './clip_settings_logic.js';
import { blockFov, blockWithFov } from './project_settings_logic.js';
import { projectFovFilledLine, projectFovNotFilledLine } from './project_log.js';

/**
 * What the game would use for a project with no field of view, from
 * `sources` (the backend's answer, null when the scan failed). Either
 * `{ fov, from, file, line }`, `from` one of 'lines' (a line of Initial
 * Commands), 'exec' (a cfg they exec), 'game' (a start-up config) or
 * 'default', or `{ fov: null, why, missing }` when it can't be told: `why`
 * 'scan', 'exec' (`missing` the exec lines whose cfg wasn't found) or
 * 'folder' (no game folder to read).
 */
export function fovToFill(sources) {
  if (!sources) return { fov: null, why: 'scan', missing: [] };
  if (sources.unfollowedExecs?.length) return { fov: null, why: 'exec', missing: [...sources.unfollowedExecs] };
  const stated = sources.fromLines;
  if (stated && isUsableFov(stated.value)) {
    return { fov: stated.value, from: stated.file ? 'exec' : 'lines', file: stated.file ?? null, line: stated.line ?? 0 };
  }
  if (!sources.gameFolder) return { fov: null, why: 'folder', missing: [] };
  const game = sources.fromGame;
  if (game && isUsableFov(game.value)) {
    return { fov: game.value, from: 'game', file: game.file ?? null, line: game.line ?? 0 };
  }
  return { fov: DEFAULT_FOV, from: 'default', file: null, line: 0 };
}

/**
 * fovToFill for Initial Commands `lines` in the install at `gamePath` (an
 * hl.exe). No install picked can't be told either (`why` 'install'). Reads
 * only, logs nothing: the Project settings page shows it.
 */
export async function fovTheGameUses({ gamePath, lines }) {
  if (!gamePath) return { fov: null, why: 'install', missing: [] };
  return fovToFill(await projectFovSources(gamePath, lines || []));
}

/**
 * `block` with a field of view, logged: the same block when it has one, or
 * when what the game would use can't be told (logged too, so the next open
 * tries again), else one with fovTheGameUses's value. `path` names the
 * project in the log. Never writes a config.
 */
export async function withFilledFov(block, { gamePath, lines, path }) {
  if (!block || blockFov(block) !== null) return block;
  const fill = await fovTheGameUses({ gamePath, lines });
  if (fill.fov === null) {
    logFrontendEvent(projectFovNotFilledLine(path, fill));
    return block;
  }
  logFrontendEvent(projectFovFilledLine(path, fill));
  return blockWithFov(block, fill.fov);
}
