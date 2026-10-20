import { describe, it, expect, vi, beforeEach } from 'vitest';

// The themed dialog, stood in for: a question answers with the next of
// `answers`; a wait (no confirm button) stays open until the code dismisses
// it, or answers Keep Studio open at once with `keepOpenDuringWait`.
const dialog = vi.hoisted(() => ({ answers: [], calls: [], pending: null, keepOpenDuringWait: false }));
vi.mock('./themed_confirm.js', () => ({
  themedConfirm: vi.fn((message, opts) => new Promise((resolve) => {
    dialog.calls.push({ message, opts });
    if (opts.hideConfirm) {
      dialog.pending = resolve;
      if (dialog.keepOpenDuringWait) resolve(false);
      return;
    }
    resolve(dialog.answers.shift());
  })),
  dismissThemedConfirm: vi.fn((result) => {
    dialog.pending?.(result);
    dialog.pending = null;
  }),
  setThemedConfirmMessage: vi.fn(),
}));
vi.mock('./ipc_bridge.js', () => ({ waitForBatchStart: vi.fn(), cancelCaptureBatch: vi.fn() }));

import { batchCloseMessage, confirmCloseDuringBatch, START_WAIT_MS } from './batch_close_prompt.js';
import { STRINGS } from './strings.js';

const s = STRINGS.BATCH_CLOSE_MODAL;

/** A running batch whose start answers `states` in turn (the first for the
 *  look without waiting, the second after the wait). */
function batch(states, { cancelStops = true } = {}) {
  let running = true;
  const waitForStart = vi.fn(async () => {
    const next = states.shift();
    if (next instanceof Error) throw next;
    return next === undefined ? new Promise(() => {}) : next;
  });
  const cancelBatch = vi.fn(async () => { if (cancelStops) running = false; });
  return {
    waitForStart,
    cancelBatch,
    args: { isRunning: () => running, isLocalBuild: async () => false, waitForStart, cancelBatch },
  };
}

const titles = () => dialog.calls.map((c) => c.opts.title);

describe('closing Studio during a batch (#545)', () => {
  beforeEach(() => {
    dialog.answers = [];
    dialog.calls = [];
    dialog.pending = null;
    dialog.keepOpenDuringWait = false;
  });

  it('closes without asking when no batch is running', async () => {
    const b = batch([]);
    const ok = await confirmCloseDuringBatch({ ...b.args, isRunning: () => false });
    expect(ok).toBe(true);
    expect(dialog.calls).toEqual([]);
    expect(b.waitForStart).not.toHaveBeenCalled();
  });

  it('asks while a started batch runs, and Keep Studio open keeps it open', async () => {
    dialog.answers = [false];
    const b = batch(['sent']);
    expect(await confirmCloseDuringBatch(b.args)).toBe(false);
    expect(dialog.calls).toHaveLength(1);
    expect(dialog.calls[0].message).toBe(s.MESSAGE);
    expect(dialog.calls[0].opts.confirmLabel).toBe(s.CLOSE_BUTTON);
    expect(b.cancelBatch).not.toHaveBeenCalled();
  });

  it('a batch started from the launch line needs no wait either', async () => {
    dialog.answers = [true];
    const b = batch(['not_needed']);
    expect(await confirmCloseDuringBatch(b.args)).toBe(true);
    expect(titles()).toEqual([s.TITLE]);
    expect(b.waitForStart).toHaveBeenCalledTimes(1);
  });

  it('waits for a start still on its way, then asks as for a running batch', async () => {
    dialog.answers = [true];
    const b = batch(['pending', 'sent']);
    expect(await confirmCloseDuringBatch(b.args)).toBe(true);
    expect(b.waitForStart.mock.calls).toEqual([[0], [START_WAIT_MS]]);
    expect(titles()).toEqual([s.WAIT_TITLE, s.TITLE]);
    expect(dialog.calls[0].opts.hideConfirm).toBe(true);
    expect(dialog.calls[0].message).toBe(s.starting(START_WAIT_MS / 1000));
    expect(b.cancelBatch).not.toHaveBeenCalled();
  });

  it('Keep Studio open during the wait keeps it open and asks nothing more', async () => {
    dialog.keepOpenDuringWait = true;
    const b = batch(['pending']);
    expect(await confirmCloseDuringBatch(b.args)).toBe(false);
    expect(titles()).toEqual([s.WAIT_TITLE]);
    expect(b.cancelBatch).not.toHaveBeenCalled();
  });

  it('a start that never reaches the game: says so, and closing cancels the batch', async () => {
    dialog.answers = [true];
    const b = batch(['pending', 'pending']);
    expect(await confirmCloseDuringBatch(b.args)).toBe(true);
    expect(titles()).toEqual([s.WAIT_TITLE, s.NOT_STARTED_TITLE, s.WAIT_TITLE]);
    expect(dialog.calls[1].message).toBe(s.NOT_STARTED_MESSAGE);
    expect(dialog.calls[1].opts.confirmLabel).toBe(s.CANCEL_AND_CLOSE_BUTTON);
    expect(dialog.calls[2].message).toBe(s.CANCELLING);
    expect(dialog.calls[2].opts.hideCancel).toBe(true);
    expect(b.cancelBatch).toHaveBeenCalledTimes(1);
  });

  it('a batch still being prepared has not started: no wait, same question', async () => {
    dialog.answers = [false];
    const b = batch(['preparing']);
    expect(await confirmCloseDuringBatch(b.args)).toBe(false);
    expect(titles()).toEqual([s.NOT_STARTED_TITLE]);
    expect(b.cancelBatch).not.toHaveBeenCalled();
  });

  it('a batch that ends during the wait closes without asking', async () => {
    const b = batch(['pending', 'not_running']);
    expect(await confirmCloseDuringBatch(b.args)).toBe(true);
    expect(titles()).toEqual([s.WAIT_TITLE]);
  });

  it('when Studio cannot tell, it asks as before and cancels nothing', async () => {
    dialog.answers = [true];
    const b = batch([new Error('ipc down')]);
    expect(await confirmCloseDuringBatch(b.args)).toBe(true);
    expect(titles()).toEqual([s.TITLE]);
    expect(b.cancelBatch).not.toHaveBeenCalled();
  });

  it('a second close during the wait opens nothing, and the first still asks', async () => {
    dialog.answers = [true];
    let sendStart;
    const b = batch(['pending']);
    b.waitForStart.mockImplementationOnce(async () => 'pending');
    b.waitForStart.mockImplementationOnce(() => new Promise((resolve) => { sendStart = resolve; }));
    const first = confirmCloseDuringBatch(b.args);
    await vi.waitFor(() => expect(titles()).toEqual([s.WAIT_TITLE]));
    expect(await confirmCloseDuringBatch(b.args)).toBe(false);
    expect(titles()).toEqual([s.WAIT_TITLE]);
    sendStart('sent');
    expect(await first).toBe(true);
    expect(titles()).toEqual([s.WAIT_TITLE, s.TITLE]);
    // Once answered, a later close asks again.
    dialog.answers = [false];
    b.waitForStart.mockImplementationOnce(async () => 'sent');
    expect(await confirmCloseDuringBatch(b.args)).toBe(false);
    expect(titles()).toEqual([s.WAIT_TITLE, s.TITLE, s.TITLE]);
  });

  it('warns a local build that closing takes the game with it', () => {
    expect(batchCloseMessage(true)).toContain(s.LOCAL_BUILD_NOTE);
    expect(batchCloseMessage(false)).not.toContain(s.LOCAL_BUILD_NOTE);
  });
});
