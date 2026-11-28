import { describe, it, expect, vi } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ planMapBuilds: vi.fn() }));
vi.mock('./themed_confirm.js', () => ({ themedConfirm: vi.fn() }));

import { confirmMixedMapBuilds, groupLeftOut } from './map_build_confirm.js';
import { STRINGS } from './strings.js';

const left = (demoName, mapName) => ({ demoPath: `C:/d/${demoName}`, demoName, mapName });

describe('the confirm before a batch mixing versions of one map (#1547)', () => {
  it('groups the demos left out by map, each once', () => {
    expect(groupLeftOut([left('a.dem', 'dod_x'), left('b.dem', 'dod_y'), left('c.dem', 'dod_x'), left('a.dem', 'dod_x')]))
      .toEqual([{ map: 'dod_x', demos: ['a.dem', 'c.dem'] }, { map: 'dod_y', demos: ['b.dem'] }]);
  });

  it('asks nothing when every demo plays on its version', async () => {
    const confirm = vi.fn();
    const plan = vi.fn(async () => []);
    expect(await confirmMixedMapBuilds(['a', 'b'], 'C:/g/hl.exe', { plan, confirm })).toBe(true);
    expect(confirm).not.toHaveBeenCalled();
  });

  it('names the demos that won\'t play on their version and lets the user start anyway', async () => {
    const confirm = vi.fn(async () => true);
    const plan = vi.fn(async () => [left('old.dem', 'dod_x')]);
    expect(await confirmMixedMapBuilds(['C:/d/old.dem', 'C:/d/new.dem'], 'C:/g/hl.exe', { plan, confirm })).toBe(true);
    expect(plan).toHaveBeenCalledWith(['C:/d/old.dem', 'C:/d/new.dem'], 'C:/g/hl.exe');
    const [message, options] = confirm.mock.calls[0];
    expect(message).toBe(STRINGS.MAP_BUILDS.mixedMessage(1));
    expect(options.confirmLabel).toBe(STRINGS.MAP_BUILDS.START_ANYWAY);
    expect(options.details).toEqual([{ primary: 'dod_x', secondary: 'old.dem' }]);
  });

  it('stops the batch when the user says no, and never blocks it on a failed check', async () => {
    const no = vi.fn(async () => false);
    expect(await confirmMixedMapBuilds(['a', 'b'], 'g', { plan: async () => [left('a', 'dod_x')], confirm: no })).toBe(false);
    const failed = async () => {
      throw new Error('boom');
    };
    expect(await confirmMixedMapBuilds(['a', 'b'], 'g', { plan: failed, confirm: no })).toBe(true);
    expect(await confirmMixedMapBuilds(['a'], 'g', { plan: failed, confirm: no })).toBe(true);
  });
});
