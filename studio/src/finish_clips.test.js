import { describe, it, expect, vi } from 'vitest';
import {
  finishCodecFor,
  finishFolders,
  jobInFolders,
  finishProgress,
  finishSkipReason,
  createFinishController,
} from './finish_clips.js';

const codecs = (overrides = {}) => ({
  obs: 'source_copy',
  video: 'render_tab',
  frames: 'render_tab',
  renderCodec: 'dnxhr',
  renderCustomArgs: '',
  ...overrides,
});

describe('finishCodecFor', () => {
  it('keeps an OBS take as captured by default', () => {
    expect(finishCodecFor('obs', codecs())).toEqual({ codec: 'source_copy', customArgs: '' });
  });

  it('uses the Render tab codec for "same as the Codec setting"', () => {
    expect(finishCodecFor('direct_to_video', codecs())).toEqual({ codec: 'dnxhr', customArgs: '' });
    expect(finishCodecFor('frame_sequence', codecs())).toEqual({ codec: 'dnxhr', customArgs: '' });
  });

  it('carries the Custom args only with the Render tab codec that owns them', () => {
    const c = codecs({ renderCodec: 'custom', renderCustomArgs: '-c:v mpeg4' });
    expect(finishCodecFor('frame_sequence', c)).toEqual({ codec: 'custom', customArgs: '-c:v mpeg4' });
    expect(finishCodecFor('obs', { ...c, obs: 'h264' })).toEqual({ codec: 'h264', customArgs: '' });
  });

  it('picks each mode its own codec', () => {
    const c = codecs({ obs: 'h264_nvenc', video: 'prores', frames: 'h264' });
    expect(finishCodecFor('obs', c).codec).toBe('h264_nvenc');
    expect(finishCodecFor('direct_to_video', c).codec).toBe('prores');
    expect(finishCodecFor('frame_sequence', c).codec).toBe('h264');
  });

  it('never keeps a non-OBS take as captured — it has no sound of its own', () => {
    const c = codecs({ video: 'source_copy', frames: 'source_copy' });
    expect(finishCodecFor('direct_to_video', c).codec).toBe('dnxhr');
    expect(finishCodecFor('frame_sequence', c).codec).toBe('dnxhr');
  });

  it('treats an unknown or missing mode as a frame sequence', () => {
    expect(finishCodecFor(undefined, codecs({ frames: 'h264' })).codec).toBe('h264');
  });
});

describe('finishFolders', () => {
  it('keeps renderable takes once each and drops the rest', () => {
    const blocks = [
      { take_folder: 'D:\\cap\\s1\\demo_b0', renderable: true },
      { take_folder: 'D:\\cap\\s1\\demo_b1', renderable: false },
      { take_folder: 'd:/cap/s1/demo_b0/', renderable: true },
      { take_folder: 'D:\\cap\\s1\\demo_b2', renderable: true },
    ];
    expect(finishFolders(blocks)).toEqual(['D:\\cap\\s1\\demo_b0', 'D:\\cap\\s1\\demo_b2']);
  });

  it('copes with no blocks', () => {
    expect(finishFolders(undefined)).toEqual([]);
  });
});

describe('jobInFolders', () => {
  const folders = ['D:\\cap\\s1\\demo_b1'];

  it('matches the take0000 folder HLAE creates inside a block', () => {
    expect(jobInFolders({ take_folder: 'D:\\cap\\s1\\demo_b1\\take0000' }, folders)).toBe(true);
    expect(jobInFolders({ take_folder: 'D:\\cap\\s1\\demo_b1' }, folders)).toBe(true);
  });

  it('ignores case and slash direction, as Windows does', () => {
    expect(jobInFolders({ take_folder: 'd:/CAP/s1/demo_b1/take0000' }, folders)).toBe(true);
  });

  it('does not match a sibling whose name merely starts the same', () => {
    expect(jobInFolders({ take_folder: 'D:\\cap\\s1\\demo_b10\\take0000' }, folders)).toBe(false);
  });
});

describe('finishProgress', () => {
  it('counts only this finish\'s jobs', () => {
    const jobs = [
      { take_folder: 'D:\\a\\take0000', status: 'Finished' },
      { take_folder: 'D:\\b\\take0000', status: 'Rendering' },
      { take_folder: 'D:\\c\\take0000', status: 'Queued' },
      { take_folder: 'D:\\d\\take0000', status: 'Error' },
      { take_folder: 'D:\\other\\take0000', status: 'Finished' },
    ];
    expect(finishProgress(jobs, ['D:\\a', 'D:\\b', 'D:\\c', 'D:\\d'])).toEqual({
      total: 4, queued: 1, rendering: 1, finished: 1, failed: 1, cancelled: 0, done: 2,
    });
  });
});

describe('finishSkipReason', () => {
  const ok = { enabled: true, outcome: 'complete', folders: ['D:\\a'], exportDirs: ['E:\\out'] };

  it('finishes a completed batch when switched on', () => {
    expect(finishSkipReason(ok)).toBeNull();
  });

  it('does nothing while switched off, whatever else is true', () => {
    expect(finishSkipReason({ ...ok, enabled: false })).toBe('off');
  });

  it('leaves a cancelled batch alone', () => {
    expect(finishSkipReason({ ...ok, outcome: 'cancelled' })).toBe('cancelled');
  });

  it('needs at least one renderable take and an export folder', () => {
    expect(finishSkipReason({ ...ok, folders: [] })).toBe('nothing');
    expect(finishSkipReason({ ...ok, exportDirs: [] })).toBe('no_export_dir');
  });
});

// ── The controller ───────────────────────────────────────────────────────────

function harness({ queueResult = 2 } = {}) {
  const calls = [];
  const deps = {
    queue: vi.fn(async (payload) => { calls.push(['queue', payload]); return queueResult; }),
    start: vi.fn(async () => { calls.push(['start']); }),
    onWaiting: vi.fn(),
    onStarted: vi.fn(),
    onProgress: vi.fn(),
    onDone: vi.fn(),
    onFailed: vi.fn(),
    onNothingFound: vi.fn(),
  };
  return { deps, calls, controller: createFinishController(deps) };
}

const request = (folders) => ({ payload: { render_directories: folders, codec: 'prores' } });
const job = (id, folder, status, extra = {}) => ({ id, take_folder: `${folder}\\take0000`, status, ...extra });
const flush = () => new Promise((r) => setTimeout(r, 0));

describe('createFinishController', () => {
  it('queues and starts straight away when the Render tab is idle', async () => {
    const { deps, calls, controller } = harness();
    controller.onJobsSnapshot([]);
    controller.request(request(['D:\\a', 'D:\\b']));
    await flush();
    expect(calls.map((c) => c[0])).toEqual(['queue', 'start']);
    expect(deps.onStarted).toHaveBeenCalledWith(2);
  });

  it('reports progress and then the end, once', async () => {
    const { deps, controller } = harness();
    controller.request(request(['D:\\a', 'D:\\b']));
    await flush();

    controller.onJobsSnapshot([job('0', 'D:\\a', 'Rendering'), job('1', 'D:\\b', 'Queued')]);
    expect(deps.onProgress).toHaveBeenLastCalledWith(expect.objectContaining({ total: 2, rendering: 1, done: 0 }));

    const final = [job('0', 'D:\\a', 'Finished'), job('1', 'D:\\b', 'Error')];
    controller.onJobsSnapshot(final);
    controller.onJobsSnapshot(final);
    expect(deps.onDone).toHaveBeenCalledTimes(1);
    expect(deps.onDone.mock.calls[0][0]).toMatchObject({ finished: 1, failed: 1, total: 2 });
    expect(controller.isActive()).toBe(false);
  });

  it('waits for a busy Render tab, then goes on its own', async () => {
    const { deps, calls, controller } = harness();
    controller.onJobsSnapshot([job('0', 'D:\\mine', 'Rendering')]);
    controller.request(request(['D:\\a']));
    await flush();
    expect(calls).toEqual([]);
    expect(deps.onWaiting).toHaveBeenCalled();

    controller.onJobsSnapshot([job('0', 'D:\\mine', 'Finished')]);
    await flush();
    expect(calls.map((c) => c[0])).toEqual(['queue', 'start']);
  });

  it('treats a staged, unstarted Render batch as busy', async () => {
    const { calls, controller } = harness();
    controller.onJobsSnapshot([job('0', 'D:\\mine', 'Queued')]);
    controller.request(request(['D:\\a']));
    await flush();
    expect(calls).toEqual([]);
  });

  it('runs a second batch only after the first has finished', async () => {
    const { calls, controller } = harness({ queueResult: 1 });
    controller.request(request(['D:\\a']));
    await flush();
    controller.request(request(['D:\\b']));
    await flush();
    expect(calls.filter((c) => c[0] === 'queue')).toHaveLength(1);

    controller.onJobsSnapshot([job('0', 'D:\\a', 'Finished')]);
    await flush();
    const queued = calls.filter((c) => c[0] === 'queue');
    expect(queued).toHaveLength(2);
    expect(queued[1][1].render_directories).toEqual(['D:\\b']);
  });

  it('counts a batch that ended before start replied', async () => {
    const { deps, controller } = harness({ queueResult: 1 });
    deps.start.mockImplementation(async () => {
      controller.onJobsSnapshot([job('0', 'D:\\a', 'Finished')]);
    });
    controller.request(request(['D:\\a']));
    await flush();
    expect(deps.onStarted).toHaveBeenCalledWith(1);
    expect(deps.onDone).toHaveBeenCalledTimes(1);
  });

  it('reports a failed queue and moves on', async () => {
    const { deps, controller } = harness();
    deps.queue.mockRejectedValueOnce('boom');
    controller.request(request(['D:\\a']));
    await flush();
    expect(deps.onFailed).toHaveBeenCalledWith('boom');
    expect(deps.start).not.toHaveBeenCalled();
    expect(controller.isActive()).toBe(false);
  });

  it('says so when the scan finds nothing', async () => {
    const { deps, controller } = harness({ queueResult: 0 });
    controller.request(request(['D:\\a']));
    await flush();
    expect(deps.onNothingFound).toHaveBeenCalled();
    expect(deps.start).not.toHaveBeenCalled();
  });

  it('keeps tracking while its rows have not shown up yet', async () => {
    const { controller } = harness();
    controller.request(request(['D:\\a']));
    await flush();
    controller.onJobsSnapshot([]);
    expect(controller.isActive()).toBe(true);
  });

  it('stops tracking quietly when every row is removed', async () => {
    const { deps, controller } = harness();
    controller.request(request(['D:\\a']));
    await flush();
    controller.onJobsSnapshot([job('0', 'D:\\a', 'Queued')]);
    controller.onJobsSnapshot([]);
    expect(deps.onDone).not.toHaveBeenCalled();
    expect(controller.isActive()).toBe(false);
  });

  it('owns the finished batch until one of its rows is reset', async () => {
    const { controller } = harness({ queueResult: 1 });
    controller.request(request(['D:\\a']));
    await flush();
    const done = [job('0', 'D:\\a', 'Finished')];
    controller.onJobsSnapshot(done);
    expect(controller.ownsJobs(done)).toBe(true);
    expect(controller.ownsJobs([...done, job('1', 'D:\\other', 'Finished')])).toBe(false);

    controller.onJobsSnapshot([job('0', 'D:\\a', 'Queued')]);
    expect(controller.ownsJobs(done)).toBe(false);
  });
});
