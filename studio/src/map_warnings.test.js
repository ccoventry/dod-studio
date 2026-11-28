import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('./ipc_bridge.js', () => ({ checkDemoMaps: vi.fn(), downloadMap: vi.fn() }));
vi.mock('./download_progress.js', () => ({ mapDownloadLine: vi.fn(), showBytes: vi.fn(), showSliding: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));

import { checkDemoMaps } from './ipc_bridge.js';
import { refreshMapWarnings, resetMapWarnings, listMapProblems, listLibraryUses } from './map_warnings.js';

const OLD = 0xbb690c32;
const NEW = 0xe4d5f058;
const row = (demoName, state, expectedChecksum) => ({
  demoName, state, expectedChecksum, mapName: 'dod_chemille', detail: '',
});

describe('the map warning banner keeps a wrong build when a good demo of the same map follows (#1539)', () => {
  beforeEach(() => {
    globalThis.document = { querySelector: () => null };
    resetMapWarnings();
  });

  it.each([
    ['wrong build first', [row('old.dem', 'wrongBuild', OLD), row('new.dem', 'ok', NEW)]],
    ['good demo first', [row('new.dem', 'ok', NEW), row('old.dem', 'wrongBuild', OLD)]],
  ])('%s', async (_, rows) => {
    checkDemoMaps.mockResolvedValue(rows);
    await refreshMapWarnings(['x'], 'C:/g/hl.exe');
    const problems = listMapProblems();
    expect(problems).toHaveLength(1);
    expect(problems[0].demos).toEqual(['old.dem']);
  });

  it('drops the warning once that build is the installed one', async () => {
    checkDemoMaps.mockResolvedValue([row('old.dem', 'wrongBuild', OLD)]);
    await refreshMapWarnings(['x'], 'C:/g/hl.exe');
    checkDemoMaps.mockResolvedValue([row('old.dem', 'ok', OLD)]);
    await refreshMapWarnings(['x'], 'C:/g/hl.exe');
    expect(listMapProblems()).toHaveLength(0);
  });

  it('lists two builds of one map as two problems', async () => {
    checkDemoMaps.mockResolvedValue([row('a.dem', 'wrongBuild', OLD), row('b.dem', 'wrongBuild', NEW)]);
    await refreshMapWarnings(['x'], 'C:/g/hl.exe');
    expect(listMapProblems()).toHaveLength(2);
  });
});

describe('demos that play on a build from the map builds are news, not problems (#1547)', () => {
  beforeEach(() => {
    globalThis.document = { querySelector: () => null };
    resetMapWarnings();
  });

  it('lists them by map and build instead of as a problem', async () => {
    checkDemoMaps.mockResolvedValue([row('old.dem', 'fromLibrary', OLD), row('older.dem', 'fromLibrary', OLD)]);
    await refreshMapWarnings(['x'], 'C:/g/hl.exe');
    expect(listMapProblems()).toHaveLength(0);
    expect(listLibraryUses()).toEqual([
      { key: `dod_chemille:${OLD}`, mapName: 'dod_chemille', checksum: OLD, demos: ['old.dem', 'older.dem'] },
    ]);
  });

  it('clears the different-version problem once the build is in the map builds', async () => {
    checkDemoMaps.mockResolvedValue([row('old.dem', 'wrongBuild', OLD)]);
    await refreshMapWarnings(['x'], 'C:/g/hl.exe');
    expect(listMapProblems()).toHaveLength(1);
    checkDemoMaps.mockResolvedValue([row('old.dem', 'fromLibrary', OLD)]);
    await refreshMapWarnings(['x'], 'C:/g/hl.exe');
    expect(listMapProblems()).toHaveLength(0);
    expect(listLibraryUses()).toHaveLength(1);
  });

  it('keeps the copy found beside the map for the problem row', async () => {
    checkDemoMaps.mockResolvedValue([
      { ...row('old.dem', 'wrongBuild', OLD), copyBeside: 'C:/g/dod/maps/dod_chemille.bsp.bak' },
    ]);
    await refreshMapWarnings(['x'], 'C:/g/hl.exe');
    expect(listMapProblems()[0].copyBeside).toBe('C:/g/dod/maps/dod_chemille.bsp.bak');
  });

  it('forgets them when the queue is cleared', async () => {
    checkDemoMaps.mockResolvedValue([row('old.dem', 'fromLibrary', OLD)]);
    await refreshMapWarnings(['x'], 'C:/g/hl.exe');
    resetMapWarnings();
    expect(listLibraryUses()).toHaveLength(0);
  });
});

describe('a file of the user\'s in dod_addon\maps (#1547)', () => {
  beforeEach(() => {
    globalThis.document = { querySelector: () => null };
    resetMapWarnings();
  });

  it('is kept on the problem row, with the demos to check again', async () => {
    checkDemoMaps.mockResolvedValue([
      { ...row('old.dem', 'wrongBuild', OLD), demoPath: 'C:/d/old.dem', addonFile: 'C:/g/dod_addon/maps/dod_chemille.bsp' },
    ]);
    await refreshMapWarnings(['C:/d/old.dem'], 'C:/g/hl.exe');
    const [problem] = listMapProblems();
    expect(problem.addonFile).toBe('C:/g/dod_addon/maps/dod_chemille.bsp');
    expect(problem.demoPaths).toEqual(['C:/d/old.dem']);
  });
});
