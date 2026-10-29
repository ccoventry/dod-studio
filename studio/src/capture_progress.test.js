import { describe, it, expect, vi, afterEach } from 'vitest';
import {
  CAPTURE_SHARES, captureStepView, heldView, endLine, endKind, createCaptureProgress,
} from './capture_progress.js';

afterEach(() => vi.useRealTimers());

describe('captureStepView (#952)', () => {
  it('puts getting the demos ready on the first tenth of the bar', () => {
    // The first line has no count: the block slides over the whole share.
    expect(captureStepView({ stage: 'preparing', index: 0, total: 0, measured: false }))
      .toEqual({ fill: 0, slideTo: CAPTURE_SHARES.preparing[1] });
    // Reading demos again: one at a time, the current one's share slides.
    expect(captureStepView({ stage: 'reading_again', index: 1, total: 2 }))
      .toEqual({ fill: 2.5, slideTo: 5 });
    // Patching: the written percent moves the fill.
    expect(captureStepView({ stage: 'preparing', index: 2.5, total: 5, measured: true }))
      .toEqual({ fill: 7.5, slideTo: null });
    // The decal pass reports none: the block slides over that demo's share.
    expect(captureStepView({ stage: 'preparing', index: 2, total: 5, measured: false }))
      .toEqual({ fill: 7, slideTo: 8 });
  });

  it('counts clips on the rest, with the next clip sliding', () => {
    expect(captureStepView({ stage: 'starting', index: 0, total: 4 })).toEqual({ fill: 10, slideTo: 32 });
    expect(captureStepView({ stage: 'loading', index: 2, total: 4 })).toEqual({ fill: 54, slideTo: 76 });
    expect(captureStepView({ stage: 'capturing', index: 3, total: 4 })).toEqual({ fill: 76, slideTo: 98 });
    // No clip count: the whole clips' share slides.
    expect(captureStepView({ stage: 'starting', index: 0, total: 0 })).toEqual({ fill: 10, slideTo: 98 });
    expect(captureStepView({ stage: 'checking', index: 6, total: 12 })).toEqual({ fill: 99, slideTo: null });
  });

  it('never makes up a number for a report with no stage or count', () => {
    expect(captureStepView({ status: 'Starting' })).toEqual({ fill: 0, slideTo: null });
    expect(captureStepView({ stage: 'capturing', index: 9, total: 4 })).toEqual({ fill: 98, slideTo: 98 });
  });
});

describe('heldView', () => {
  it('never goes below what the bar already shows', () => {
    expect(heldView({ fill: 5, slideTo: 10 }, 7)).toEqual({ fill: 7, slideFrom: 7, slideWidth: 3 });
    expect(heldView({ fill: 5, slideTo: 6 }, 7)).toEqual({ fill: 7, slideFrom: null, slideWidth: 0 });
    expect(heldView({ fill: 50, slideTo: null }, 7)).toEqual({ fill: 50, slideFrom: null, slideWidth: 0 });
  });

  it('only moves forward across a whole batch of stages', () => {
    const reports = [
      { stage: 'preparing', index: 0, total: 0, measured: false },
      { stage: 'reading_again', index: 0, total: 1 },
      { stage: 'preparing', index: 0, total: 2, measured: false },
      { stage: 'preparing', index: 1.5, total: 2, measured: true },
      { stage: 'preparing', index: 2, total: 2, measured: true },
      { stage: 'starting', index: 0, total: 3 },
      { stage: 'loading', index: 0, total: 3 },
      { stage: 'capturing', index: 0, total: 3 },
      { stage: 'capturing', index: 1, total: 3 },
      { stage: 'loading', index: 2, total: 3 },
      { stage: 'capturing', index: 2, total: 3 },
      { stage: 'checking', index: 0, total: 3 },
      { stage: 'checking', index: 3, total: 3 },
    ];
    let shown = 0;
    for (const p of reports) {
      const next = heldView(captureStepView(p), shown).fill;
      expect(next).toBeGreaterThanOrEqual(shown);
      shown = next;
    }
    expect(shown).toBe(100);
  });
});

describe('the end of a batch', () => {
  it('says how long it took and how many clips landed', () => {
    expect(endLine({ status: 'Complete', clips_captured: 12, clips_total: 12 }, 372_000))
      .toBe('Done in 6 min 12 s: 12 clips captured');
    expect(endLine({ status: 'Complete', clips_captured: 10, clips_total: 12 }, 42_000))
      .toBe('Done in 42.0 s: 10 of 12 clips captured');
    expect(endLine({ status: 'Complete', clips_captured: 1, clips_total: 1 }, 42_000))
      .toBe('Done in 42.0 s: 1 clip captured');
    expect(endLine({ status: 'Complete' }, 4_200)).toBe('Done in 4.2 s');
  });

  it('says a cancelled or failed batch as today', () => {
    expect(endLine({ status: 'Cancelled', clips_captured: 2, clips_total: 5 }, 1000)).toBe('Cancelled');
    expect(endLine({ error: true, status: 'Failed to launch HLAE: x' }, 1000)).toBe('Error: Failed to launch HLAE: x');
    expect(endKind({ error: true })).toBe('error');
    expect(endKind({ status: 'Cancelled' })).toBe('cancelled');
    expect(endKind({ status: 'Complete' })).toBe('completed');
  });
});

describe('createCaptureProgress', () => {
  it('runs a clock on the line, logs the start and the end, and ends on Done in', () => {
    vi.useFakeTimers();
    let t = 1_000;
    const statusEl = { textContent: '', title: '' };
    const log = vi.fn();
    const progress = createCaptureProgress({ statusEl, container: null, now: () => t, log });

    progress.update({ running: true, stage: 'capturing', index: 0, total: 2, status: 'Capturing clip 1 of 2 (anzio.dem)' });
    expect(progress.isRunning()).toBe(true);
    expect(log).toHaveBeenCalledWith('[capture] started');
    expect(statusEl.textContent).toBe('Capturing clip 1 of 2 (anzio.dem) · 0:00');

    t = 66_000;
    vi.advanceTimersByTime(1000);
    expect(statusEl.textContent).toBe('Capturing clip 1 of 2 (anzio.dem) · 1:05');

    // A clip boundary's estimate counts down on the line.
    progress.update({ running: true, stage: 'capturing', index: 1, total: 2, status: 'Capturing clip 2 of 2 (anzio.dem)', seconds_left: 240 });
    expect(statusEl.textContent).toBe('Capturing clip 2 of 2 (anzio.dem) · about 4 min left · 1:05');

    // The check of the takes has no time left.
    progress.update({ running: true, stage: 'checking', index: 0, total: 2, status: 'Checking the takes: 0 of 2' });
    expect(statusEl.textContent).toBe('Checking the takes: 0 of 2 · 1:05');

    t = 373_000;
    progress.end({ running: false, status: 'Complete', clips_captured: 2, clips_total: 2 });
    expect(progress.isRunning()).toBe(false);
    expect(statusEl.textContent).toBe('Done in 6 min 12 s: 2 clips captured');
    expect(log).toHaveBeenLastCalledWith('[capture] completed after 6 min 12 s: 2 of 2 clip(s) captured');
  });

  it('keeps the hold line on through the clock and later reports', () => {
    vi.useFakeTimers();
    let t = 1_000;
    const statusEl = { textContent: '', title: '' };
    const progress = createCaptureProgress({ statusEl, container: null, now: () => t });

    progress.update({ running: true, stage: 'capturing', index: 1, total: 4, status: 'Capturing clip 2 of 4 (anzio.dem)' });
    progress.setRoom({ text: 'Will wait after this clip', replaces: false });
    expect(statusEl.textContent).toBe('Capturing clip 2 of 4 (anzio.dem) · Will wait after this clip · 0:00');

    // Held: nothing records, so the hold's line is the whole line, through
    // the clock's redraw and the next clip report alike.
    progress.setRoom({ text: 'Waiting for renders to free 6 GB for the next clip', replaces: true });
    t = 11_000;
    vi.advanceTimersByTime(1000);
    expect(statusEl.textContent).toBe('Waiting for renders to free 6 GB for the next clip · 0:10');
    progress.update({ running: true, stage: 'capturing', index: 2, total: 4, status: 'Capturing clip 3 of 4 (anzio.dem)' });
    expect(statusEl.textContent).toBe('Waiting for renders to free 6 GB for the next clip · 0:10');

    progress.setRoom(null);
    expect(statusEl.textContent).toBe('Capturing clip 3 of 4 (anzio.dem) · 0:10');
    progress.end({ running: false, status: 'Complete', clips_captured: 4, clips_total: 4 });
  });

  it('logs nothing for a batch refused before it ran', () => {
    const statusEl = { textContent: '', title: '' };
    const log = vi.fn();
    const progress = createCaptureProgress({ statusEl, container: null, log });
    progress.end({ running: false, error: true, status: 'No highlights in the batch' });
    expect(log).not.toHaveBeenCalled();
    expect(statusEl.textContent).toBe('Error: No highlights in the batch');
  });
});
