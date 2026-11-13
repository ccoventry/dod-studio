import { describe, it, expect, vi } from 'vitest';
import { recordCapture } from './take_record.js';
import { recordRender } from './take_renders.js';
import { createProjectVegasExport } from './project_vegas_export.js';

const render = (started, stream, path) => ({
  started_unix_ms: started, stream, codec: 'prores', fps: 60, outcome: 'finished', output_path: path,
});

function setUp({ located, lengths, path = 'D:/out/movie.txt' }) {
  const index = { 't1': ['u1'], 't2': ['u2'] };
  recordCapture(index, 't1', { take_folder: 'D:/caps/t1', captured_at: 1 });
  recordCapture(index, 't2', { take_folder: 'D:/caps/t2', captured_at: 2 });
  recordRender(index, 't1', render(10, 'all', 'D:/v/a.mov'));
  recordRender(index, 't2', render(20, 'all', 'D:/v/b.mov'));
  recordRender(index, 't2', render(30, 'hud:hudcolor', 'D:/v/b_hud.mov'));
  const io = {
    locate: vi.fn(async () => located),
    probe: vi.fn(async (paths, _ffmpeg, onProgress) => {
      paths.forEach((_p, i) => onProgress?.(i + 1, paths.length));
      return new Map(Object.entries(lengths));
    }),
    pickPath: vi.fn(async () => path),
    write: vi.fn(async () => {}),
    saveManifest: vi.fn(async () => ({ clips: [] })),
    log: vi.fn(),
    toast: vi.fn(),
    progress: { show: vi.fn(), hide: vi.fn() },
    now: () => 0,
  };
  const exporter = createProjectVegasExport({ getTakeIndex: () => index, getAllDemos: () => [], io });
  return { io, exporter };
}

describe('createProjectVegasExport', () => {
  it('exports the videos still there, in movie order, and reports the missing', async () => {
    const { io, exporter } = setUp({
      located: ['E:/moved/a.mov', 'D:/v/b.mov', null],
      lengths: { 'E:/moved/a.mov': 3, 'D:/v/b.mov': 4 },
    });
    expect(await exporter.run()).toBe(true);
    const text = io.write.mock.calls[0][1];
    expect(text).toContain('E:/moved/a.mov');
    expect(text).toContain('D:/v/b.mov');
    expect(text).not.toContain('b_hud');
    expect(io.toast).toHaveBeenLastCalledWith(expect.stringContaining('1 missing'), 'success');
  });
  it('says so when no recorded video is on disk', async () => {
    const { io, exporter } = setUp({ located: [null, null, null], lengths: {} });
    expect(await exporter.run()).toBe(false);
    expect(io.write).not.toHaveBeenCalled();
    expect(io.toast).toHaveBeenLastCalledWith(expect.stringContaining('None of the 3 videos'), 'info');
    expect(io.log).toHaveBeenLastCalledWith(expect.stringContaining('wrote no EDL: none of the 3 video(s) found'));
  });
  it('logs an end line when FFmpeg reads no length', async () => {
    const { io, exporter } = setUp({ located: ['D:/v/a.mov', null, null], lengths: {} });
    expect(await exporter.run()).toBe(false);
    expect(io.write).not.toHaveBeenCalled();
    expect(io.log).toHaveBeenLastCalledWith(expect.stringContaining('wrote no EDL: FFmpeg could not read'));
  });
  it('writes nothing when the save dialog is cancelled', async () => {
    const { io, exporter } = setUp({
      located: ['D:/v/a.mov', 'D:/v/b.mov', 'D:/v/b_hud.mov'],
      lengths: { 'D:/v/a.mov': 3, 'D:/v/b.mov': 4, 'D:/v/b_hud.mov': 4 },
      path: null,
    });
    expect(await exporter.run()).toBe(false);
    expect(io.write).not.toHaveBeenCalled();
    expect(io.log).toHaveBeenLastCalledWith(expect.stringContaining('wrote no EDL: the save dialog was cancelled'));
  });
  describe('progress', () => {
    const shown = (io) => io.progress.show.mock.calls.map(([view]) => view.text);
    it('shows the lookup, each length read and the save step, then hides on success', async () => {
      const { io, exporter } = setUp({
        located: ['D:/v/a.mov', 'D:/v/b.mov', 'D:/v/b_hud.mov'],
        lengths: { 'D:/v/a.mov': 3, 'D:/v/b.mov': 4, 'D:/v/b_hud.mov': 4 },
      });
      await exporter.run();
      const lines = shown(io);
      expect(lines[0]).toContain('3 rendered videos');
      expect(io.progress.show.mock.calls[0][0].indeterminate).toBe(true);
      const reads = io.progress.show.mock.calls.map(([v]) => v).filter((v) => !v.indeterminate);
      expect(reads.map((v) => v.pct)).toEqual([0, 33, 67, 100]);
      expect(lines.at(-1)).toContain('Writing');
      expect(io.progress.hide).toHaveBeenCalledTimes(1);
      expect(io.toast).not.toHaveBeenCalledWith(expect.stringContaining('Looking for'), 'info');
    });
    it('hides when the run ends without an EDL: nothing found, cancelled, a lookup that failed', async () => {
      const none = setUp({ located: [null, null, null], lengths: {} });
      await none.exporter.run();
      expect(none.io.progress.hide).toHaveBeenCalledTimes(1);
      const cancelled = setUp({ located: ['D:/v/a.mov', null, null], lengths: { 'D:/v/a.mov': 3 }, path: null });
      await cancelled.exporter.run();
      expect(shown(cancelled.io).some((t) => t.includes('Choose where'))).toBe(true);
      expect(cancelled.io.progress.hide).toHaveBeenCalledTimes(1);
      const failed = setUp({ located: null, lengths: {} });
      await failed.exporter.run();
      expect(failed.io.progress.hide).toHaveBeenCalledTimes(1);
    });
    it('hides when a step throws', async () => {
      const { io, exporter } = setUp({ located: ['D:/v/a.mov', null, null], lengths: { 'D:/v/a.mov': 3 } });
      io.probe = vi.fn(async () => { throw new Error('boom'); });
      await expect(exporter.run()).rejects.toThrow('boom');
      expect(io.progress.hide).toHaveBeenCalledTimes(1);
    });
  });
});
