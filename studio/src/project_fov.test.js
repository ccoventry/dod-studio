// The project's own field of view (#1085, decision 3): an older project with
// none takes what the game would use for it today (a mirv_fov line, a cfg
// its lines exec, the start-up configs, else the game's 90), and takes
// nothing when that can't be told; the activity log says which.

import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ projectFovSources: vi.fn(), logFrontendEvent: vi.fn() }));

import { projectFovSources, logFrontendEvent } from './ipc_bridge.js';
import { fovToFill, fovTheGameUses, withFilledFov } from './project_fov.js';
import { blockFov } from './project_settings_logic.js';
import { projectFovFilledLine, projectFovNotFilledLine } from './project_log.js';

const WSOD = ['exec dicewsod25.cfg', 'ds_hud_hide_scoreboard 1'];
/** The backend's answer, nothing set anywhere unless `extra` says. */
const sources = (extra = {}) => ({ fromLines: null, fromGame: null, configs: [], unfollowedExecs: [], gameFolder: true, ...extra });
const MOVIE_CFG = { value: 105, file: 'movie.cfg', line: 3 };

describe('fovToFill', () => {
  it("takes the value a cfg the lines exec sets, and says where (the WSOD25 project's 110)", () => {
    expect(fovToFill(sources({ fromLines: { value: 110, file: 'dicewsod25.cfg', line: 4 }, fromGame: MOVIE_CFG })))
      .toEqual({ fov: 110, from: 'exec', file: 'dicewsod25.cfg', line: 4 });
  });

  it('takes a mirv_fov line of its own', () => {
    expect(fovToFill(sources({ fromLines: { value: 105, file: null, line: 0 } })))
      .toEqual({ fov: 105, from: 'lines', file: null, line: 0 });
  });

  it("takes what the start-up configs set when its lines set none, as the game does today (review 9/10)", () => {
    expect(fovToFill(sources({ fromGame: MOVIE_CFG }))).toEqual({ fov: 105, from: 'game', file: 'movie.cfg', line: 3 });
  });

  it("takes the game's 90 only when nothing sets one", () => {
    expect(fovToFill(sources())).toEqual({ fov: 90, from: 'default', file: null, line: 0 });
  });

  it("takes nothing when a cfg its lines exec isn't in this install (review 1)", () => {
    const missing = sources({ fromLines: { value: 100, file: null, line: 0 }, unfollowedExecs: ['exec dicewsod25.cfg'] });
    expect(fovToFill(missing)).toEqual({ fov: null, why: 'exec', missing: ['exec dicewsod25.cfg'] });
  });

  it('takes nothing with no game folder to read, unless its own line sets it', () => {
    expect(fovToFill(sources({ gameFolder: false }))).toMatchObject({ fov: null, why: 'folder' });
    expect(fovToFill(sources({ gameFolder: false, fromLines: { value: 100, file: null, line: 0 } })).fov).toBe(100);
  });

  it('takes nothing when the scan failed', () => {
    expect(fovToFill(null)).toMatchObject({ fov: null, why: 'scan' });
  });
});

describe('fovTheGameUses', () => {
  beforeEach(() => vi.clearAllMocks());

  it('asks nothing and takes nothing with no hl.exe picked', async () => {
    expect(await fovTheGameUses({ gamePath: '', lines: ['mirv_fov 100'] })).toMatchObject({ fov: null, why: 'install' });
    expect(projectFovSources).not.toHaveBeenCalled();
  });
});

describe('withFilledFov', () => {
  beforeEach(() => vi.clearAllMocks());

  it('fills a block with none from the scan, logging where it came from', async () => {
    projectFovSources.mockResolvedValue(sources({ fromLines: { value: 110, file: 'dicewsod25.cfg', line: 4 } }));
    const block = { picture: { width: 1920 }, futureField: 1 };
    const next = await withFilledFov(block, { gamePath: 'C:/Games/HL/hl.exe', lines: WSOD, path: 'D:/p/wsod.dodstudio' });
    expect(projectFovSources).toHaveBeenCalledWith('C:/Games/HL/hl.exe', WSOD);
    expect(blockFov(next)).toBe(110);
    expect(next).toMatchObject({ picture: { width: 1920 }, futureField: 1 });
    expect(block.picture.fov).toBeUndefined();
    expect(logFrontendEvent).toHaveBeenCalledWith(projectFovFilledLine('D:/p/wsod.dodstudio',
      { fov: 110, from: 'exec', file: 'dicewsod25.cfg', line: 4 }));
  });

  it("leaves the block without one, and logs why, when an exec'd cfg is missing", async () => {
    projectFovSources.mockResolvedValue(sources({ unfollowedExecs: ['exec dicewsod25.cfg'] }));
    const block = { picture: { width: 1920 } };
    expect(await withFilledFov(block, { gamePath: 'C:/Games/HL/hl.exe', lines: WSOD, path: 'a.dodstudio' })).toBe(block);
    expect(logFrontendEvent).toHaveBeenCalledWith(projectFovNotFilledLine('a.dodstudio', { why: 'exec', missing: ['exec dicewsod25.cfg'] }));
  });

  it('leaves a block that has one alone, asking nothing', async () => {
    const block = { picture: { fov: 100 } };
    expect(await withFilledFov(block, { gamePath: '', lines: ['mirv_fov 75'], path: 'x' })).toBe(block);
    expect(projectFovSources).not.toHaveBeenCalled();
    expect(logFrontendEvent).not.toHaveBeenCalled();
  });
});

describe('the log lines', () => {
  it('name the source and that the look stays, or that it took the default', () => {
    expect(projectFovFilledLine('a.dodstudio', { fov: 110, from: 'exec', file: 'dicewsod25.cfg', line: 4 }))
      .toBe("[project] a.dodstudio had no field of view of its own: took 110 from dicewsod25.cfg, line 4, which its Initial Commands exec, so its look doesn't change; saved with the next save");
    expect(projectFovFilledLine('a.dodstudio', { fov: 105, from: 'game', file: 'movie.cfg', line: 3 }))
      .toContain('took 105 from movie.cfg, line 3, which the game runs at start-up');
    expect(projectFovFilledLine('a.dodstudio', { fov: 90, from: 'default', file: null, line: 0 }))
      .toContain("took 90, the game's default");
  });

  it('say why none was taken', () => {
    expect(projectFovNotFilledLine('a.dodstudio', { why: 'exec', missing: ['exec dicewsod25.cfg'] }))
      .toBe("[project] a.dodstudio has no field of view of its own and keeps none for now: a cfg its Initial Commands exec isn't in the picked install (exec dicewsod25.cfg). It captures as before; the next open tries again");
    expect(projectFovNotFilledLine('a.dodstudio', { why: 'install' })).toContain('no hl.exe is picked');
  });
});
