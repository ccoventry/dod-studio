import { describe, it, expect, vi, afterEach } from 'vitest';
import {
  rowsOf, batchFrames, secondsLeft, renderingLine, scanEndLine, createRenderBatch,
} from './render_batch_progress.js';
import { batchProgress } from './render_jobs.js';

afterEach(() => vi.useRealTimers());

const row = (id, status, frames = 100, progress = 0) => ({ id, status, frames, progress });

describe('the batch counts only its own rows (#954)', () => {
  it('leaves out rows an earlier batch finished', () => {
    const jobs = [row('a', 'Finished', 100, 100), row('b', 'Finished', 100, 100), row('c', 'Queued'), row('d', 'Rendering', 100, 0)];
    const rows = rowsOf(jobs, new Set(['c', 'd']));
    expect(rows.map((j) => j.id)).toEqual(['c', 'd']);
    // The whole table would read 50%; this batch has done nothing yet.
    expect(batchProgress(jobs)).toBe(50);
    expect(batchProgress(rows)).toBe(0);
    expect(renderingLine(rows, null)).toBe('Rendering 1 of 2');
  });

  it('counts frames done and left, without cancelled rows', () => {
    const rows = [row('a', 'Finished', 200, 100), row('b', 'Rendering', 100, 40), row('c', 'Queued', 300), row('d', 'Cancelled', 900), row('e', 'Error', 50)];
    expect(batchFrames(rows)).toEqual({ done: 290, left: 360 });
    expect(renderingLine(rows, null)).toBe('Rendering 3 of 4');
  });

  it('works out the time left at the pace so far, after the first seconds', () => {
    expect(secondsLeft({ done: 100, left: 400 }, 4000)).toBe(null);
    expect(secondsLeft({ done: 0, left: 400 }, 60000)).toBe(null);
    // 100 frames in 10 s: 400 more take 40 s.
    expect(secondsLeft({ done: 100, left: 400 }, 10000)).toBe(40);
    expect(renderingLine([row('a', 'Finished'), row('b', 'Rendering')], 240)).toBe('Rendering 2 of 2 · about 4 min left');
  });
});

describe('scanEndLine', () => {
  it('says how many takes it found, how many are new, and how long it took', () => {
    expect(scanEndLine({ total: 14, added: 3 }, 2100)).toBe('Found 14 takes (3 new) in 2.1 s');
    expect(scanEndLine({ total: 14, added: 0 }, 2100)).toBe('Found 14 takes (none new) in 2.1 s');
    expect(scanEndLine({ total: 1, added: 1, refreshed: 0 }, 300)).toBe('Found 1 take (1 new) in 0.3 s');
    expect(scanEndLine({ total: 4, added: 1, refreshed: 2, gone: 1, gone_queued: 1 }, 1000))
      .toBe('Found 4 takes (1 new, 2 refreshed, 1 gone (1 was queued)) in 1.0 s');
    expect(scanEndLine({ total: 0 }, 800)).toBe('Found no takes in 0.8 s');
  });
});

describe('createRenderBatch', () => {
  it('starts on its first row, keeps its rows after it ends, and starts the next one empty', () => {
    vi.useFakeTimers();
    let t = 1_000;
    const log = vi.fn();
    const batch = createRenderBatch({ log, now: () => t });
    expect(batch.running()).toBe(false);
    expect(batch.shownIds().size).toBe(0);

    batch.add(['a', 'b']);
    batch.add(['b']);
    expect(batch.running()).toBe(true);
    expect([...batch.ids()]).toEqual(['a', 'b']);
    expect(log).toHaveBeenCalledWith('[render] started');
    expect(log).toHaveBeenCalledTimes(1);

    t = 461_000;
    const ended = batch.finish([row('a', 'Finished'), row('b', 'Error'), row('z', 'Finished')]);
    expect(ended).toMatchObject({ ms: 460_000, finished: 1, failed: 1, cancelled: 0 });
    expect(log).toHaveBeenLastCalledWith('[render] done after 460.0 s: 1 rendered, 1 failed, 0 cancelled');
    // The bar still shows how it ended.
    expect([...batch.shownIds()]).toEqual(['a', 'b']);

    // The next batch starts from its own rows only.
    batch.add(['z2']);
    expect([...batch.shownIds()]).toEqual(['z2']);

    // A refused Start leaves no batch running and logs no end.
    const calls = log.mock.calls.length;
    batch.discard();
    expect(batch.running()).toBe(false);
    expect(log.mock.calls.length).toBe(calls);
  });
});
