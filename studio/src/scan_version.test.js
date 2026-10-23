import { describe, it, expect, vi } from 'vitest';

vi.mock('./themed_confirm.js', () => ({ themedConfirm: vi.fn(), setThemedConfirmMessage: vi.fn() }));
vi.mock('./ipc_bridge.js', () => ({ estimateRescan: vi.fn(), logFrontendEvent: vi.fn() }));

import { HIGHLIGHTS_VERSION, scanIsCurrent, outdatedDemos } from './scan_version.js';
import { themedConfirm, setThemedConfirmMessage } from './themed_confirm.js';
import { createProjectRescan } from './project_rescan.js';
import { STRINGS } from './strings.js';

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
  const setup = (demos, answer, timeLine = async () => 'About 3 s.') => {
    themedConfirm.mockReset();
    setThemedConfirmMessage.mockReset();
    if (answer instanceof Promise) themedConfirm.mockReturnValue(answer);
    else themedConfirm.mockResolvedValue(answer);
    const scan = vi.fn(async () => true);
    return { scan, offer: createProjectRescan({ getDemos: () => demos, scan, timeLine }).offerRescan };
  };

  it('counts and lists the older demos, Rescan first', async () => {
    const demos = [current('C:/d/a.dem'), current('C:/d/b.dem', { highlights_version: 0 }), { path: 'C:/d/c.dem' }];
    const { scan, offer } = setup(demos, true);
    expect(await offer()).toBe(true);
    const [message, options] = themedConfirm.mock.calls[0];
    expect(message).toMatch(/^2\/3 demos in this project were scanned with an older DoD Studio\. Rescan them now\?/);
    expect(message).toContain('marks, notes, status and kill ranges are carried over');
    // The time line is worked out after the prompt opens (#937).
    expect(message).toContain('carried over. Working out how long… Press “Not now”');
    // What "Not now" does sits with the question, above the long list.
    expect(message).toContain('Press “Not now” to leave them as they are');
    expect(options.footer).toBeUndefined();
    expect(options).toMatchObject({ confirmLabel: 'Rescan', cancelLabel: 'Not now', focusConfirm: true });
    expect(options.details.map((d) => d.primary)).toEqual(['b.dem', 'c.dem']);
    expect(scan).toHaveBeenCalledWith(['C:/d/b.dem', 'C:/d/c.dem']);
  });

  it('fills in the time once the estimate is in, without waiting for it', async () => {
    let answer;
    let estimated;
    const timeLine = vi.fn(() => new Promise((resolve) => { estimated = resolve; }));
    const { offer } = setup([current('a.dem', { highlights_version: 0 })], new Promise((r) => { answer = r; }), timeLine);
    const offered = offer();
    expect(themedConfirm).toHaveBeenCalledTimes(1);
    expect(timeLine).toHaveBeenCalledWith(['a.dem']);
    estimated('About 4 min: 120 need a full read.');
    await vi.waitFor(() => expect(setThemedConfirmMessage).toHaveBeenCalledTimes(1));
    expect(setThemedConfirmMessage.mock.calls[0][0])
      .toContain('carried over. About 4 min: 120 need a full read. Press “Not now”');
    answer(false);
    expect(await offered).toBe(false);
  });

  it('a failed estimate leaves the time out', async () => {
    let answer;
    const { offer } = setup([current('a.dem', { highlights_version: 0 })], new Promise((r) => { answer = r; }), async () => null);
    const offered = offer();
    await vi.waitFor(() => expect(setThemedConfirmMessage).toHaveBeenCalledTimes(1));
    expect(setThemedConfirmMessage.mock.calls[0][0]).toContain('carried over. Press “Not now”');
    answer(false);
    await offered;
  });

  it('an estimate after the prompt closed changes nothing', async () => {
    let estimated;
    const { offer } = setup([current('a.dem', { highlights_version: 0 })], false, () => new Promise((r) => { estimated = r; }));
    expect(await offer()).toBe(false);
    estimated('About 3 s.');
    await Promise.resolve();
    await Promise.resolve();
    expect(setThemedConfirmMessage).not.toHaveBeenCalled();
  });

  it('says how many of the project\'s demos, or all of them', () => {
    const say = STRINGS.MAIN.outdatedDemosMessage;
    expect(say(158, 158)).toMatch(/^All 158 demos in this project were scanned/);
    expect(say(12, 158)).toMatch(/^12\/158 demos in this project were scanned/);
    expect(say(1, 158)).toMatch(/^1\/158 demos in this project was scanned .* Rescan it now\?/);
    expect(say(1, 1)).toMatch(/^1 demo in this project was scanned .* leave it as it is;/);
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
