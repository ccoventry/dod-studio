// folder_players.js — who is in each demo of the Demo Analyzer's folder
// (#437), read as cheaply as possible (#992):
// - One `lookup_demo_players` call answers every demo whose players are
//   cached, at once, so a folder Studio has read shows its players with no
//   counter. It opens no demo.
// - Only the demos left are read one by one (`index_demo_players`, the
//   'analyzer' lane), and the counter counts only those.
// - A demo on a cloud drive with no cache entry isn't read: that would
//   download it. Cache all, or opening it, reads it.
// - While Cache all runs, the demos it has queued aren't read here too: their
//   players come from the cache entry it writes, as each one finishes.

/**
 * @param {{
 *   lookupDemoPlayers: (paths: string[]) => Promise<{found: {path: string, demoType: string, players: object[]}[], to_read: string[], in_cloud: string[]} | null>,
 *   indexDemoPlayers: (paths: string[], requestId: number, lane: string) => Promise<number>,
 *   isCaching: (path: string) => boolean,
 *   onChange: () => void,
 *   strings: { PLAYERS_LOOKING: string, playersReading: (done: number, total: number) => string, playersInCloud: (n: number) => string },
 *   setTimer?: (fn: () => void, ms: number) => any,
 *   clearTimer?: (id: any) => void,
 * }} deps
 */
export function createFolderPlayers({
  lookupDemoPlayers,
  indexDemoPlayers,
  isCaching,
  onChange,
  strings,
  setTimer = setTimeout,
  clearTimer = clearTimeout,
}) {
  const LANE = 'analyzer';
  // A lookup this quick shows nothing: a folder already read shouldn't
  // flash a status line.
  const LOOKING_DELAY_MS = 300;

  let players = new Map(); // path -> { demoType, players }
  let folder = 0; // bumped per folder: answers for an older one are dropped
  let request = 0; // the backend walk's id; a newer one stops the older walk
  let paths = [];
  let walk = []; // the demos the current walk reads
  let walkDone = 0;
  let walking = false;
  let looking = false;
  let lookingShown = false;
  let lookingTimer = null;
  let inCloud = new Set();
  let leftForCache = new Set(); // queued in Cache all, not read here
  let runSeen = false; // a Cache all run has sent its first event

  const stopLooking = () => {
    looking = false;
    lookingShown = false;
    if (lookingTimer !== null) clearTimer(lookingTimer);
    lookingTimer = null;
  };

  const take = (found) => {
    for (const f of found || []) {
      players.set(f.path, { demoType: f.demoType, players: f.players || [] });
      inCloud.delete(f.path);
      leftForCache.delete(f.path);
    }
  };

  /** Reads `list` one by one. Only one walk runs at a time. */
  const startWalk = (list) => {
    request += 1;
    walk = list;
    walkDone = 0;
    walking = list.length > 0;
    const id = request;
    // Sent for an empty list too: the new id stops the last walk.
    Promise.resolve(indexDemoPlayers(list, id, LANE)).then(() => {
      if (id !== request || list.length === 0) return;
      walking = false;
      onChange();
    });
  };

  /** Starts on a new folder's demos. */
  async function start(folderPaths) {
    folder += 1;
    const token = folder;
    paths = folderPaths;
    players = new Map();
    inCloud = new Set();
    leftForCache = new Set();
    stopLooking();
    // Stop the last folder's walk now: on a cloud drive it downloads demos.
    startWalk([]);
    if (folderPaths.length === 0) {
      onChange();
      return;
    }
    looking = true;
    lookingTimer = setTimer(() => {
      lookingTimer = null;
      if (token !== folder || !looking) return;
      lookingShown = true;
      onChange();
    }, LOOKING_DELAY_MS);
    onChange();
    const lookup = await lookupDemoPlayers(folderPaths);
    if (token !== folder) return;
    stopLooking();
    if (!lookup) {
      // The lookup failed: read every demo, as before #992.
      startWalk(folderPaths.filter((p) => !isCaching(p)));
      onChange();
      return;
    }
    take(lookup.found);
    inCloud = new Set(lookup.in_cloud || []);
    const toRead = [];
    for (const path of lookup.to_read || []) {
      if (isCaching(path)) leftForCache.add(path);
      else toRead.push(path);
    }
    startWalk(toRead);
    onChange();
  }

  /** A `demo_players` event. Returns whether it was this folder's. */
  function onPlayers(p) {
    if (p.lane !== LANE || p.requestId !== request) return false;
    walkDone += 1;
    if (!p.error) players.set(p.path, { demoType: p.demoType, players: p.players || [] });
    onChange();
    return true;
  }

  /** Looks `list` up again, for demos Cache all may have cached since. */
  async function relook(list) {
    const token = folder;
    const lookup = await lookupDemoPlayers(list);
    if (token !== folder || !lookup) return null;
    take(lookup.found);
    return lookup;
  }

  /** A `demo_cache_progress` event from Cache all. */
  async function onCacheProgress(progress) {
    if (progress.finished) {
      runSeen = false;
    } else if (!runSeen) {
      runSeen = true;
      // The cloud line leaves out the demos the run has queued: redraw it
      // now, not when the first one is cached (on a cloud drive, after its
      // whole download).
      if (inCloud.size > 0) onChange();
    }
    // Cache all reads the demos now: stop reading the ones it has queued
    // here, so no demo is analysed twice. They come in as it caches them.
    if (!progress.finished && walking) {
      const queued = walk.slice(walkDone).filter((p) => isCaching(p) && !players.has(p));
      if (queued.length > 0) {
        for (const path of queued) leftForCache.add(path);
        startWalk(walk.slice(walkDone).filter((p) => !isCaching(p) && !players.has(p)));
        onChange();
      }
    }
    const last = progress.last;
    if (last && paths.includes(last) && !players.has(last)) {
      if (await relook([last])) onChange();
    }
    if (progress.finished) {
      // Stopped early, or a demo it couldn't cache: what's left is read
      // here, or waits again if it is in the cloud.
      const left = [...leftForCache, ...inCloud].filter((p) => !players.has(p));
      if (left.length === 0) return;
      const lookup = await relook(left);
      if (!lookup) return;
      inCloud = new Set(lookup.in_cloud || []);
      leftForCache = new Set();
      if (!walking && (lookup.to_read || []).length > 0) startWalk(lookup.to_read);
      onChange();
    }
  }

  /** The status line under the player box: empty once there's nothing to say. */
  function status() {
    if (looking) return lookingShown ? strings.PLAYERS_LOOKING : '';
    if (walking) return strings.playersReading(walkDone, walk.length);
    const cloud = [...inCloud].filter((p) => !players.has(p) && !isCaching(p)).length;
    return cloud > 0 ? strings.playersInCloud(cloud) : '';
  }

  return {
    start,
    onPlayers,
    onCacheProgress,
    status,
    get: (path) => players.get(path),
    all: () => [...players.values()],
  };
}
