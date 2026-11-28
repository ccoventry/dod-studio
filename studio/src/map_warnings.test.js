import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('./ipc_bridge.js', () => ({ checkDemoMaps: vi.fn(), downloadMap: vi.fn() }));
vi.mock('./download_progress.js', () => ({ mapDownloadLine: vi.fn(), showBytes: vi.fn(), showSliding: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));

import { checkDemoMaps } from './ipc_bridge.js';
import { refreshMapWarnings, resetMapWarnings, listMapProblems } from './map_warnings.js';

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
