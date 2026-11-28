// map_build_confirm.js
// Before a capture batch (#1547): a game plays one version of each map.
// - A batch Studio starts a game for keeps the installed version of a map
//   when any picked demo wants it, else the first old one; when the picked
//   demos of one map were recorded on different versions, this names the
//   ones left out.
// - A batch in the game that is open already plays on whatever version that
//   game has loaded; this names the demos it won't play on theirs.
// Either way the user can start anyway (warn, don't restrict).

import { openGameMapWarnings, planMapBuilds } from './ipc_bridge.js';
import { themedConfirm } from './themed_confirm.js';
import { STRINGS } from './strings.js';

/** The left-out demos grouped by map: `[{ map, playing, demos: [names] }]`,
 *  `playing` the old version the batch uses instead (null: the installed
 *  one), in the order the maps first appear. */
export function groupLeftOut(leftOut) {
  const byMap = new Map();
  for (const demo of Array.isArray(leftOut) ? leftOut : []) {
    if (!byMap.has(demo.mapName)) byMap.set(demo.mapName, { map: demo.mapName, playing: demo.playing ?? null, demos: [] });
    const entry = byMap.get(demo.mapName);
    if (!entry.demos.includes(demo.demoName)) entry.demos.push(demo.demoName);
  }
  return [...byMap.values()];
}

/** Asks before a batch whose demos mix versions of a map. Resolves true to
 *  go ahead: none do, the check failed, or the user chose to anyway. */
export async function confirmMixedMapBuilds(demoPaths, gamePath, { plan = planMapBuilds, confirm = themedConfirm } = {}) {
  if (!gamePath || !Array.isArray(demoPaths) || demoPaths.length < 2) return true;
  let leftOut;
  try {
    leftOut = await plan(demoPaths, gamePath);
  } catch {
    return true; // never block a batch on the check itself
  }
  const maps = groupLeftOut(leftOut);
  if (!maps.length) return true;
  const S = STRINGS.MAP_BUILDS;
  const count = maps.reduce((n, m) => n + m.demos.length, 0);
  // Each map says its own version: one may stay on the installed version
  // while another plays on an old one.
  return confirm(S.mixedMessage(count), {
    title: S.mixedTitle(count),
    confirmLabel: S.START_ANYWAY,
    details: maps.map((m) => ({ primary: m.map, secondary: S.leftOutDetail(m.demos, m.playing) })),
  });
}

/** Asks before a batch in the open game whose demos that game won't play on
 *  their map version. Resolves true to go ahead. */
export async function confirmOpenGameMapBuilds(demoPaths, gamePath, { check = openGameMapWarnings, confirm = themedConfirm } = {}) {
  if (!gamePath || !Array.isArray(demoPaths) || !demoPaths.length) return true;
  let warnings;
  try {
    warnings = await check(demoPaths, gamePath);
  } catch {
    return true;
  }
  if (!Array.isArray(warnings) || !warnings.length) return true;
  const S = STRINGS.MAP_BUILDS;
  return confirm(S.OPEN_GAME_MESSAGE, {
    title: S.OPEN_GAME_TITLE,
    confirmLabel: S.START_ANYWAY,
    details: warnings.map((w) => ({ primary: w })),
  });
}
