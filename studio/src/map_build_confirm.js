// map_build_confirm.js
// Before a capture batch (#1547): a game plays one version of each map, so
// when the picked demos of one map were recorded on different versions,
// some of them won't play on theirs. The batch keeps the installed version
// when any demo wants it, else the first old one; this names the demos left
// out and lets the user start anyway (warn, don't restrict).

import { planMapBuilds } from './ipc_bridge.js';
import { themedConfirm } from './themed_confirm.js';
import { STRINGS } from './strings.js';

/** The left-out demos grouped by map: `[{ map, demos: [names] }]`, in the
 *  order the maps first appear. */
export function groupLeftOut(leftOut) {
  const byMap = new Map();
  for (const demo of Array.isArray(leftOut) ? leftOut : []) {
    if (!byMap.has(demo.mapName)) byMap.set(demo.mapName, { map: demo.mapName, demos: [] });
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
  return confirm(S.mixedMessage(count), {
    title: S.mixedTitle(count),
    confirmLabel: S.START_ANYWAY,
    details: maps.map((m) => ({ primary: m.map, secondary: m.demos.join(', ') })),
  });
}
