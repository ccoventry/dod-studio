import { describe, it, expect } from 'vitest';
import { startView, combineProgressView, endText } from './combine_progress.js';

describe('combine_progress (#955)', () => {
  it('shows a line and the sliding block the moment Combine is clicked', () => {
    expect(startView()).toEqual({ pct: 0, text: 'Checking the clips…', indeterminate: true });
  });

  it('counts the clip being re-read, on the sliding block', () => {
    expect(combineProgressView({ step: 'checking', done: 0, total: 8 }, 240))
      .toEqual({ pct: 0, text: 'Checking the clips (1 of 8)…', indeterminate: true });
    expect(combineProgressView({ step: 'checking', done: 2, total: 8 }, 240).text).toBe('Checking the clips (3 of 8)…');
    expect(combineProgressView({ step: 'checking', done: 8, total: 8 }, 240).text).toBe('Checking the clips (8 of 8)…');
    expect(combineProgressView({ step: 'checking', done: 0, total: 0 }, 240).text).toBe('Checking the clips…');
  });

  it('joins on a real bar by length written, with time left once it has a pace', () => {
    // Too early for a pace.
    expect(combineProgressView({ step: 'joining', fraction: 0.01 }, 240, 500))
      .toEqual({ pct: 1, text: 'Joining clips: 0:02 of 4:00', indeterminate: false });
    expect(combineProgressView({ step: 'joining', fraction: 0.5 }, 240, 1000).text).toBe('Joining clips: 2:00 of 4:00');
    // A third done in 20 s: 40 s left.
    expect(combineProgressView({ step: 'joining', fraction: 1 / 3 }, 240, 20000))
      .toEqual({ pct: 33, text: 'Joining clips: 1:20 of 4:00 · about 40 s left', indeterminate: false });
    expect(combineProgressView({ step: 'joining', fraction: 0.1 }, 600, 20000).text)
      .toBe('Joining clips: 1:00 of 10:00 · about 3 min left');
    expect(combineProgressView({ step: 'joining', fraction: 1 }, 240, 60000).text).toBe('Joining clips: 4:00 of 4:00');
  });

  it('reads an event with no step as a join, and keeps a bad fraction in range', () => {
    expect(combineProgressView({ fraction: 0.5 }, 14).pct).toBe(50);
    expect(combineProgressView({ fraction: 7 }, 14).pct).toBe(100);
    expect(combineProgressView({ fraction: 'x' }, 14).pct).toBe(0);
    expect(combineProgressView(null, 14).pct).toBe(0);
  });

  it('says how it ended', () => {
    expect(endText('saved', 42400)).toBe('Saved in 42s.');
    expect(endText('saved', 65000)).toBe('Saved in 1m 05s.');
    expect(endText('cancelled', 3000)).toBe('Cancelled. Nothing was saved.');
    expect(endText('failed', 3000)).toBe('Stopped with an error (see the message).');
  });
});
