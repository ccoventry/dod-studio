import { describe, it, expect, vi } from 'vitest';
import { createFolderPlayers } from './folder_players.js';

const strings = {
  PLAYERS_LOOKING: 'Looking up players…',
  playersReading: (done, total) => `Reading players: ${done} / ${total} not read yet`,
  playersInCloud: (n) => `cloud ${n}`,
};

const found = (path, name) => ({ path, demoType: 'POV', players: [{ id: name, name }] });
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

function setup({ lookup, caching = new Set() } = {}) {
  const walks = [];
  const deps = {
    lookupDemoPlayers: vi.fn(lookup || (async () => ({ found: [], to_read: [], in_cloud: [] }))),
    indexDemoPlayers: vi.fn((paths, requestId, lane) => {
      const walk = { paths, requestId, lane };
      walks.push(walk);
      return new Promise((resolve) => { walk.resolve = resolve; });
    }),
    isCaching: (path) => caching.has(path),
    onChange: vi.fn(),
    strings,
    setTimer: () => 1,
    clearTimer: () => {},
  };
  return { players: createFolderPlayers(deps), deps, walks };
}

describe('createFolderPlayers', () => {
  it('answers a folder already read in one lookup, with no walk and no counter', async () => {
    const { players, deps, walks } = setup({
      lookup: async (paths) => ({ found: paths.map((p) => found(p, p)), to_read: [], in_cloud: [] }),
    });
    await players.start(['a.dem', 'b.dem']);
    expect(deps.lookupDemoPlayers).toHaveBeenCalledTimes(1);
    expect(players.get('a.dem').players[0].name).toBe('a.dem');
    expect(players.all()).toHaveLength(2);
    // Only the empty walks that stop an older folder's reads.
    expect(walks.every((w) => w.paths.length === 0 && w.lane === 'analyzer')).toBe(true);
    expect(players.status()).toBe('');
  });

  it('reads only the demos not read yet and counts just those', async () => {
    const { players, walks } = setup({
      lookup: async () => ({ found: [found('a.dem', 'x')], to_read: ['b.dem', 'c.dem'], in_cloud: [] }),
    });
    await players.start(['a.dem', 'b.dem', 'c.dem']);
    const walk = walks.at(-1);
    expect(walk.paths).toEqual(['b.dem', 'c.dem']);
    expect(players.status()).toBe('Reading players: 0 / 2 not read yet');
    expect(players.onPlayers({ lane: 'analyzer', requestId: walk.requestId, path: 'b.dem', demoType: 'HLTV', players: [] })).toBe(true);
    expect(players.status()).toBe('Reading players: 1 / 2 not read yet');
    expect(players.get('b.dem').demoType).toBe('HLTV');
    // Another lane's or an older request's answer isn't this folder's.
    expect(players.onPlayers({ lane: 'queue', requestId: walk.requestId, path: 'c.dem' })).toBe(false);
    expect(players.onPlayers({ lane: 'analyzer', requestId: walk.requestId - 1, path: 'c.dem' })).toBe(false);
    walk.resolve(2);
    await flush();
    expect(players.status()).toBe('');
  });

  it("leaves a cloud drive's uncached demos unread and says so", async () => {
    const { players, walks } = setup({
      lookup: async () => ({ found: [], to_read: ['local.dem'], in_cloud: ['g1.dem', 'g2.dem'] }),
    });
    await players.start(['local.dem', 'g1.dem', 'g2.dem']);
    expect(walks.at(-1).paths).toEqual(['local.dem']);
    walks.at(-1).resolve(1);
    await flush();
    expect(players.status()).toBe('cloud 2');
  });

  it('a failed lookup reads every demo, as before', async () => {
    const { players, walks } = setup({ lookup: async () => null });
    await players.start(['a.dem', 'b.dem']);
    expect(walks.at(-1).paths).toEqual(['a.dem', 'b.dem']);
  });

  it("drops an older folder's lookup", async () => {
    let releaseFirst;
    const { players } = setup({
      lookup: (paths) => paths[0] === 'old.dem'
        ? new Promise((resolve) => { releaseFirst = () => resolve({ found: [found('old.dem', 'o')], to_read: [], in_cloud: [] }); })
        : Promise.resolve({ found: [], to_read: [], in_cloud: [] }),
    });
    const first = players.start(['old.dem']);
    await players.start(['new.dem']);
    releaseFirst();
    await first;
    expect(players.get('old.dem')).toBeUndefined();
  });

  it("doesn't read a demo Cache all has queued, and takes its players as Cache all caches it", async () => {
    const caching = new Set(['b.dem']);
    let cached = new Set(['a.dem']);
    const { players, walks } = setup({
      caching,
      lookup: async (paths) => ({
        found: paths.filter((p) => cached.has(p)).map((p) => found(p, p)),
        to_read: paths.filter((p) => !cached.has(p)),
        in_cloud: [],
      }),
    });
    await players.start(['a.dem', 'b.dem', 'c.dem']);
    expect(walks.at(-1).paths).toEqual(['c.dem']);
    cached = new Set(['a.dem', 'b.dem']);
    await players.onCacheProgress({ last: 'b.dem', finished: false });
    expect(players.get('b.dem')).toBeDefined();
  });

  it('stops its walk when Cache all starts on the same demos, and reads what Cache all left', async () => {
    const caching = new Set();
    let cached = new Set();
    const { players, walks } = setup({
      caching,
      lookup: async (paths) => ({
        found: paths.filter((p) => cached.has(p)).map((p) => found(p, p)),
        to_read: paths.filter((p) => !cached.has(p)),
        in_cloud: [],
      }),
    });
    await players.start(['a.dem', 'b.dem', 'c.dem']);
    const first = walks.at(-1);
    players.onPlayers({ lane: 'analyzer', requestId: first.requestId, path: 'a.dem', demoType: 'POV', players: [] });
    // Cache all clicked: b and c are its now.
    caching.add('a.dem').add('b.dem').add('c.dem');
    await players.onCacheProgress({ finished: false });
    const stop = walks.at(-1);
    expect(stop.requestId).toBeGreaterThan(first.requestId);
    expect(stop.paths).toEqual([]);
    // Stopped after b: c is read here.
    cached = new Set(['a.dem', 'b.dem']);
    caching.clear();
    await players.onCacheProgress({ finished: true, last: 'b.dem' });
    expect(players.get('b.dem')).toBeDefined();
    expect(walks.at(-1).paths).toEqual(['c.dem']);
  });

  it('shows "looking up" only when the lookup is slow', async () => {
    let fire;
    let release;
    const { deps } = setup({
      lookup: () => new Promise((resolve) => { release = () => resolve({ found: [], to_read: [], in_cloud: [] }); }),
    });
    const players = createFolderPlayers({ ...deps, setTimer: (fn) => { fire = fn; return 1; } });
    const started = players.start(['a.dem']);
    expect(players.status()).toBe('');
    fire();
    expect(players.status()).toBe('Looking up players…');
    release();
    await started;
    expect(players.status()).toBe('');
  });
});
