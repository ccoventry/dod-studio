import { describe, it, expect, vi, afterEach } from 'vitest';
import {
  elapsedText, lastRunKey, readLastRun, rememberLastRun, againstLastRun,
  showBar, hideBar, createClock,
} from './progress_view.js';

// A `.progress-bar-container` with its fill, enough for showBar/hideBar.
function fakeBar() {
  const classes = new Set();
  const fill = { style: { width: '' } };
  return {
    hidden: true,
    fill,
    classList: {
      toggle: (c, on) => (on ? classes.add(c) : classes.delete(c)),
      remove: (c) => classes.delete(c),
      contains: (c) => classes.has(c),
    },
    querySelector: (sel) => (sel === '.progress-bar-fill' ? fill : null),
  };
}

function fakeStorage(entries = {}) {
  const map = new Map(Object.entries(entries));
  return { getItem: (k) => (map.has(k) ? map.get(k) : null), setItem: (k, v) => map.set(k, v), map };
}

afterEach(() => vi.useRealTimers());

describe('progress_view (#951)', () => {
  it('times a running job as m:ss', () => {
    expect(elapsedText(0)).toBe('0:00');
    expect(elapsedText(6_800)).toBe('0:06');
    expect(elapsedText(75_000)).toBe('1:15');
    expect(elapsedText(-5)).toBe('0:00');
  });

  it('keys a last run by its area and paths, in any order and case', () => {
    expect(lastRunKey('scan', ['D:\\b', 'C:\\A'])).toBe(lastRunKey('scan', ['c:\\a', 'd:\\B']));
    expect(lastRunKey('scan', ['C:\\a'])).not.toBe(lastRunKey('audit', ['C:\\a']));
    expect(lastRunKey('scan', [], 'all')).toBe('scan:all|');
  });

  it('remembers a count, and reads 0 when there is none or storage throws', () => {
    const storage = fakeStorage();
    expect(readLastRun('k', storage)).toBe(0);
    rememberLastRun('k', 120, storage);
    expect(readLastRun('k', storage)).toBe(120);
    // Nothing counted: the last run's count stays.
    rememberLastRun('k', 0, storage);
    expect(readLastRun('k', storage)).toBe(120);
    const broken = { getItem: () => { throw new Error('blocked'); }, setItem: () => { throw new Error('blocked'); } };
    expect(readLastRun('k', broken)).toBe(0);
    expect(() => rememberLastRun('k', 5, broken)).not.toThrow();
    expect(readLastRun('k', null)).toBe(0);
  });

  it('measures against the last run, short of full, and slides without one', () => {
    expect(againstLastRun(50, 200)).toEqual({ pct: 25, indeterminate: false });
    expect(againstLastRun(300, 200)).toEqual({ pct: 95, indeterminate: false });
    expect(againstLastRun(50, 0)).toEqual({ pct: 0, indeterminate: true });
  });

  it('switches a bar between real progress and the sliding block', () => {
    const bar = fakeBar();
    showBar(bar, { pct: 0, indeterminate: true });
    expect(bar.hidden).toBe(false);
    expect(bar.classList.contains('indeterminate')).toBe(true);
    showBar(bar, { pct: 42.5 });
    expect(bar.classList.contains('indeterminate')).toBe(false);
    expect(bar.fill.style.width).toBe('42.5%');
    showBar(bar, { pct: 140 });
    expect(bar.fill.style.width).toBe('100%');
    showBar(bar, { indeterminate: true });
    hideBar(bar);
    expect(bar.hidden).toBe(true);
    expect(bar.classList.contains('indeterminate')).toBe(false);
    expect(bar.fill.style.width).toBe('0%');
    expect(() => showBar(null, {})).not.toThrow();
  });

  it('runs a clock that redraws every second and says how long it ran', () => {
    vi.useFakeTimers();
    let t = 1_000;
    const redraw = vi.fn();
    const clock = createClock({ redraw, now: () => t });
    expect(clock.suffix()).toBe('');
    clock.start();
    expect(clock.running()).toBe(true);
    t += 65_000;
    vi.advanceTimersByTime(3_000);
    expect(redraw).toHaveBeenCalledTimes(3);
    expect(clock.suffix()).toBe(' · 1:05');
    expect(clock.stop()).toBe(65_000);
    expect(clock.running()).toBe(false);
    expect(clock.suffix()).toBe('');
    vi.advanceTimersByTime(3_000);
    expect(redraw).toHaveBeenCalledTimes(3);
  });
});

