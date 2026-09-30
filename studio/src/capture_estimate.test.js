import { describe, it, expect } from 'vitest';
import { streakSeconds, computeRequiredCaptureBytes } from './capture_estimate.js';

const opts = {
  preRollSeconds: 2, postRollSeconds: 0.6,
  recordStartLead: 0, recordStopTrail: 0,
  captureFps: 1, resWidth: 1, resHeight: 1,
};

describe('streakSeconds', () => {
  it('uses kill times, not frame index / tickrate (#464)', () => {
    // Frame index 50000 / 100 would say 500 s; the kills are at 312-326 s.
    const s = { start_tick: 50000, end_tick: 50900, demo_fps: 100,
      kills: [[50000, 312.4, 'K98'], [50400, 318.0, 'K98'], [50900, 325.9, 'Colt']] };
    expect(streakSeconds(s)).toEqual([312.4, 325.9]);
  });

  it('follows the Kill Range', () => {
    const s = { kills: [[1, 10, 'a'], [2, 20, 'b'], [3, 30, 'c']], start_index: 1, end_index: 1 };
    expect(streakSeconds(s)).toEqual([20, 20]);
  });

  it('falls back to ticks for a streak with no kills', () => {
    expect(streakSeconds({ start_tick: 100, end_tick: 300, demo_fps: 100, kills: [] })).toEqual([1, 3]);
  });
});

describe('computeRequiredCaptureBytes', () => {
  const streak = (a, b) => ({ selected: true, kills: [[0, a, 'x'], [0, b, 'y']] });

  it('bills each separate highlight for its own length', () => {
    const demos = [{ streaks: [streak(10, 14), streak(100, 103)] }];
    // 3 bytes per frame at 1x1: (4 + 3) s x 1 fps x 3 bytes.
    expect(computeRequiredCaptureBytes(demos, opts)).toBe(21);
  });

  it('merges highlights whose rolls overlap, so shared footage is billed once', () => {
    const demos = [{ streaks: [streak(10, 14), streak(15, 16)] }];
    expect(computeRequiredCaptureBytes(demos, opts)).toBe(18); // 10..16
  });

  it('ignores unselected highlights', () => {
    const demos = [{ streaks: [{ ...streak(10, 14), selected: false }] }];
    expect(computeRequiredCaptureBytes(demos, opts)).toBe(0);
  });
});
