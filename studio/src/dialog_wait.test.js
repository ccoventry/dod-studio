import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ fileDialogShown: vi.fn() }));

import { createDialogWait } from './dialog_wait.js';

function setup(shownAnswers) {
  const answers = [...shownAnswers];
  const isShown = vi.fn(async () => (answers.length > 1 ? answers.shift() : answers[0]));
  const setBusy = vi.fn();
  const setNotice = vi.fn();
  const wait = createDialogWait({ isShown, setBusy, setNotice, delayMs: 500, pollMs: 100 });
  return { wait, isShown, setBusy, setNotice };
}

describe('dialog wait', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('shows the busy cursor from the start and never the notice for a quick dialog', async () => {
    const { wait, isShown, setBusy, setNotice } = setup([true]);
    wait.set(true);
    expect(setBusy).toHaveBeenLastCalledWith(true);
    await vi.advanceTimersByTimeAsync(300);
    wait.set(false);
    await vi.advanceTimersByTimeAsync(1000);
    expect(isShown).not.toHaveBeenCalled();
    expect(setNotice).not.toHaveBeenCalledWith(true);
    expect(setBusy).toHaveBeenLastCalledWith(false);
  });

  it('shows no notice when the dialog is already up at the delay', async () => {
    const { wait, isShown, setNotice } = setup([true]);
    wait.set(true);
    await vi.advanceTimersByTimeAsync(1000);
    expect(isShown).toHaveBeenCalledTimes(1);
    expect(setNotice).not.toHaveBeenCalledWith(true);
    wait.set(false);
  });

  it('shows the notice while a slow dialog is still coming, and hides it once it appears', async () => {
    const { wait, isShown, setNotice } = setup([false, false, false, true]);
    wait.set(true);
    await vi.advanceTimersByTimeAsync(500);
    expect(setNotice).toHaveBeenLastCalledWith(true);
    await vi.advanceTimersByTimeAsync(300);
    expect(setNotice).toHaveBeenLastCalledWith(false);
    const checks = isShown.mock.calls.length;
    await vi.advanceTimersByTimeAsync(1000);
    expect(isShown).toHaveBeenCalledTimes(checks); // stops checking once it's up
    wait.set(false);
  });

  it('hides the notice and stops checking when the dialog returns before appearing', async () => {
    const { wait, isShown, setNotice, setBusy } = setup([false]);
    wait.set(true);
    await vi.advanceTimersByTimeAsync(700);
    expect(setNotice).toHaveBeenLastCalledWith(true);
    wait.set(false);
    expect(setNotice).toHaveBeenLastCalledWith(false);
    expect(setBusy).toHaveBeenLastCalledWith(false);
    const checks = isShown.mock.calls.length;
    await vi.advanceTimersByTimeAsync(1000);
    expect(isShown).toHaveBeenCalledTimes(checks);
  });

  it('treats a failing check as shown, so the notice never sticks', async () => {
    const setNotice = vi.fn();
    const wait = createDialogWait({
      isShown: async () => { throw new Error('ipc'); },
      setBusy: () => {}, setNotice, delayMs: 500, pollMs: 100,
    });
    wait.set(true);
    await vi.advanceTimersByTimeAsync(1000);
    expect(setNotice).not.toHaveBeenCalledWith(true);
    wait.set(false);
  });

  it('around() covers the whole async work and ends the wait when it throws', async () => {
    const { wait, setBusy } = setup([true]);
    const work = wait.around(async () => {
      expect(setBusy).toHaveBeenLastCalledWith(true);
      throw new Error('cancelled');
    });
    await expect(work).rejects.toThrow('cancelled');
    expect(setBusy).toHaveBeenLastCalledWith(false);
  });

  it('overlapping waits share one cursor, cleared when the last ends', async () => {
    const { wait, setBusy } = setup([true]);
    wait.set(true);
    wait.set(true);
    wait.set(false);
    expect(setBusy).toHaveBeenCalledTimes(1);
    wait.set(false);
    expect(setBusy).toHaveBeenLastCalledWith(false);
    wait.set(false); // an extra end is ignored
    expect(setBusy).toHaveBeenCalledTimes(2);
  });
});
