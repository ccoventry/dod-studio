import { describe, it, expect, vi } from 'vitest';
import { previewKey, frameLabel, passProgress, createTakePreview, PREVIEW_STATE } from './take_preview.js';

const job = (over = {}) => ({ id: '0', take_folder: 'D:/caps/s/chain_01_b0/take0000', stream_folder: 'all', ...over });

/** Resolves once every queued promise callback has run. */
const settle = () => new Promise((r) => setTimeout(r, 0));

describe('take pictures (#1069)', () => {
  it('keys one picture per take stream, and none without a stream', () => {
    expect(previewKey(job())).toBe('D:/caps/s/chain_01_b0/take0000|all');
    expect(previewKey(job({ stream_folder: '' }))).toBe('');
    expect(previewKey(job({ take_folder: '' }))).toBe('');
  });

  it('captions a frame with what is known of it', () => {
    expect(frameLabel({ secs: 4.5, frame: 1350 })).toBe('4.5 s · frame 1350');
    expect(frameLabel({ secs: null, frame: 12 })).toBe('frame 12');
    expect(frameLabel({})).toBe('');
  });

  it('counts the take being made by its frames, and hides once nothing is left', () => {
    const states = new Map([
      ['a', { status: PREVIEW_STATE.DONE }],
      ['b', { status: PREVIEW_STATE.MAKING }],
      ['c', { status: PREVIEW_STATE.WAITING }],
      ['d', { status: PREVIEW_STATE.FAILED }],
    ]);
    expect(passProgress(states, { done: 3, total: 6 })).toEqual({ done: 2, total: 4, pct: 63 });
    states.get('b').status = PREVIEW_STATE.DONE;
    states.get('c').status = PREVIEW_STATE.DONE;
    expect(passProgress(states)).toBeNull();
  });

  it('makes each take once, one at a time, a HUD pair of rows once per stream', async () => {
    const calls = [];
    const thumb = vi.fn(async (take, stream) => {
      calls.push(`${take}|${stream}`);
      return { uri: `data:${stream}` };
    });
    const preview = createTakePreview({ thumb, sheet: vi.fn(), cancel: vi.fn(async () => {}), getFps: () => 300, onChange: () => {} });
    preview.sync([job(), job({ id: '1' }), job({ id: '2', stream_folder: 'hudColor' })]);
    preview.sync([job(), job({ id: '1' }), job({ id: '2', stream_folder: 'hudColor' })]);
    await settle();
    expect(calls).toEqual(['D:/caps/s/chain_01_b0/take0000|all', 'D:/caps/s/chain_01_b0/take0000|hudColor']);
    expect(thumb).toHaveBeenCalledWith('D:/caps/s/chain_01_b0/take0000', 'all', 300);
    expect(preview.thumbFor(previewKey(job()))).toBe('data:all');
    expect(preview.progress()).toBeNull();
  });

  it('a pause cancels the take being made and makes it again on resume', async () => {
    let fail = true;
    const cancel = vi.fn(async () => {});
    const thumb = vi.fn(async () => {
      if (fail) {
        fail = false;
        throw 'cancelled';
      }
      return { uri: 'data:x' };
    });
    const preview = createTakePreview({ thumb, sheet: vi.fn(), cancel, getFps: () => null, onChange: () => {} });
    preview.sync([job()]);
    preview.pause();
    expect(cancel).toHaveBeenCalledTimes(1);
    await settle();
    expect(preview.thumbFor(previewKey(job()))).toBe('');
    preview.resume();
    await settle();
    expect(thumb).toHaveBeenCalledTimes(2);
    expect(preview.thumbFor(previewKey(job()))).toBe('data:x');
  });

  it('a take FFmpeg fails on says why, and the pass goes on', async () => {
    const thumb = vi.fn(async (take) => {
      if (take === 'bad') throw 'FFmpeg made no picture';
      return { uri: 'data:ok' };
    });
    const preview = createTakePreview({ thumb, sheet: vi.fn(), cancel: vi.fn(async () => {}), getFps: () => null, onChange: () => {} });
    preview.sync([job({ take_folder: 'bad' }), job({ id: '1', take_folder: 'good' })]);
    await settle();
    await settle();
    expect(preview.stateFor(previewKey(job({ take_folder: 'bad' })))).toMatchObject({
      status: PREVIEW_STATE.FAILED,
      error: 'FFmpeg made no picture',
    });
    expect(preview.thumbFor(previewKey(job({ take_folder: 'good' })))).toBe('data:ok');
  });
});
