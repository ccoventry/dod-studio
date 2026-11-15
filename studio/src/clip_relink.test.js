import { describe, it, expect, vi } from 'vitest';
import { recordRender, rendersOf } from './take_renders.js';
import { createClipRelink } from './clip_relink.js';

const KEY = 's/b0';
const render = (started) => ({
  started_unix_ms: started, stream: 'all', codec: 'prores', fps: 60, outcome: 'finished',
  output_path: `D:/exports/clip_${started}.mov`,
});

function setUp({ report, relinkError = null, folder = 'E:/sorted' } = {}) {
  const index = { [KEY]: ['uid'] };
  recordRender(index, KEY, render(1));
  recordRender(index, KEY, render(2));
  let current = index;
  let progressHandler = null;
  const unlisten = vi.fn();
  const io = {
    listen: vi.fn((name, fn) => { progressHandler = fn; return Promise.resolve(unlisten); }),
    relink: vi.fn(async () => {
      progressHandler?.({ payload: { step: 'checking', done: 0, total: 3 } });
      if (relinkError) throw relinkError;
      return report;
    }),
    pickFolder: vi.fn(async () => folder),
    cancel: vi.fn(async () => {}),
    log: vi.fn(),
    toast: vi.fn(),
    progress: { show: vi.fn(), hide: vi.fn() },
  };
  const markDirty = vi.fn();
  const onBusy = vi.fn();
  const relink = createClipRelink({ getTakeIndex: () => current, markDirty, onBusy, io });
  return { index, io, markDirty, onBusy, unlisten, relink, swap: (next) => { current = next; } };
}

const ALL_FOUND = {
  clips: [
    { path: 'D:/exports/clip_1.mov', found_by: 'in_place', size_bytes: 1, content_hash: 'aa' },
    { path: 'E:/sorted/renamed.mov', found_by: 'file_tag', size_bytes: 2, content_hash: 'bb' },
  ],
  search_complete: true,
};

describe('Find Moved Videos', () => {
  it('points the project at a moved video, marks it unsaved and logs the counts', async () => {
    const { index, io, markDirty, unlisten, relink } = setUp({ report: ALL_FOUND });
    expect(await relink.run()).toEqual({ moved: 1, notFound: 0, superseded: 0, changed: 2 });
    expect(io.relink).toHaveBeenCalledWith([
      expect.objectContaining({ take_key: KEY, started_unix_ms: 1, stream: 'all' }),
      expect.objectContaining({ take_key: KEY, started_unix_ms: 2, output_path: 'D:/exports/clip_2.mov' }),
    ], null);
    expect(rendersOf(index, KEY)[1].output_path).toBe('E:/sorted/renamed.mov');
    expect(markDirty).toHaveBeenCalledTimes(1);
    expect(io.log.mock.calls[0][0]).toContain('1 of 2 video(s) found in a new place');
    expect(io.toast).toHaveBeenCalledWith('Found 1 moved video and pointed the project at it.', 'success');
    expect(unlisten).toHaveBeenCalled();
  });

  it('shows progress from the first moment to the end, and greys the button out meanwhile', async () => {
    const { io, onBusy, relink } = setUp({ report: ALL_FOUND });
    await relink.run();
    expect(io.progress.show.mock.calls[0][0].text).toBe('Looking at video 1 of 2 where it was rendered…');
    expect(io.progress.show.mock.calls[1][0].text).toBe('Reading the tag of file 1 of 3…');
    expect(io.progress.hide).toHaveBeenCalledTimes(1);
    expect(onBusy.mock.calls).toEqual([[true], [false]]);
  });

  it('offers a folder when videos are still not found, and looks there', async () => {
    const report = { clips: [ALL_FOUND.clips[0], { path: null, found_by: 'missing' }], search_complete: true };
    const { io, relink } = setUp({ report });
    await relink.run();
    const [text, kind, , options] = io.toast.mock.calls[0];
    expect(text).toBe('No moved videos found. 1 of 2 still not found.');
    expect(kind).toBe('info');
    expect(options.action.label).toBe('Look in a folder…');
    await relink.pickAndRun();
    expect(io.pickFolder).toHaveBeenCalled();
    expect(io.relink).toHaveBeenLastCalledWith(expect.any(Array), 'E:/sorted');
  });

  it('a cancelled folder dialog runs nothing', async () => {
    const { io, relink } = setUp({ report: ALL_FOUND, folder: null });
    expect(await relink.pickAndRun()).toBeNull();
    expect(io.relink).not.toHaveBeenCalled();
  });

  it('a project with no renders says so and asks the backend nothing', async () => {
    const { io, relink, swap } = setUp({ report: ALL_FOUND });
    swap({ other: ['x'] });
    expect(await relink.run()).toBeNull();
    expect(io.relink).not.toHaveBeenCalled();
    expect(io.toast.mock.calls[0][1]).toBe('info');
  });

  it('a failed scan says why and changes nothing', async () => {
    const { index, io, markDirty, relink } = setUp({ relinkError: 'Not searched: the folder E:/x wasn\'t found.' });
    expect(await relink.run()).toBeNull();
    expect(io.toast.mock.calls[0]).toEqual(["Couldn't look for the project's videos: Not searched: the folder E:/x wasn't found.", 'error']);
    expect(markDirty).not.toHaveBeenCalled();
    expect(rendersOf(index, KEY)[1].output_path).toBe('D:/exports/clip_2.mov');
    expect(io.progress.hide).toHaveBeenCalledTimes(1);
  });

  it('drops the answer when another project opened meanwhile', async () => {
    const { io, markDirty, relink, swap } = setUp({ report: ALL_FOUND });
    io.relink.mockImplementation(async () => { swap({}); return ALL_FOUND; });
    expect(await relink.run()).toBeNull();
    expect(markDirty).not.toHaveBeenCalled();
  });

  it('a second click while a scan runs starts nothing', async () => {
    const { io, relink } = setUp({ report: ALL_FOUND });
    const first = relink.run();
    expect(await relink.run()).toBeNull();
    await first;
    expect(io.relink).toHaveBeenCalledTimes(1);
  });

  it('Cancel asks the backend to stop only while a scan runs, and the toast says it stopped', async () => {
    const { io, relink } = setUp({ report: { ...ALL_FOUND, cancelled: true } });
    relink.cancel();
    expect(io.cancel).not.toHaveBeenCalled();
    io.relink.mockImplementation(async () => { relink.cancel(); return { ...ALL_FOUND, cancelled: true }; });
    await relink.run();
    expect(io.cancel).toHaveBeenCalledTimes(1);
    expect(io.toast.mock.calls[0]).toEqual(['Stopped. Found 1 moved video and pointed the project at it.', 'info']);
  });
});
