// crash_map_warnings.js
// Before a capture batch: picked demos on a map a game session crashed on,
// with a cause DoD Studio knows (#207, native::crash_maps). Such a crash
// can't be predicted from the demo file, so this only knows what has
// happened before.

import { crashMapWarnings } from './ipc_bridge.js';
import { themedConfirm } from './themed_confirm.js';
import { STRINGS } from './strings.js';

/** One entry per map, with the demos on it: `[{ map, cause, build, count,
 *  lastUnixSecs, demos: [names] }]`, in the order the maps first appear. */
export function groupByMap(warnings) {
  const byMap = new Map();
  for (const w of warnings || []) {
    const key = `${w.map.toLowerCase()}|${w.cause}`;
    if (!byMap.has(key)) {
      byMap.set(key, { map: w.map, cause: w.cause, build: w.build, count: w.count, lastUnixSecs: w.last_unix_secs, demos: [] });
    }
    const entry = byMap.get(key);
    if (!entry.demos.includes(w.demo_name)) entry.demos.push(w.demo_name);
  }
  return [...byMap.values()];
}

/** Asks before a batch with demos on a crash-prone map. Resolves true to go
 *  ahead: none are, or the user chose to anyway. */
export async function confirmCrashMaps(demoPaths) {
  if (!demoPaths.length) return true;
  let warnings;
  try {
    warnings = await crashMapWarnings(demoPaths);
  } catch {
    return true; // never block a batch on the check itself
  }
  const maps = groupByMap(warnings);
  if (!maps.length) return true;
  const S = STRINGS.CRASH_MAPS;
  return themedConfirm(S.message(maps.length), {
    title: S.title(maps.reduce((n, m) => n + m.demos.length, 0)),
    confirmLabel: S.START_ANYWAY,
    details: maps.map((m) => ({
      primary: `${m.map}: ${S.demos(m.demos)}`,
      secondary: `${m.cause} ${S.seen(m.count, new Date(m.lastUnixSecs * 1000).toLocaleDateString(), m.build)}`,
    })),
  });
}
