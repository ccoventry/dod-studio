import { describe, it, expect } from 'vitest';
import { streakSeconds, computeRequiredCaptureBytes, AGR_BYTES_PER_FRAME, bytesPerFrameFor, obsBytesPerSecond } from './capture_estimate.js';

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

  it('bills AGR mode by its own bytes per frame, not the resolution', () => {
    const demos = [{ streaks: [streak(10, 14)] }];
    const agr = { ...opts, bytesPerFrame: AGR_BYTES_PER_FRAME };
    expect(computeRequiredCaptureBytes(demos, agr)).toBe(4 * AGR_BYTES_PER_FRAME);
  });
});

describe('bytesPerFrameFor (#1096)', () => {
  it('bills a compressed video take at two thirds of raw, and raw frames as raw', () => {
    expect(bytesPerFrameFor('direct_to_video', 'utvideo', 2560, 1440)).toBe(2560 * 1440 * 2);
    expect(bytesPerFrameFor('direct_to_video', 'ffv1', 10, 10)).toBe(200);
    expect(bytesPerFrameFor('direct_to_video', 'x264_lossless', 10, 10)).toBe(200);
    expect(bytesPerFrameFor('direct_to_video', 'rawvideo', 10, 10)).toBeUndefined();
    expect(bytesPerFrameFor('frame_sequence', 'utvideo', 10, 10)).toBeUndefined();
    expect(bytesPerFrameFor('obs', 'utvideo', 10, 10)).toBeUndefined();
    expect(bytesPerFrameFor('agr', 'utvideo', 10, 10)).toBe(AGR_BYTES_PER_FRAME);
  });
});

describe('an OBS take is billed by quality and length (#1096)', () => {
  // One highlight billed for exactly 10 s: lead 0, 10 s of kills, trail 0.
  const demos = [{ streaks: [{ selected: true, kills: [[0, 100], [0, 110]], start_index: 0, end_index: 1 }] }];
  const base = { preRollSeconds: 0, postRollSeconds: 0, recordStartLead: 0, recordStopTrail: 0, captureFps: 120, resWidth: 2560, resHeight: 1440 };
  const bill = (quality, w, h, fps) => computeRequiredCaptureBytes(demos, {
    ...base, resWidth: w, resHeight: h, captureFps: fps, bytesPerSecond: obsBytesPerSecond(quality, w, h, fps),
  });

  it('agrees with the backend, figure for figure (builder/tests.rs pins the same numbers)', () => {
    expect(bill('standard', 2560, 1440, 120)).toBe(633_333_334);
    expect(bill('standard', 3840, 2160, 240)).toBe(2_850_000_000);
    expect(bill('standard', 1280, 720, 60)).toBe(633_333_334);
  });

  it('is above the largest take measured per quality, within 25%', () => {
    const measured = { small: 1900, standard: 3170, high: 4740, max: 5720, obs_simple: 3340 };
    for (const [quality, mb] of Object.entries(measured)) {
      const minute = obsBytesPerSecond(quality, 2560, 1440, 120) * 60;
      expect(minute).toBeGreaterThan(mb * 1e6);
      expect(minute).toBeLessThan(mb * 1e6 * 1.25);
    }
  });

  it('bills an unknown quality as Standard', () => {
    expect(obsBytesPerSecond('', 2560, 1440, 120)).toBe(obsBytesPerSecond('standard', 2560, 1440, 120));
  });
});
