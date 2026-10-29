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

  it('says a take far off its planned length is a capture problem, with both frame counts', () => {
    const line = findingLine({ check: 'length', cause: 'capture', warn: true, frames: 1500, planned_frames: 5700, secs: 5 });
    expect(line.text).toContain('1500 frames (5.0 s) but 5700 were planned');
    expect(line.cause).toBe('Capturing again helps');
    expect(line.warn).toBe(true);
  });

  it('says how many kills are outside the clip and where the first falls', () => {
    const one = findingLine({ check: 'kill_outside', cause: 'capture', warn: true, count: 1, at_secs: 12, secs: 8 });
    expect(one.text).toBe('A kill is not in the clip (it falls at 12.0 s of a 8.0 s clip).');
    const two = findingLine({ check: 'kill_outside', cause: 'capture', warn: true, count: 2, at_secs: -2.5, secs: 8 });
    expect(two.text).toContain('2 kills are not in the clip (the first falls at -2.5 s');
  });

  it('shows a take with no plan as OK with a note that its length and kills are unknown', () => {
    const chip = checkChip({ status: CHECK_STATE.DONE, result: { findings: [{ check: 'no_plan', cause: 'unknown', warn: false }] } });
    expect(chip.label).toBe('OK');
    expect(chip.title).toBe("The clip's length and kills weren't checked: this take has no plan recorded (captured before Studio kept one).");
    expect(findingLine({ check: 'no_plan', cause: 'unknown', warn: false }).cause).toBe('');
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
    expect(clean.title).toBe('Picture, sound, length and kills look good.');
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
  it('a capture batch stops the running check, and the pass goes on when it ends', async () => {
    const handlers = {};
    const listen = (name, cb) => { handlers[name] = cb; };
    let release;
    const check = vi.fn()
      .mockImplementationOnce(() => new Promise((_, reject) => { release = () => reject('cancelled'); }))
      .mockImplementation(async () => ({ findings: [] }));
    const cancel = vi.fn(async () => { release?.(); });
    const checks = createTakeChecks({ check, cancel, listen, getFps: () => null, onChange: () => {} });
    checks.sync([job()]);
    await settle();
    handlers.capture_status({ payload: { running: true } });
    await settle();
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(checks.isHeld()).toBe(true);
    // Progress reports during the batch don't start anything.
    handlers.capture_status({ payload: { running: true } });
    checks.sync([job()]);
    checks.run();
    await settle();
    expect(check).toHaveBeenCalledTimes(1);
    handlers.capture_status({ payload: { running: false } });
    await settle();
    await settle();
    expect(check).toHaveBeenCalledTimes(2);
    expect(checks.stateFor('D:/caps/s/chain_01_b0/take0000|all').status).toBe(CHECK_STATE.DONE);
  });

  it('a render starting stops the running check', async () => {
    let rendering = false;
    let release;
    const check = vi.fn(() => new Promise((_, reject) => { release = () => reject('cancelled'); }));
    const cancel = vi.fn(async () => { release?.(); });
    const checks = createTakeChecks({ check, cancel, getFps: () => null, isBlocked: () => rendering, mustStop: () => rendering, onChange: () => {} });
    checks.sync([job()]);
    await settle();
    rendering = true;
    checks.sync([job()]);
    await settle();
    expect(cancel).toHaveBeenCalled();
    expect(checks.isRunning()).toBe(false);
    expect(checks.stateFor('D:/caps/s/chain_01_b0/take0000|all').status).toBe(CHECK_STATE.WAITING);
  });

  it('a take dropped from the list stops being read and the next one is checked', async () => {
    let release;
    const check = vi.fn()
      .mockImplementationOnce(() => new Promise((_, reject) => { release = () => reject('cancelled'); }))
      .mockImplementation(async () => ({ findings: [] }));
    const cancel = vi.fn(async () => { release?.(); });
    const other = job({ id: '1', take_folder: 'D:/caps/s/chain_02_b0/take0000' });
    const checks = createTakeChecks({ check, cancel, getFps: () => null, onChange: () => {} });
    checks.sync([job(), other]);
    await settle();
    checks.sync([other]);
    await settle();
    await settle();
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(check).toHaveBeenCalledTimes(2);
    expect(checks.stateFor('D:/caps/s/chain_02_b0/take0000|all').status).toBe(CHECK_STATE.DONE);
  });
});
