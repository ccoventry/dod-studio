import { describe, it, expect, vi } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('./themed_confirm.js', () => ({ themedConfirm: vi.fn() }));
vi.mock('./toast.js', () => ({ showToast: vi.fn() }));

const {
  AFTER_FINISH, normalizeChoice, whenNeededAvailable, makesRoomBeforeCapture, settingHint, needsConfirm,
  progressText, doneToast, createTakeCleanupSetting, createTakeCleanupReports,
} = await import('./take_cleanup.js');
const { STRINGS } = await import('./strings.js');

const gb = (bytes) => `${(bytes / 1073741824).toFixed(1)} GB`;

describe('the setting (#644)', () => {
  it('reads anything unknown as Keep, like the backend', () => {
    expect(normalizeChoice('recycle')).toBe(AFTER_FINISH.RECYCLE);
    expect(normalizeChoice('delete')).toBe(AFTER_FINISH.DELETE);
    expect(normalizeChoice(undefined)).toBe(AFTER_FINISH.KEEP);
    expect(normalizeChoice('shred')).toBe(AFTER_FINISH.KEEP);
  });

  it('offers "Only when space is needed" only with permanent deletion', () => {
    expect(whenNeededAvailable(AFTER_FINISH.DELETE)).toBe(true);
    expect(whenNeededAvailable(AFTER_FINISH.RECYCLE)).toBe(false);
    expect(whenNeededAvailable(AFTER_FINISH.KEEP)).toBe(false);
  });

  it('lets a capture short of room start only when finished takes may be deleted to make room', () => {
    expect(makesRoomBeforeCapture(AFTER_FINISH.DELETE, true)).toBe(true);
    expect(makesRoomBeforeCapture(AFTER_FINISH.DELETE, false)).toBe(false);
    expect(makesRoomBeforeCapture(AFTER_FINISH.RECYCLE, true)).toBe(false);
    expect(makesRoomBeforeCapture(AFTER_FINISH.KEEP, true)).toBe(false);
  });

  it('says what each choice does, and that the Recycle Bin frees no space until emptied', () => {
    expect(settingHint(AFTER_FINISH.KEEP, true)).toBe(STRINGS.TAKE_CLEANUP.HINT_KEEP);
    expect(settingHint(AFTER_FINISH.RECYCLE, true)).toContain('frees no space until you empty it');
    expect(settingHint(AFTER_FINISH.DELETE, false)).toContain('cannot be restored');
    expect(settingHint(AFTER_FINISH.DELETE, true)).toBe(STRINGS.TAKE_CLEANUP.HINT_DELETE_WHEN_NEEDED);
  });

  it('asks only when permanent deletion is turned on', () => {
    expect(needsConfirm(AFTER_FINISH.KEEP, AFTER_FINISH.DELETE)).toBe(true);
    expect(needsConfirm(AFTER_FINISH.RECYCLE, AFTER_FINISH.DELETE)).toBe(true);
    expect(needsConfirm(AFTER_FINISH.DELETE, AFTER_FINISH.DELETE)).toBe(false);
    expect(needsConfirm(AFTER_FINISH.DELETE, AFTER_FINISH.KEEP)).toBe(false);
    expect(needsConfirm(AFTER_FINISH.KEEP, AFTER_FINISH.RECYCLE)).toBe(false);
  });
});

describe('createTakeCleanupSetting', () => {
  function harness(confirmAnswer) {
    const painted = [];
    const confirm = vi.fn(() => Promise.resolve(confirmAnswer));
    const onChange = vi.fn();
    const setting = createTakeCleanupSetting({ confirm, render: (s) => painted.push(s), onChange });
    return { setting, painted, confirm, onChange, last: () => painted[painted.length - 1] };
  }

  it('loads a saved permanent choice without asking or saving', () => {
    const h = harness(false);
    h.setting.set('delete', true);
    expect(h.confirm).not.toHaveBeenCalled();
    expect(h.onChange).not.toHaveBeenCalled();
    expect(h.last()).toMatchObject({ choice: 'delete', whenNeeded: true, whenNeededEnabled: true });
  });

  it('puts the old choice back when the confirm is cancelled', async () => {
    const h = harness(false);
    h.setting.set('recycle', false);
    await h.setting.choose('delete');
    expect(h.confirm).toHaveBeenCalledTimes(1);
    expect(h.setting.get().choice).toBe('recycle');
    expect(h.last().choice).toBe('recycle');
    expect(h.onChange).not.toHaveBeenCalled();
  });

  it('keeps and saves the permanent choice once confirmed', async () => {
    const h = harness(true);
    h.setting.set('keep', false);
    await h.setting.choose('delete');
    expect(h.setting.get().choice).toBe('delete');
    expect(h.last()).toMatchObject({ whenNeededEnabled: true, hint: STRINGS.TAKE_CLEANUP.HINT_DELETE });
    expect(h.onChange).toHaveBeenCalledTimes(1);
  });

  it('saves the other choices without asking', async () => {
    const h = harness(false);
    h.setting.set('delete', true);
    await h.setting.choose('recycle');
    expect(h.confirm).not.toHaveBeenCalled();
    expect(h.last()).toMatchObject({ choice: 'recycle', whenNeededEnabled: false });
    h.setting.setWhenNeeded(false);
    expect(h.onChange).toHaveBeenCalledTimes(2);
  });
});

describe('the reports', () => {
  it('names the step under the bar', () => {
    expect(progressText({ step: 'looking', done: 0, total: 0 })).toBe('Making room: looking for finished takes');
    expect(progressText({ step: 'measuring', done: 2, total: 5 })).toBe('Making room: measuring finished takes (2 of 5)');
    expect(progressText({ step: 'deleting', done: 1, total: 3 })).toBe('Deleting finished takes (1 of 3)');
    expect(progressText({ step: 'recycling', done: 1, total: 3 })).toBe('Moving finished takes to the Recycle Bin (1 of 3)');
  });

  const take = (size) => ({ take_folder: 'D:\\cap\\s\\b\\take0000', take_key: 's/b', size_bytes: size });

  it('says how many went, how much space, where and why', () => {
    const twoGb = 2 * 1073741824;
    expect(doneToast({ reason: 'finished', method: 'recycle', deleted: [take(twoGb)], failed: [] }, gb))
      .toEqual({ message: 'Moved a finished take to the Recycle Bin (2.0 GB).', level: 'success' });
    expect(doneToast({ reason: 'finished', method: 'delete', deleted: [take(twoGb), take(twoGb)], failed: [] }, gb).message)
      .toBe('Deleted 2 finished takes for good (4.0 GB).');
    expect(doneToast({ reason: 'capture', method: 'delete', deleted: [take(twoGb)], failed: [] }, gb).message)
      .toBe('Deleted a finished take (2.0 GB) to make room for the capture.');
    expect(doneToast({ reason: 'finish', method: 'delete', deleted: [take(twoGb)], failed: [] }, gb).message)
      .toBe('Deleted a finished take (2.0 GB) to make room for the renders.');
  });

  it('warns when even every finished take is not enough, and errors on a failure', () => {
    const short = doneToast({
      reason: 'capture', method: 'delete', deleted: [], failed: [],
      short: { needed_bytes: 40 * 1073741824, available_bytes: 1073741824 },
    }, gb);
    expect(short.level).toBe('warning');
    expect(short.message).toContain('The capture needs 40.0 GB free on one drive');
    expect(short.message).toContain('at most 1.0 GB on any one drive');
    expect(short.message).toContain('No takes were deleted');
    const failed = doneToast({ reason: 'finished', method: 'delete', deleted: [], failed: [{ take_folder: 'x', error: 'in use' }] }, gb);
    expect(failed.level).toBe('error');
    expect(failed.message).toContain('in use');
  });

  it('says nothing when nothing went', () => {
    expect(doneToast({ reason: 'finished', method: 'delete', deleted: [], failed: [] }, gb)).toBeNull();
  });

  it('paints progress, then hides the bar and toasts at the end', () => {
    const handlers = {};
    const showProgress = vi.fn();
    const toast = vi.fn();
    createTakeCleanupReports({
      listen: (name, handler) => { handlers[name] = handler; },
      showProgress, toast, formatSize: gb,
    });
    handlers.take_cleanup_progress({ payload: { step: 'deleting', done: 1, total: 2 } });
    expect(showProgress).toHaveBeenLastCalledWith({ step: 'deleting', done: 1, total: 2 });
    handlers.take_cleanup_done({ payload: { reason: 'finished', method: 'delete', deleted: [take(1073741824)], failed: [] } });
    expect(showProgress).toHaveBeenLastCalledWith(null);
    expect(toast).toHaveBeenCalledWith('Deleted a finished take for good (1.0 GB).', 'success');
  });
});
