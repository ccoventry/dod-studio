// The project's own field of view (#1085, decision 3): an older project with
// none takes what its lines give (a mirv_fov line, or a cfg they exec), else
// the game's 90, and says so in the activity log.

import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ projectFovSources: vi.fn(), logFrontendEvent: vi.fn() }));

import { projectFovSources, logFrontendEvent } from './ipc_bridge.js';
import { fovToFill, withFilledFov } from './project_fov.js';
import { blockFov } from './project_settings_logic.js';
import { projectFovFilledLine } from './project_log.js';

const WSOD = ['exec dicewsod25.cfg', 'ds_hud_hide_scoreboard 1'];

describe('fovToFill', () => {
  it("takes the value a cfg the lines exec sets, and says where (the WSOD25 project's 110)", () => {
    const sources = { fromLines: { value: 110, file: 'dicewsod25.cfg', line: 4 }, configs: [] };
    expect(fovToFill(sources, WSOD)).toEqual({ fov: 110, file: 'dicewsod25.cfg', line: 4, isDefault: false });
  });

  it('takes a mirv_fov line of its own', () => {
    const sources = { fromLines: { value: 105, file: null, line: 0 }, configs: [] };
    expect(fovToFill(sources, ['mirv_fov 105'])).toEqual({ fov: 105, file: null, line: 0, isDefault: false });
  });

  it("takes the game's 90 when nothing in its lines sets one, whatever a start-up config says", () => {
    const sources = { fromLines: null, configs: [{ value: 105, file: 'movie.cfg', line: 3 }] };
    expect(fovToFill(sources, ['ds_hud_hide_scoreboard 1'])).toEqual({ fov: 90, file: null, line: 0, isDefault: true });
  });

  it('reads the lines alone when the scan failed', () => {
    expect(fovToFill(null, ['mirv_fov 100', 'mirv_fov handleZoom enabled 1']).fov).toBe(100);
    expect(fovToFill(null, ['exec dicewsod25.cfg']).isDefault).toBe(true);
  });
});

describe('withFilledFov', () => {
  beforeEach(() => vi.clearAllMocks());

  it('fills a block with none from the scan, logging where it came from', async () => {
    projectFovSources.mockResolvedValue({ fromLines: { value: 110, file: 'dicewsod25.cfg', line: 4 }, configs: [] });
    const block = { picture: { width: 1920 }, futureField: 1 };
    const next = await withFilledFov(block, { gamePath: 'C:/Games/HL/hl.exe', lines: WSOD, path: 'D:/p/wsod.dodstudio' });
    expect(projectFovSources).toHaveBeenCalledWith('C:/Games/HL/hl.exe', WSOD);
    expect(blockFov(next)).toBe(110);
    expect(next).toMatchObject({ picture: { width: 1920 }, futureField: 1 });
    expect(block.picture.fov).toBeUndefined();
    expect(logFrontendEvent).toHaveBeenCalledWith(projectFovFilledLine('D:/p/wsod.dodstudio',
      { fov: 110, file: 'dicewsod25.cfg', line: 4, isDefault: false }));
  });

  it('leaves a block that has one alone, asking nothing', async () => {
    const block = { picture: { fov: 100 } };
    expect(await withFilledFov(block, { gamePath: '', lines: ['mirv_fov 75'], path: 'x' })).toBe(block);
    expect(projectFovSources).not.toHaveBeenCalled();
    expect(logFrontendEvent).not.toHaveBeenCalled();
  });
});

describe('the log line', () => {
  it('names the source and that the look stays, or that it took the default', () => {
    expect(projectFovFilledLine('a.dodstudio', { fov: 110, file: 'dicewsod25.cfg', line: 4, isDefault: false }))
      .toBe("[project] a.dodstudio had no field of view of its own: took 110 from dicewsod25.cfg, line 4, which its Initial Commands exec, so its look doesn't change; saved with the next save");
    expect(projectFovFilledLine('a.dodstudio', { fov: 90, file: null, line: 0, isDefault: true }))
      .toContain("took 90, the game's default");
  });
});
