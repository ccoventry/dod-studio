import { describe, it, expect, vi } from 'vitest';

vi.mock('./themed_confirm.js', () => ({ themedConfirm: vi.fn() }));

import { HIGHLIGHTS_VERSION, scanIsCurrent, outdatedDemos } from './scan_version.js';
import { themedConfirm } from './themed_confirm.js';
import { createProjectRescan } from './project_rescan.js';

const hl = (extra = {}) => ({ spectated: false, viewdemo_times: [], kills: [[1, 1, 'K98']], ...extra });
const current = (path, extra = {}) => ({
  path, file_key: '1-abc', map_name: 'dod_anzio', teams: [], signon_maps: ['dod_anzio'], no_camera_data: false,
  highlights_version: HIGHLIGHTS_VERSION, streaks: [hl()], ...extra,
});

describe('which demos an older Studio scanned', () => {
  it('a demo scanned by this version is current', () => {
    expect(scanIsCurrent(current('a.dem'))).toBe(true);
  });

  it('a demo with no version (an older project) is older', () => {
    const demo = current('a.dem');
    delete demo.highlights_version;
    expect(scanIsCurrent(demo)).toBe(false);
  });

  it('a lower version is older, a higher one (a newer Studio) is not', () => {
    expect(scanIsCurrent(current('a.dem', { highlights_version: HIGHLIGHTS_VERSION - 1 }))).toBe(false);
    expect(scanIsCurrent(current('a.dem', { highlights_version: HIGHLIGHTS_VERSION + 1 }))).toBe(true);
  });

  it('a missing field a scan now fills makes it older, whatever its version', () => {
    expect(scanIsCurrent(current('a.dem', { file_key: undefined }))).toBe(false);
    expect(scanIsCurrent(current('a.dem', { map_name: undefined }))).toBe(false);
    expect(scanIsCurrent(current('a.dem', { teams: undefined }))).toBe(false);
    expect(scanIsCurrent(current('a.dem', { signon_maps: undefined }))).toBe(false);
    expect(scanIsCurrent(current('a.dem', { no_camera_data: undefined }))).toBe(false);
    expect(scanIsCurrent(current('a.dem', { streaks: [hl({ spectated: undefined })] }))).toBe(false);
    expect(scanIsCurrent(current('a.dem', { streaks: [hl({ viewdemo_times: undefined })] }))).toBe(false);
  });

  it('outdatedDemos leaves out current and missing demos', () => {
    const missing = current('m.dem', { highlights_version: 0 });
    Object.defineProperty(missing, 'missing', { value: true });
    const demos = [current('a.dem'), current('b.dem', { highlights_version: 0 }), missing, { path: 'c.dem' }];
    expect(outdatedDemos(demos).map((d) => d.path)).toEqual(['b.dem', 'c.dem']);
  });
});

describe('the rescan offer on opening a project', () => {
  const setup = (demos, answer) => {
    themedConfirm.mockReset();
    themedConfirm.mockResolvedValue(answer);
    const scan = vi.fn(async () => true);
    return { scan, offer: createProjectRescan({ getDemos: () => demos, scan }).offerRescan };
  };

  it('counts and lists the older demos, Rescan first', async () => {
    const demos = [current('C:/d/a.dem'), current('C:/d/b.dem', { highlights_version: 0 }), { path: 'C:/d/c.dem' }];
    const { scan, offer } = setup(demos, true);
    expect(await offer()).toBe(true);
    const [message, options] = themedConfirm.mock.calls[0];
    expect(message).toMatch(/^2 demos in this project were scanned with an older DoD Studio\. Rescan them now\?/);
    expect(message).toContain('marks, notes, status and kill ranges are carried over');
    // What "Not now" does sits with the question, above the long list.
    expect(message).toContain('Press “Not now” to leave them as they are');
    expect(options.footer).toBeUndefined();
    expect(options).toMatchObject({ confirmLabel: 'Rescan', cancelLabel: 'Not now', focusConfirm: true });
    expect(options.details.map((d) => d.primary)).toEqual(['b.dem', 'c.dem']);
    expect(scan).toHaveBeenCalledWith(['C:/d/b.dem', 'C:/d/c.dem']);
  });

  it('Not now scans nothing', async () => {
    const { scan, offer } = setup([current('a.dem', { highlights_version: 0 })], false);
    expect(await offer()).toBe(false);
    expect(scan).not.toHaveBeenCalled();
  });

  it('asks nothing when every demo is current', async () => {
    const { scan, offer } = setup([current('a.dem')], true);
    expect(await offer()).toBe(false);
    expect(themedConfirm).not.toHaveBeenCalled();
    expect(scan).not.toHaveBeenCalled();
  });
});
