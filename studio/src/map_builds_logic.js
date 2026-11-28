// map_builds_logic.js
// The rules behind the map builds on the Maps needed banner (#1547), with no
// DOM: which demos play on a build from Studio's library, and what adding a
// build did for the demos that asked for it.

/** A build checksum as the 8 hex digits the map files are named by. */
export function buildText(checksum) {
  return Number.isInteger(checksum) ? (checksum >>> 0).toString(16).padStart(8, '0') : '?';
}

/** The last part of a Windows or forward-slash path. */
export function fileName(path) {
  const text = String(path || '');
  return text.slice(Math.max(text.lastIndexOf('\\'), text.lastIndexOf('/')) + 1);
}

/** The key of one map and build, shared with the banner's problem key. */
export function buildKey(mapName, checksum) {
  return `${mapName}:${checksum ?? 0}`;
}

/** Records in `uses` (key -> { key, mapName, checksum, demos }) that
 *  `demoName` plays on build `checksum` of `mapName` from the library. */
export function addLibraryUse(uses, mapName, checksum, demoName) {
  const key = buildKey(mapName, checksum);
  const use = uses.get(key) ?? { key, mapName, checksum, demos: [] };
  if (!use.demos.includes(demoName)) use.demos.push(demoName);
  uses.set(key, use);
}

/** Map-check rows that play on a library build, grouped by map and build:
 *  [{ key, mapName, checksum, demos: [names] }], in the order first seen. */
export function libraryUses(rows) {
  const uses = new Map();
  for (const row of Array.isArray(rows) ? rows : []) {
    if (row?.state !== 'fromLibrary' || !row.mapName) continue;
    addLibraryUse(uses, row.mapName, row.expectedChecksum, row.demoName);
  }
  return [...uses.values()];
}

/** What adding a build did for demos that want `wanted`: 'matches' when it
 *  is their build (they now play on it), else 'other' (kept for demos
 *  recorded on that one). */
export function addOutcome(added, wanted) {
  return Number.isInteger(wanted) && added?.checksum === wanted ? 'matches' : 'other';
}
