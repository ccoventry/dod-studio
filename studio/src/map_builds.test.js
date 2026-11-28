import { describe, it, expect, vi } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('./ipc_bridge.js', () => ({ addMapBuild: vi.fn(), logFrontendEvent: vi.fn() }));
vi.mock('./pick_path.js', () => ({ pickPath: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));

import { createMapBuilds } from './map_builds.js';
import { STRINGS } from './strings.js';

function actions({ gamePath = 'C:/g/hl.exe', added = null, failure = null, picked = null } = {}) {
  const toast = vi.fn();
  const log = vi.fn();
  const add = vi.fn(async () => {
    if (failure) throw failure;
    return added;
  });
  const pick = vi.fn(async () => picked);
  const builds = createMapBuilds({ getGamePath: () => gamePath, pick, add, toast, log, onProgress: () => {} });
  return { builds, toast, log, add, pick };
}

describe('adding a map build from the banner (#1547)', () => {
  it('adds the file to the install the game path points at and says the demos play on it', async () => {
    const { builds, toast, add, log } = actions({ added: { checksum: 0x61e9d38d } });
    const result = await builds.add({ mapName: 'dod_armory_b5', wanted: 0x61e9d38d, source: 'D:/old.bsp', statusEl: null });
    expect(result).toEqual({ checksum: 0x61e9d38d });
    expect(add).toHaveBeenCalledWith('dod_armory_b5', 'D:/old.bsp', 'C:/g/hl.exe');
    expect(toast).toHaveBeenCalledWith(STRINGS.MAP_BUILDS.addedToast('dod_armory_b5', '61e9d38d'), 'success');
    expect(log).toHaveBeenCalledTimes(1);
  });

  it('says so when the file is another build than the demos want', async () => {
    const { builds, toast } = actions({ added: { checksum: 0x745d9833 } });
    await builds.add({ mapName: 'dod_armory_b5', wanted: 0x61e9d38d, source: 'D:/x.bsp', statusEl: null });
    expect(toast).toHaveBeenCalledWith(
      STRINGS.MAP_BUILDS.otherBuildToast('dod_armory_b5', '745d9833', '61e9d38d'),
      'info'
    );
  });

  it('toasts a failure and resolves null', async () => {
    const { builds, toast } = actions({ failure: 'not a map' });
    expect(await builds.add({ mapName: 'dod_x', wanted: 1, source: 'D:/x.bsp', statusEl: null })).toBeNull();
    expect(toast).toHaveBeenCalledWith(STRINGS.MAP_BUILDS.addFailedToast('dod_x', 'not a map'), 'error');
  });

  it('asks for the game path before adding anything', async () => {
    const { builds, toast, add } = actions({ gamePath: '' });
    expect(await builds.add({ mapName: 'dod_x', wanted: 1, source: 'D:/x.bsp', statusEl: null })).toBeNull();
    expect(add).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith(STRINGS.MAPS.NO_GAME_PATH, 'error');
  });

  it('opens a .bsp picker and reads one path back, or null on Cancel', async () => {
    const { builds, pick } = actions({ picked: ['D:/a.bsp'] });
    expect(await builds.pickFile()).toBe('D:/a.bsp');
    expect(pick.mock.calls[0][0]).toMatchObject({ field: 'maps.add_build', filters: [{ extensions: ['bsp'] }] });
    expect(await actions({ picked: null }).builds.pickFile()).toBeNull();
  });
});

describe('map build warnings from a batch or a preview (#1547)', () => {
  it('shows each one as a warning toast that stays long enough to read', async () => {
    const { startMapBuildWarnings } = await import('./map_builds.js');
    let handler = null;
    const toast = vi.fn();
    startMapBuildWarnings({ listenTo: (_name, h) => { handler = h; return Promise.resolve(() => {}); }, toast });
    handler({ payload: 'old.dem plays on another version of dod_x in the open game.' });
    handler({ payload: '' });
    expect(toast).toHaveBeenCalledTimes(1);
    expect(toast.mock.calls[0][0]).toContain('old.dem');
    expect(toast.mock.calls[0][1]).toBe('warning');
    expect(toast.mock.calls[0][2]).toBeGreaterThanOrEqual(10000);
  });
});
