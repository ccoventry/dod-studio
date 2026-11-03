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
    probe: vi.fn(async () => new Map(Object.entries(lengths))),
    pickPath: vi.fn(async () => path),
    write: vi.fn(async () => {}),
    saveManifest: vi.fn(async () => ({ clips: [] })),
    log: vi.fn(),
    toast: vi.fn(),
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
  });
  it('writes nothing when the save dialog is cancelled', async () => {
    const { io, exporter } = setUp({
      located: ['D:/v/a.mov', 'D:/v/b.mov', 'D:/v/b_hud.mov'],
      lengths: { 'D:/v/a.mov': 3, 'D:/v/b.mov': 4, 'D:/v/b_hud.mov': 4 },
      path: null,
    });
    expect(await exporter.run()).toBe(false);
    expect(io.write).not.toHaveBeenCalled();
  });
});
