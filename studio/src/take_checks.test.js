import { describe, it, expect, vi } from 'vitest';
import { findingLine, findingLines, checkChip, checksProgress, createTakeChecks, CHECK_STATE } from './take_checks.js';

const job = (over = {}) => ({ id: '0', take_folder: 'D:/caps/s/chain_01_b0/take0000', stream_folder: 'all', ...over });

/** Resolves once every queued promise callback has run. */
const settle = () => new Promise((r) => setTimeout(r, 0));

const HOLDS = { check: 'holds', cause: 'demo', warn: false, at_secs: 7.43, secs: 0.34, count: 9 };
const BLACK = { check: 'black', cause: 'capture', warn: true, at_secs: 2, secs: 3.5 };

describe('take checks (#1084)', () => {
  it('says what each finding is, when, and whether capturing again helps', () => {
    expect(findingLine(BLACK)).toEqual({
      text: 'Black picture for 3.5 s from 2.0 s.',
      cause: 'Capturing again helps',
      warn: true,
    });
    const holds = findingLine(HOLDS);
    expect(holds.text).toContain('9 times (longest 0.3 s at 7.4 s)');
    expect(holds.cause).toContain('The demo itself');
    expect(holds.warn).toBe(false);
    expect(findingLine({ check: 'clipping', cause: 'capture', warn: true, share: 0.29 }).text).toContain('29.0%');
    expect(findingLine({ check: 'something_new' })).toBeNull();
  });

  it('lists warnings before notes', () => {
    const lines = findingLines({ findings: [HOLDS, BLACK] });
    expect(lines.map((l) => l.warn)).toEqual([true, false]);
  });

  it('shows OK for a take with only notes, and the warning count otherwise', () => {
    const ok = checkChip({ status: CHECK_STATE.DONE, result: { findings: [HOLDS] } });
    expect(ok.label).toBe('OK');
    expect(ok.title).toContain('holds still briefly');
    const clean = checkChip({ status: CHECK_STATE.DONE, result: { findings: [] } });
    expect(clean.title).toBe('Picture and sound look good.');
    const bad = checkChip({ status: CHECK_STATE.DONE, result: { findings: [BLACK, { check: 'no_sound', cause: 'capture', warn: true }, HOLDS] } });
    expect(bad.label).toBe('⚠ 2');
    expect(bad.cls).toBe('rj-check-warn');
    expect(bad.title.split('\n')).toHaveLength(3);
    expect(checkChip({ status: CHECK_STATE.FAILED, error: 'no FFmpeg' }).title).toBe("Couldn't check this take: no FFmpeg");
    // The backend gave nothing back: nothing to show.
    expect(checkChip({ status: CHECK_STATE.DONE, result: null })).toBeNull();
    expect(checkChip(null)).toBeNull();
  });

  it('counts the take being checked by how far through it is', () => {
    const states = new Map([
      ['a', { status: CHECK_STATE.DONE }],
      ['b', { status: CHECK_STATE.CHECKING }],
    ]);
    expect(checksProgress(states, { done: 5, total: 10 })).toEqual({ done: 1, total: 2, pct: 75 });
    states.get('b').status = CHECK_STATE.FAILED;
    expect(checksProgress(states)).toBeNull();
  });

  it('checks each take stream once, in turn, and waits while blocked', async () => {
    let blocked = true;
    const check = vi.fn(async () => ({ findings: [] }));
    const onChange = vi.fn();
    const checks = createTakeChecks({ check, cancel: vi.fn(async () => {}), getFps: () => 300, isBlocked: () => blocked, onChange });
    checks.sync([job(), job({ id: '1' }), job({ id: '2', stream_folder: 'hudColor' })]);
    await settle();
    expect(check).not.toHaveBeenCalled();
    blocked = false;
    checks.run();
    await settle();
    await settle();
    expect(check.mock.calls).toEqual([
      ['D:/caps/s/chain_01_b0/take0000', 'all', 300],
      ['D:/caps/s/chain_01_b0/take0000', 'hudColor', 300],
    ]);
    expect(checks.stateFor('D:/caps/s/chain_01_b0/take0000|all').status).toBe(CHECK_STATE.DONE);
    expect(checks.progress()).toBeNull();
  });

  it('a pause cancels the running check and it is checked again on resume', async () => {
    let release;
    const check = vi.fn()
      .mockImplementationOnce(() => new Promise((_, reject) => { release = () => reject('cancelled'); }))
      .mockImplementation(async () => ({ findings: [BLACK] }));
    const cancel = vi.fn(async () => { release(); });
    const checks = createTakeChecks({ check, cancel, getFps: () => null, onChange: () => {} });
    checks.sync([job()]);
    await settle();
    checks.pause();
    await settle();
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(checks.stateFor('D:/caps/s/chain_01_b0/take0000|all').status).toBe(CHECK_STATE.WAITING);
    checks.resume();
    await settle();
    await settle();
    expect(check).toHaveBeenCalledTimes(2);
    expect(checks.stateFor('D:/caps/s/chain_01_b0/take0000|all').result.findings).toEqual([BLACK]);
  });

  it('a check that fails says why, and the pass goes on', async () => {
    const check = vi.fn()
      .mockImplementationOnce(async () => { throw 'FFmpeg failed'; })
      .mockImplementation(async () => ({ findings: [] }));
    const checks = createTakeChecks({ check, cancel: vi.fn(), getFps: () => null, onChange: () => {} });
    checks.sync([job(), job({ id: '1', take_folder: 'D:/caps/s/chain_02_b0/take0000' })]);
    await settle();
    await settle();
    const failed = checks.stateFor('D:/caps/s/chain_01_b0/take0000|all');
    expect(failed.status).toBe(CHECK_STATE.FAILED);
    expect(failed.error).toBe('FFmpeg failed');
    expect(checks.stateFor('D:/caps/s/chain_02_b0/take0000|all').status).toBe(CHECK_STATE.DONE);
  });
});
