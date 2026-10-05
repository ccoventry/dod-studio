import { describe, it, expect } from 'vitest';
import { highlightStartSeconds, highlightDurationSeconds, formatClock } from './highlight_time.js';

// A late highlight: frame-record index 50000 / 100 would say 8:20, but the
// demo player's clock at the first kill reads 10:12.
const streak = () => ({
  start_tick: 50000,
  end_tick: 50900,
  kills: [[50000, 312.4, 'K98'], [50400, 318.0, 'K98'], [50900, 325.9, 'Colt']],
  viewdemo_times: [612.7, 618.3, 626.2],
  start_index: 0,
  end_index: 2,
});

describe('highlight Time and Dur. (#464)', () => {
  it('Time is the first kill on the demo player clock, not frame index / tickrate', () => {
    expect(formatClock(highlightStartSeconds(streak(), 100))).toBe('10:12');
  });

  it('Dur. is last kill minus first kill', () => {
    expect(highlightDurationSeconds(streak(), 100).toFixed(1)).toBe('13.5');
  });

  it('both follow the Kill Range', () => {
    const s = { ...streak(), start_index: 1 };
    expect(formatClock(highlightStartSeconds(s, 100))).toBe('10:18');
    expect(highlightDurationSeconds(s, 100).toFixed(1)).toBe('7.9');
  });

  it('falls back to the old arithmetic for projects saved without viewdemo_times', () => {
    const old = { start_tick: 6100, end_tick: 6600, kills: [], viewdemo_times: [] };
    expect(formatClock(highlightStartSeconds(old, 100))).toBe('1:01');
    expect(highlightDurationSeconds(old, 100)).toBe(5);
  });

  it('copes with missing indices and tickrate', () => {
    const s = { start_tick: 1, end_tick: 2, kills: [[1, 10, 'a'], [2, 12.5, 'b']], viewdemo_times: [70] };
    expect(formatClock(highlightStartSeconds(s, 0))).toBe('1:10');
    expect(highlightDurationSeconds(s)).toBe(2.5);
  });

  it('formatClock pads seconds and never goes negative', () => {
    expect(formatClock(65.9)).toBe('1:05');
    expect(formatClock(-3)).toBe('0:00');
    expect(formatClock(undefined)).toBe('0:00');
  });
});
