import { describe, it, expect, vi } from 'vitest';
import { routeOpenedFiles, normalizeDemTarget, createLaunchFiles } from './launch_files.js';
import { demOpenWithNote } from './file_types_settings.js';

// Files opened from Explorer (#528): a project opens like Load Project, a
// demo goes where Settings → Open .dem Files In says.

describe('routeOpenedFiles', () => {
  it('opens the first project and keeps every demo', () => {
    expect(routeOpenedFiles([
      'D:\\demos\\a.dem', 'D:\\p\\one.dodstudio', 'D:\\p\\two.json', 'D:\\demos\\b.dem', 'D:\\notes.txt',
    ])).toEqual({ project: 'D:\\p\\one.dodstudio', demos: ['D:\\demos\\a.dem', 'D:\\demos\\b.dem'] });
  });

  it('opens nothing for nothing', () => {
    expect(routeOpenedFiles([])).toEqual({ project: null, demos: [] });
    expect(routeOpenedFiles(undefined)).toEqual({ project: null, demos: [] });
  });
});

describe('normalizeDemTarget', () => {
  it('is the Demo Analyzer unless the queue was picked', () => {
    expect(normalizeDemTarget('queue')).toBe('queue');
    expect(normalizeDemTarget('analyzer')).toBe('analyzer');
    expect(normalizeDemTarget(undefined)).toBe('analyzer');
    expect(normalizeDemTarget('something new')).toBe('analyzer');
  });
});

function harness({ files, target = 'analyzer' }) {
  const batches = [...files];
  const app = {
    openProject: vi.fn(async () => true),
    openDemoInAnalyzer: vi.fn(async () => {}),
    addDemosToQueue: vi.fn(async () => true),
    getDemTarget: () => target,
    take: vi.fn(async () => batches.shift() || []),
    listenFor: vi.fn(async (event, cb) => { app.fire = cb; }),
  };
  return { app, launch: createLaunchFiles(app) };
}

describe('createLaunchFiles', () => {
  it('opens a double-clicked project the same way as Load Project', async () => {
    const { app, launch } = harness({ files: [['D:\\p\\WSOD25.dodstudio']] });
    await launch.start();
    expect(app.openProject).toHaveBeenCalledWith('D:\\p\\WSOD25.dodstudio');
    expect(app.openDemoInAnalyzer).not.toHaveBeenCalled();
    expect(app.addDemosToQueue).not.toHaveBeenCalled();
  });

  it('sends a demo to the Demo Analyzer by default', async () => {
    const { app, launch } = harness({ files: [['D:\\demos\\a.dem', 'D:\\demos\\b.dem']] });
    await launch.start();
    expect(app.openDemoInAnalyzer).toHaveBeenCalledTimes(1);
    expect(app.openDemoInAnalyzer).toHaveBeenCalledWith('D:\\demos\\b.dem');
    expect(app.addDemosToQueue).not.toHaveBeenCalled();
  });

  it("adds demos to Studio's queue when Settings says so", async () => {
    const { app, launch } = harness({ files: [['D:\\demos\\a.dem', 'D:\\demos\\b.dem']], target: 'queue' });
    await launch.start();
    expect(app.addDemosToQueue).toHaveBeenCalledWith(['D:\\demos\\a.dem', 'D:\\demos\\b.dem']);
    expect(app.openDemoInAnalyzer).not.toHaveBeenCalled();
  });

  it('a later double-click is opened when the backend says it arrived', async () => {
    const { app, launch } = harness({ files: [[], ['D:\\p\\second.dodstudio']] });
    await launch.start();
    expect(app.listenFor).toHaveBeenCalledWith('open_files_pending', expect.any(Function));
    expect(app.openProject).not.toHaveBeenCalled();
    await app.fire();
    expect(app.openProject).toHaveBeenCalledWith('D:\\p\\second.dodstudio');
  });

  it('a second batch waits for the first (its unsaved-changes prompt) to finish', async () => {
    let finishFirst;
    const { app, launch } = harness({ files: [['D:\\p\\one.dodstudio'], ['D:\\p\\two.dodstudio']] });
    app.openProject.mockImplementationOnce(() => new Promise((resolve) => { finishFirst = resolve; }));
    const first = launch.start();
    await vi.waitFor(() => expect(app.openProject).toHaveBeenCalledTimes(1));
    const second = launch.drain();
    await Promise.resolve();
    expect(app.openProject).toHaveBeenCalledTimes(1);
    finishFirst(true);
    await first;
    await second;
    expect(app.openProject).toHaveBeenLastCalledWith('D:\\p\\two.dodstudio');
  });

  it('one failure does not stop later files', async () => {
    const { app, launch } = harness({ files: [['D:\\p\\bad.dodstudio'], ['D:\\p\\good.dodstudio']] });
    app.openProject.mockRejectedValueOnce(new Error('boom'));
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    await launch.start();
    await launch.drain();
    expect(app.openProject).toHaveBeenLastCalledWith('D:\\p\\good.dodstudio');
    error.mockRestore();
  });
});

describe('demOpenWithNote', () => {
  it('says nothing unless Open With opens another copy', () => {
    expect(demOpenWithNote(null)).toBe('');
    expect(demOpenWithNote({ registered: false, thisCopy: false, otherExe: null })).toBe('');
    expect(demOpenWithNote({ registered: true, thisCopy: true, otherExe: null })).toBe('');
    expect(demOpenWithNote({ registered: true, thisCopy: false, otherExe: 'D:\\dev\\dod-studio.exe' }))
      .toContain('D:\\dev\\dod-studio.exe');
  });
});
