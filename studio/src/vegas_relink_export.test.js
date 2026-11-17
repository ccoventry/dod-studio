import { describe, it, expect, vi } from 'vitest';
import { recordRender, relocateRender, rendersOf } from './take_renders.js';
import { createVegasRelinkExport } from './vegas_relink_export.js';

const render = (started, path) => ({
  started_unix_ms: started, stream: 'all', codec: 'prores', fps: 60, outcome: 'finished', output_path: path,
});

function setUp({ save = 'H:/movie/list.relink.json', backup = null, exists = true, reRenders = false, lengths = {} } = {}) {
  const index = { a: ['u1'], b: ['u1'] };
  recordRender(index, 'a', render(1, 'E:/a.mov'));
  recordRender(index, 'b', render(2, 'E:/a_2.mov'));
  relocateRender(index, 'a', rendersOf(index, 'a')[0], { output_path: 'H:/movie/a.mov' });
  const io = {
    pickBackup: vi.fn(async () => (backup ? 'H:/project.backup' : null)),
    read: vi.fn(async () => backup),
    exists: vi.fn(async (paths) => paths.map(() => exists)),
    probe: vi.fn(async (paths, _ffmpeg, onProgress) => {
      paths.forEach((_p, i) => onProgress?.(i + 1, paths.length));
      return new Map(Object.entries(lengths));
    }),
    pickSave: vi.fn(async () => save),
    write: vi.fn(async () => {}),
    log: vi.fn(),
    toast: vi.fn(),
    progress: { show: vi.fn(), hide: vi.fn() },
    now: () => 0,
  };
  const exporter = createVegasRelinkExport({ getTakeIndex: () => index, withReRenders: () => reRenders, io });
  return { io, exporter };
}

const written = (io) => JSON.parse(io.write.mock.calls[0][1]);

describe('createVegasRelinkExport', () => {
  it('saves the moves Studio kept, logs counts and hides the bar', async () => {
    const { io, exporter } = setUp();
    expect(await exporter.run()).toBe(true);
    expect(written(io).moves).toEqual([{ from: 'E:/a.mov', to: 'H:/movie/a.mov', why: 'moved' }]);
    expect(io.log).toHaveBeenLastCalledWith(expect.stringContaining('1 moved, 0 re-rendered, 0 new file(s) not there'));
    expect(io.toast).toHaveBeenLastCalledWith(expect.stringContaining('Relink list saved (1 moved video)'), 'success');
    expect(io.progress.hide).toHaveBeenCalledTimes(1);
    expect(io.probe).not.toHaveBeenCalled();
  });

  it('says so when the new file is not there', async () => {
    const { io, exporter } = setUp({ exists: false });
    await exporter.run();
    expect(io.toast).toHaveBeenLastCalledWith(expect.stringContaining("1 new file isn't there now"), 'info');
  });

  it('with re-renders ticked, reads lengths and warns when they differ', async () => {
    const { io, exporter } = setUp({ reRenders: true, lengths: { 'H:/movie/a.mov': 4.2, 'E:/a.mov': 4.2, 'E:/a_2.mov': 5 } });
    await exporter.run();
    const { moves } = written(io);
    expect(moves).toContainEqual({ from: 'H:/movie/a.mov', to: 'E:/a_2.mov', why: 're-rendered', seconds_from: 4.2, seconds_to: 5 });
    expect(io.toast).toHaveBeenLastCalledWith(expect.stringContaining('1 re-render has a different length'), 'info');
    const reads = io.progress.show.mock.calls.map(([v]) => v).filter((v) => !v.indeterminate);
    expect(reads.at(-1).pct).toBe(100);
  });

  it('writes nothing when nothing moved', async () => {
    const { io } = setUp();
    const empty = { a: ['u'] };
    recordRender(empty, 'a', render(1, 'E:/a.mov'));
    const quiet = createVegasRelinkExport({ getTakeIndex: () => empty, io });
    expect(await quiet.run()).toBe(false);
    expect(io.write).not.toHaveBeenCalled();
    expect(io.toast).toHaveBeenLastCalledWith(expect.stringContaining('No video of the loaded project has been moved'), 'info');
    expect(io.log).toHaveBeenLastCalledWith(expect.stringContaining('no list saved: no video has moved'));
  });

  it('writes nothing when the save dialog is cancelled', async () => {
    const { io, exporter } = setUp({ save: null });
    expect(await exporter.run()).toBe(false);
    expect(io.write).not.toHaveBeenCalled();
    expect(io.log).toHaveBeenLastCalledWith(expect.stringContaining('the save dialog was cancelled'));
  });

  it('from a backup: reads it, never writes it, and adds its moves', async () => {
    const backup = JSON.stringify({ takeRenders: { b: [render(2, 'D:/old/a_2.mov')] } });
    const { io, exporter } = setUp({ backup });
    expect(await exporter.run({ fromBackup: true })).toBe(true);
    expect(io.read).toHaveBeenCalledWith('H:/project.backup');
    expect(io.write).toHaveBeenCalledTimes(1);
    expect(io.write.mock.calls[0][0]).toBe('H:/movie/list.relink.json');
    expect(written(io).moves).toContainEqual({ from: 'D:/old/a_2.mov', to: 'E:/a_2.mov', why: 'moved' });
    expect(io.log).toHaveBeenCalledWith(expect.stringContaining('backup read: 1 render(s) matched, 0 not in this project'));
  });

  it('from a backup: a cancelled pick or an unreadable file saves nothing', async () => {
    const cancelled = setUp();
    expect(await cancelled.exporter.run({ fromBackup: true })).toBe(false);
    expect(cancelled.io.pickSave).not.toHaveBeenCalled();
    const bad = setUp({ backup: 'not json' });
    expect(await bad.exporter.run({ fromBackup: true })).toBe(false);
    expect(bad.io.toast).toHaveBeenLastCalledWith(expect.stringContaining("Couldn't read H:/project.backup"), 'error');
    expect(bad.io.write).not.toHaveBeenCalled();
  });
});
