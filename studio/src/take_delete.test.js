import { describe, it, expect, vi } from 'vitest';
import {
  renderingTakeFolders,
  queuedJobCount,
  uniqueTakeFolders,
  totalBytes,
  confirmContent,
  resultToast,
  progressLine,
  createTakeDelete,
  DELETE_STEP,
} from './take_delete.js';

const fmt = (bytes) => `${bytes} B`;
const TAKE_A = 'D:/captures/session_1/chain_01_b0/take0000';
const TAKE_B = 'D:/captures/session_1/chain_02_b0/take0000';

describe('which takes are busy (#644)', () => {
  it('a take with either half of a HUD pair rendering is busy; a queued one is not', () => {
    const busy = renderingTakeFolders([
      { take_folder: TAKE_A, status: 'Finished' },
      { take_folder: TAKE_A, status: 'Rendering' },
      { take_folder: TAKE_B, status: 'Error' },
    ]);
    expect([...busy]).toEqual([TAKE_A]);
    expect(renderingTakeFolders([{ take_folder: TAKE_B, status: 'Queued' }]).has(TAKE_B)).toBe(false);
    expect(renderingTakeFolders(null).size).toBe(0);
  });

  it('counts the queued renders a delete pulls from the queue', () => {
    const jobs = [
      { take_folder: TAKE_A, status: 'Queued' },
      { take_folder: TAKE_A, status: 'Queued', stream: 'HUD ONLY' },
      { take_folder: TAKE_B, status: 'Queued' },
      { take_folder: TAKE_B, status: 'Finished' },
    ];
    expect(queuedJobCount(jobs, [TAKE_A])).toBe(2);
    expect(queuedJobCount(jobs, [TAKE_A, TAKE_B])).toBe(3);
    expect(queuedJobCount(null, [TAKE_A])).toBe(0);
  });

  it('two rows of one take are one take', () => {
    expect(uniqueTakeFolders([TAKE_A, TAKE_B, TAKE_A, ''])).toEqual([TAKE_A, TAKE_B]);
  });
});

describe('the confirm', () => {
  const sizes = [
    { take_folder: TAKE_A, take_key: 'session_1/chain_01_b0', size_bytes: 100 },
    { take_folder: TAKE_B, take_key: 'session_1/chain_02_b0', size_bytes: 50 },
  ];

  it('names every take folder and the total size', () => {
    expect(totalBytes(sizes)).toBe(150);
    const c = confirmContent(sizes, fmt);
    expect(c.title).toBe('Delete 2 Takes');
    expect(c.message).toBe('Move these 2 takes (150 B) to the Recycle Bin?');
    expect(c.details.map((d) => d.primary)).toEqual(['session_1/chain_01_b0', 'session_1/chain_02_b0']);
    expect(c.details[0].secondary).toBe(`100 B · ${TAKE_A}`);
    expect(c.confirmLabel).toBe('Move to Recycle Bin');
  });

  it('is singular for one take', () => {
    const c = confirmContent(sizes.slice(0, 1), fmt);
    expect(c.title).toBe('Delete Take');
    expect(c.message).toBe('Move this take (100 B) to the Recycle Bin?');
    expect(c.footer).toBe('The space comes back when the Recycle Bin is emptied. The render history stays.');
  });

  it('says when queued renders leave the queue', () => {
    expect(confirmContent(sizes.slice(0, 1), fmt, 1).footer)
      .toBe('Its queued render is removed from the queue. The space comes back when the Recycle Bin is emptied. The render history stays.');
    expect(confirmContent(sizes, fmt, 3).footer.startsWith('3 queued renders are removed from the queue. ')).toBe(true);
  });
});

describe('the result', () => {
  it('reports success with the size moved', () => {
    const t = resultToast({ deleted: [{ size_bytes: 7 }], failed: [] }, fmt);
    expect(t).toEqual({ message: 'Take moved to the Recycle Bin (7 B).', level: 'success' });
  });

  it('reports a failure with the first error', () => {
    const t = resultToast({ deleted: [{ size_bytes: 7 }], failed: [{ take_folder: TAKE_B, error: 'locked' }] }, fmt);
    expect(t.level).toBe('error');
    expect(t.message).toBe('1 take not deleted (1 moved to the Recycle Bin): locked. Scan for Takes to list it again.');
  });

  it('says which step is running', () => {
    expect(progressLine(DELETE_STEP.MEASURING, 0, 2)).toBe('Measuring takes: 0 of 2');
    expect(progressLine(DELETE_STEP.RECYCLING, 1, 2)).toBe('Moving to the Recycle Bin: 1 of 2');
  });
});

describe('the click flow', () => {
  function setup({ confirmed = true, measureError = null } = {}) {
    const calls = { progress: [], busy: [], toasts: [] };
    const deps = {
      measure: vi.fn(async (folders) => {
        if (measureError) throw measureError;
        return folders.map((f) => ({ take_folder: f, take_key: 'k', size_bytes: 10 }));
      }),
      remove: vi.fn(async (folders) => ({ deleted: folders.map((f) => ({ take_folder: f, size_bytes: 10 })), failed: [] })),
      confirm: vi.fn(async () => confirmed),
      showToast: (message, level) => calls.toasts.push({ message, level }),
      formatSize: fmt,
      showProgress: (p) => calls.progress.push(p),
      onBusyChange: (b) => calls.busy.push(b),
      getJobs: () => [{ take_folder: TAKE_A, status: 'Queued' }],
    };
    return { deps, calls, flow: createTakeDelete(deps) };
  }

  it('measures, confirms, then moves each take once', async () => {
    const { deps, calls, flow } = setup();
    await flow.deleteTakes([TAKE_A, TAKE_A]);
    expect(deps.measure).toHaveBeenCalledWith([TAKE_A]);
    expect(deps.confirm).toHaveBeenCalledTimes(1);
    expect(deps.confirm.mock.calls[0][1].footer.startsWith('Its queued render is removed from the queue. ')).toBe(true);
    expect(deps.remove).toHaveBeenCalledWith([TAKE_A]);
    expect(calls.toasts).toEqual([{ message: 'Take moved to the Recycle Bin (10 B).', level: 'success' }]);
    expect(calls.busy).toEqual([true, false]);
    expect(calls.progress[0]).toEqual({ step: 'measuring', done: 0, total: 1 });
    expect(calls.progress).toContainEqual({ step: 'recycling', done: 0, total: 1 });
    expect(calls.progress.at(-1)).toBeNull();
    expect(flow.isRunning()).toBe(false);
  });

  it('moves nothing when the confirm is cancelled', async () => {
    const { deps, calls, flow } = setup({ confirmed: false });
    await flow.deleteTakes([TAKE_A]);
    expect(deps.remove).not.toHaveBeenCalled();
    expect(calls.toasts).toEqual([]);
    expect(calls.busy).toEqual([true, false]);
  });

  it('shows the backend refusal and never asks', async () => {
    const { deps, calls, flow } = setup({ measureError: 'it is rendering' });
    await flow.deleteTakes([TAKE_A]);
    expect(deps.confirm).not.toHaveBeenCalled();
    expect(deps.remove).not.toHaveBeenCalled();
    expect(calls.toasts[0]).toEqual({ message: "Couldn't delete the take: it is rendering", level: 'error' });
  });

  it('ignores a second click while one is running', async () => {
    const { deps, flow } = setup();
    let release;
    deps.confirm.mockImplementationOnce(() => new Promise((r) => { release = r; }));
    const first = flow.deleteTakes([TAKE_A]);
    await vi.waitFor(() => expect(deps.confirm).toHaveBeenCalled());
    await flow.deleteTakes([TAKE_B]);
    expect(deps.measure).toHaveBeenCalledTimes(1);
    release(true);
    await first;
    expect(deps.remove).toHaveBeenCalledWith([TAKE_A]);
  });
});
