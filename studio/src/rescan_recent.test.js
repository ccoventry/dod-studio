import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('./ipc_bridge.js', () => ({
  filesExist: vi.fn(),
  readProjectFile: vi.fn(),
  writeProjectFile: vi.fn(),
  estimateRescan: vi.fn(),
  getAppVersion: vi.fn(),
  logFrontendEvent: vi.fn(),
}));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));

const {
  offerStamp, rescanOfferSettings, applyRescanOfferSettings, localDate, backupPath, planRecent,
  sessionsToRescan, worthOfferingAtStartup, skipAllowed, overallFraction, rescanSessions,
  createRescanRecent, SKIP_OVER_SECS, freeBackupPath,
} = await import('./rescan_recent.js');
const { HIGHLIGHTS_VERSION } = await import('./scan_version.js');
const { STRINGS } = await import('./strings.js');

const current = (path) => ({
  path, file_key: '1-abc', map_name: 'dod_anzio', teams: [], signon_maps: ['dod_anzio'], no_camera_data: false,
  highlights_version: HIGHLIGHTS_VERSION, streaks: [],
});
const old = (path) => ({ path, file_key: '1-abc', streaks: [] });
const project = (...demos) => JSON.stringify({ version: '0.12.0', demos });

describe('the start-up offer runs once per update', () => {
  it('is keyed on the app version and the highlight detection version', () => {
    expect(offerStamp('0.2.1')).toBe(`0.2.1+h${HIGHLIGHTS_VERSION}`);
    expect(offerStamp(null)).toBe(null);
  });

  it('keeps the stamp in the settings file; an older file has none', () => {
    applyRescanOfferSettings({ rescan_offered_for: '0.2.0+h1' });
    expect(rescanOfferSettings()).toEqual({ rescan_offered_for: '0.2.0+h1' });
    applyRescanOfferSettings({});
    expect(rescanOfferSettings()).toEqual({ rescan_offered_for: '' });
    applyRescanOfferSettings({ rescan_offered_for: 7 });
    expect(rescanOfferSettings()).toEqual({ rescan_offered_for: '' });
  });
});

describe('backups', () => {
  it('go beside the session, named after it and the date', () => {
    expect(backupPath('H:\\movies\\WSOD25 main.json', '2026-10-23'))
      .toBe('H:\\movies\\WSOD25 main.before-rescan-2026-10-23.json');
    expect(backupPath('D:/p/monday.json', '2026-10-23')).toBe('D:/p/monday.before-rescan-2026-10-23.json');
  });

  it('count up for a second rescan the same day, so the first backup is kept', () => {
    expect(backupPath('D:/p/monday.json', '2026-10-23', 2)).toBe('D:/p/monday.before-rescan-2026-10-23-2.json');
  });

  it('never overwrite an earlier backup: no free name, or no way to tell, means no backup', () => {
    const tries = ['a.1.json', 'a.2.json', 'a.3.json'];
    expect(freeBackupPath(tries, [true, false, false])).toBe('a.2.json');
    expect(freeBackupPath(tries, [true, true, true])).toBe(null);
    expect(freeBackupPath(tries, null)).toBe(null);
  });

  it('dates in local time, zero-padded', () => {
    expect(localDate(new Date(2026, 0, 5, 23, 59))).toBe('2026-01-05');
  });
});

describe('the list of sessions', () => {
  const files = {
    'D:/p/a.json': project(old('D:/d/1.dem'), old('D:/d/2.dem'), current('D:/d/3.dem')),
    'D:/p/b.json': project(current('D:/d/4.dem')),
    'D:/p/c.json': 'not json',
    'D:/p/d.json': project(old('D:/d/gone.dem')),
  };
  const read = async (path) => files[path];
  const exists = async (paths) => paths.map((p) => p !== 'D:/p/gone.json' && p !== 'D:/d/gone.dem');

  it('lists each session with its outdated demos on disk; missing and unreadable files say so', async () => {
    const rows = await planRecent(
      ['D:/p/a.json', 'D:/p/b.json', 'D:/p/gone.json', 'D:/p/c.json', 'D:/p/d.json'], { exists, read },
    );
    expect(rows).toEqual([
      { path: 'D:/p/a.json', name: 'a.json', state: 'outdated', demoPaths: ['D:/d/1.dem', 'D:/d/2.dem'] },
      { path: 'D:/p/gone.json', name: 'gone.json', state: 'missing' },
      { path: 'D:/p/c.json', name: 'c.json', state: 'unreadable' },
    ]);
  });

  it('reads no file that is missing', async () => {
    const reads = [];
    await planRecent(['D:/p/gone.json'], { exists, read: async (p) => { reads.push(p); return ''; } });
    expect(reads).toEqual([]);
  });

  it('when the existence check fails, counts every demo', async () => {
    const rows = await planRecent(['D:/p/d.json'], { exists: async () => null, read });
    expect(rows[0].demoPaths).toEqual(['D:/d/gone.dem']);
  });

  it('starts up only with two sessions to rescan, and leaves skipped ones out', () => {
    const a = { state: 'outdated', demoPaths: ['x'] };
    const b = { state: 'outdated', demoPaths: ['y'], skip: true };
    expect(worthOfferingAtStartup([a, { state: 'missing' }])).toBe(false);
    expect(worthOfferingAtStartup([a, b])).toBe(true);
    expect(sessionsToRescan([a, b, { state: 'missing' }])).toEqual([a]);
  });

  it(`offers Skip for now only past ${SKIP_OVER_SECS} s in all`, () => {
    expect(skipAllowed(SKIP_OVER_SECS)).toBe(false);
    expect(skipAllowed(SKIP_OVER_SECS + 1)).toBe(true);
    expect(skipAllowed(undefined)).toBe(false);
  });
});

describe('one bar across every session', () => {
  it('weights each session by its demos', () => {
    expect(overallFraction([1, 3], 0, 0)).toBe(0);
    expect(overallFraction([1, 3], 1, 0)).toBe(0.25);
    expect(overallFraction([1, 3], 1, 0.5)).toBe(0.625);
    expect(overallFraction([1, 3], 1, 1)).toBe(1);
    expect(overallFraction([], 0, 0.5)).toBe(0);
  });
});

describe('the run', () => {
  const rows = () => [
    { path: 'D:/p/a.json', name: 'a.json', state: 'outdated', demoPaths: ['D:/d/1.dem'] },
    { path: 'D:/p/b.json', name: 'b.json', state: 'outdated', demoPaths: ['D:/d/2.dem'], skip: true },
    { path: 'D:/p/gone.json', name: 'gone.json', state: 'missing' },
    { path: 'D:/p/c.json', name: 'c.json', state: 'outdated', demoPaths: ['D:/d/3.dem'] },
  ];
  function deps(over = {}) {
    const calls = [];
    const lines = [];
    let open = null;
    const d = {
      calls,
      lines,
      backup: async (path) => { calls.push(['backup', path]); return path.replace('.json', '.before-rescan-2026-10-23.json'); },
      open: async (path) => { calls.push(['open', path]); open = path; return true; },
      currentOutdated: () => (open === 'D:/p/a.json' ? ['D:/d/1.dem'] : ['D:/d/3.dem', 'D:/d/gone.dem']),
      exists: async (paths) => paths.map((p) => !p.includes('gone')),
      scan: async (paths) => { calls.push(['scan', paths]); return true; },
      save: async () => { calls.push(['save', open]); return true; },
      log: (line) => lines.push(line),
      now: () => 0,
      ...over,
    };
    return d;
  }

  it('backs up, opens, rescans and saves each session in turn, skipping the skipped and the missing', async () => {
    const d = deps();
    const result = await rescanSessions(rows(), d);
    expect(d.calls).toEqual([
      ['backup', 'D:/p/a.json'], ['open', 'D:/p/a.json'], ['scan', ['D:/d/1.dem']], ['save', 'D:/p/a.json'],
      ['backup', 'D:/p/c.json'], ['open', 'D:/p/c.json'], ['scan', ['D:/d/3.dem']], ['save', 'D:/p/c.json'],
    ]);
    expect(result).toEqual({ done: 2, failed: 0, total: 2, stopped: false });
    expect(d.lines).toEqual([
      STRINGS.RESCAN_RECENT.skippedLog('b.json'),
      STRINGS.RESCAN_RECENT.missingLog('D:/p/gone.json'),
      STRINGS.RESCAN_RECENT.doneLog('a.json', 1, '0.0', 'a.before-rescan-2026-10-23.json'),
      STRINGS.RESCAN_RECENT.doneLog('c.json', 1, '0.0', 'c.before-rescan-2026-10-23.json'),
    ]);
    expect(d.lines.every((l) => l.startsWith('[rescan] '))).toBe(true);
  });

  it('Cancel stops between sessions: the one under way finishes and saves', async () => {
    let stop = false;
    const d = deps({ save: async () => { d.calls.push(['save']); stop = true; return true; }, shouldStop: () => stop });
    const result = await rescanSessions(rows(), d);
    expect(d.calls.map((c) => c[0])).toEqual(['backup', 'open', 'scan', 'save']);
    expect(result).toMatchObject({ done: 1, stopped: true });
    expect(d.lines.at(-1)).toBe(STRINGS.RESCAN_RECENT.stoppedLog(1));
  });

  it('a session that fails is logged and not saved, and the next one still runs', async () => {
    const d = deps({ scan: async (paths) => { d.calls.push(['scan', paths]); return paths[0] !== 'D:/d/1.dem'; } });
    const result = await rescanSessions(rows(), d);
    expect(d.calls.filter((c) => c[0] === 'save')).toEqual([['save', 'D:/p/c.json']]);
    expect(result).toMatchObject({ done: 1, failed: 1 });
    expect(d.lines).toContain(STRINGS.RESCAN_RECENT.failedLog('a.json', '0.0', STRINGS.RESCAN_RECENT.SCAN_FAILED));
  });

  it('a session replaced while it scanned (Ctrl+O, Ctrl+N) is not saved: the save would write over another file', async () => {
    let swapped = false;
    const d = deps({
      scan: async (paths) => { d.calls.push(['scan', paths]); swapped = paths[0] === 'D:/d/1.dem'; return true; },
      isOpen: (path) => !(swapped && path === 'D:/p/a.json'),
    });
    const result = await rescanSessions(rows(), d);
    expect(d.calls.filter((c) => c[0] === 'save')).toEqual([['save', 'D:/p/c.json']]);
    expect(result).toMatchObject({ done: 1, failed: 1 });
    expect(d.lines).toContain(STRINGS.RESCAN_RECENT.failedLog('a.json', '0.0', STRINGS.RESCAN_RECENT.SESSION_CHANGED));
  });

  it('a backup that fails stops that session before it is opened', async () => {
    const d = deps({ backup: async () => { throw new Error('disk full'); } });
    const result = await rescanSessions(rows(), d);
    expect(d.calls).toEqual([]);
    expect(result).toMatchObject({ done: 0, failed: 2 });
  });
});

describe('createRescanRecent', () => {
  beforeEach(() => {
    globalThis.document = { querySelector: () => null };
    applyRescanOfferSettings({});
  });

  const make = (over = {}) => {
    const reads = [];
    const app = {
      getRecent: () => ['D:/p/a.json', 'D:/p/b.json'],
      session: {},
      getDemos: () => [],
      scan: vi.fn(),
      persistAppSettings: vi.fn(async () => {}),
      exists: async (paths) => paths.map(() => true),
      read: async (p) => { reads.push(p); return project(current('D:/d/1.dem')); },
      appVersion: async () => '0.2.1',
      ...over,
    };
    return { app, reads, rr: createRescanRecent(app) };
  };

  it('checks the recent sessions on the first start after an update, and saves that it did', async () => {
    const { app, reads, rr } = make();
    await rr.offerAfterUpdate();
    expect(reads).toEqual(['D:/p/a.json', 'D:/p/b.json']);
    expect(app.persistAppSettings).toHaveBeenCalled();
    expect(rescanOfferSettings()).toEqual({ rescan_offered_for: offerStamp('0.2.1') });
  });

  it('does nothing on later starts of the same version', async () => {
    applyRescanOfferSettings({ rescan_offered_for: offerStamp('0.2.1') });
    const { app, reads, rr } = make();
    await rr.offerAfterUpdate();
    expect(reads).toEqual([]);
    expect(app.persistAppSettings).not.toHaveBeenCalled();
  });

  it('waits while the Setup window is open, without using up the offer', async () => {
    const { app, reads, rr } = make({ isSetupOpen: () => true });
    await rr.offerAfterUpdate();
    expect(reads).toEqual([]);
    expect(app.persistAppSettings).not.toHaveBeenCalled();
    expect(rescanOfferSettings()).toEqual({ rescan_offered_for: '' });
  });

  it('does not start while the queue is scanning: that scan would land in the session it opens', async () => {
    const { reads, rr } = make({ isScanning: () => true });
    expect(await rr.offer()).toBe(null);
    expect(reads).toEqual([]);
    const { reads: startupReads, rr: atStartup, app } = make({ isScanning: () => true });
    await atStartup.offerAfterUpdate();
    expect(startupReads).toEqual([]);
    expect(app.persistAppSettings).not.toHaveBeenCalled();
  });

  it('with one recent session, saves the stamp and leaves it to its own prompt', async () => {
    const { app, reads, rr } = make({ getRecent: () => ['D:/p/a.json'] });
    await rr.offerAfterUpdate();
    expect(reads).toEqual([]);
    expect(app.persistAppSettings).toHaveBeenCalled();
  });
});
