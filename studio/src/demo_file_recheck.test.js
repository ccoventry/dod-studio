import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ filesExist: vi.fn(), logFrontendEvent: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));

const { createDemoFileRecheck } = await import('./demo_file_recheck.js');
const { logFrontendEvent } = await import('./ipc_bridge.js');
const { showToast } = await import('./toast.js');

const demo = (name) => ({ name, path: `C:/demos/${name}` });

// The queue, a disk holding `onDisk`, and project_demos.js's two markers as
// small stand-ins: a gone file is found again under `renamed[path]`.
function setUp(demos, { onDisk = demos.map((d) => d.path), renamed = {} } = {}) {
  const disk = new Set(onDisk);
  const target = new EventTarget();
  let clock = 0;
  const app = {
    getDemos: () => demos,
    markDemosGone: vi.fn(async (gone) => gone.map((d) => Object.assign(d, { missing: true, foundAt: renamed[d.path] || null }))),
    markDemosBack: vi.fn((back) => back.forEach((d) => Object.assign(d, { missing: false, foundAt: null }))),
    refreshQueue: vi.fn(),
    useFoundCopies: vi.fn(),
    exists: vi.fn(async (paths) => paths.map((p) => disk.has(p))),
    now: () => clock,
    target,
  };
  const recheck = createDemoFileRecheck(app);
  const focus = async () => {
    target.dispatchEvent(new Event('focus'));
    // Let the look's promises settle.
    for (let i = 0; i < 10; i += 1) await Promise.resolve();
  };
  return { app, recheck, disk, focus, advance: (ms) => { clock += ms; } };
}

beforeEach(() => vi.clearAllMocks());

describe('the look on focus', () => {
  it('marks a demo whose file went missing, redraws the queue and logs it once', async () => {
    const [a, b] = [demo('a.dem'), demo('b.dem')];
    const { app, disk, focus } = setUp([a, b]);
    disk.delete(b.path);
    await focus();
    expect(app.markDemosGone).toHaveBeenCalledWith([b]);
    expect(b.missing).toBe(true);
    expect(app.refreshQueue).toHaveBeenCalledWith(false);
    expect(logFrontendEvent).toHaveBeenCalledTimes(1);
    expect(logFrontendEvent.mock.calls[0][0]).toContain('1 gone (b.dem)');
  });

  it('names a renamed file that was found and offers Use found copy', async () => {
    const a = demo('a.dem');
    const { app, disk, focus } = setUp([a], { renamed: { 'C:/demos/a.dem': 'C:/demos/ax.dem' } });
    disk.delete(a.path);
    await focus();
    const [message, type, , { action }] = showToast.mock.calls[0];
    expect(message).toContain('No longer where the queue says: a.dem');
    expect(message).toContain('Use found copy');
    expect(type).toBe('warning');
    action.onClick();
    expect(app.useFoundCopies).toHaveBeenCalledWith([a]);
  });

  it('clears a missing demo whose file is back', async () => {
    const a = Object.assign(demo('a.dem'), { missing: true, foundAt: null });
    const { app, focus } = setUp([a]);
    await focus();
    expect(app.markDemosBack).toHaveBeenCalledWith([a]);
    expect(a.missing).toBe(false);
    expect(showToast).not.toHaveBeenCalled();
    expect(logFrontendEvent.mock.calls[0][0]).toContain('1 back (a.dem)');
  });

  it('logs nothing and redraws nothing when every file is where it was', async () => {
    const { app, focus } = setUp([demo('a.dem')]);
    await focus();
    expect(app.exists).toHaveBeenCalledTimes(1);
    expect(app.refreshQueue).not.toHaveBeenCalled();
    expect(logFrontendEvent).not.toHaveBeenCalled();
  });

  it('a focus soon after the last one does not look again', async () => {
    const { app, focus, advance } = setUp([demo('a.dem')]);
    await focus();
    advance(500);
    await focus();
    expect(app.exists).toHaveBeenCalledTimes(1);
    advance(5000);
    await focus();
    expect(app.exists).toHaveBeenCalledTimes(2);
  });

  it('a failed look leaves every row as it was', async () => {
    const a = demo('a.dem');
    const { app, focus } = setUp([a]);
    app.exists.mockResolvedValueOnce(null);
    await focus();
    expect(app.markDemosGone).not.toHaveBeenCalled();
    expect(a.missing).toBeUndefined();
  });

  it('says nothing when project_demos.js marked none (its own missing check has them)', async () => {
    const a = demo('a.dem');
    const { app, disk, focus } = setUp([a]);
    app.markDemosGone.mockResolvedValueOnce([]);
    disk.delete(a.path);
    await focus();
    expect(app.markDemosGone).toHaveBeenCalledWith([a]);
    expect(app.refreshQueue).not.toHaveBeenCalled();
    expect(logFrontendEvent).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });
});

describe('ensurePresent', () => {
  it('resolves true when the file is there, with no toast', async () => {
    const a = demo('a.dem');
    const { recheck } = setUp([a]);
    expect(await recheck.ensurePresent([a])).toBe(true);
    expect(showToast).not.toHaveBeenCalled();
  });

  it('a file gone since the last look: marked, one error toast naming it and Locate, resolves false', async () => {
    const a = demo('a.dem');
    const { app, recheck, disk } = setUp([a]);
    disk.delete(a.path);
    expect(await recheck.ensurePresent([a])).toBe(false);
    expect(app.markDemosGone).toHaveBeenCalledWith([a]);
    expect(showToast).toHaveBeenCalledTimes(1);
    const [message, type] = showToast.mock.calls[0];
    expect(type).toBe('error');
    expect(message).toContain('Not found: a.dem');
    expect(message).toContain('Locate');
  });

  it('a demo already marked missing and still gone is refused too', async () => {
    const a = Object.assign(demo('a.dem'), { missing: true, foundAt: 'C:/demos/ax.dem' });
    const { recheck } = setUp([a], { onDisk: [] });
    expect(await recheck.ensurePresent([a])).toBe(false);
    expect(showToast.mock.calls[0][0]).toContain('Use found copy');
  });

  it('a check that failed lets the action run', async () => {
    const a = demo('a.dem');
    const { app, recheck } = setUp([a]);
    app.exists.mockResolvedValueOnce(null);
    expect(await recheck.ensurePresent([a])).toBe(true);
  });
});
