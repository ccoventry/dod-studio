import { describe, it, expect } from 'vitest';
import {
  FPS_LADDER, LEVEL, pct, lowerRate, adviceFor, adviceText, ladderStops, highestSteady,
  firstFailure, ladderSummary, stepLines, testSessionId, soundText, gameText, gameCell,
} from './obs_capture_test_logic.js';

const v = (level, cause = 'none', extra = {}) => ({
  level, cause, skipped_share: 0, missed_share: 0, repeated_share: 0, counters_read: true, sound: 'ok', ...extra,
});

describe('obs capture test logic', () => {
  it('formats shares people read', () => {
    expect(pct(0)).toBe('0%');
    expect(pct(0.0007)).toBe('0.07%');
    expect(pct(0.042)).toBe('4.2%');
    expect(pct(undefined)).toBe('0%');
  });

  it('steps down the ladder', () => {
    expect(lowerRate(240)).toBe(200);
    expect(lowerRate(144)).toBe(120);
    expect(lowerRate(130)).toBe(120);
    expect(lowerRate(60)).toBe(30);
    expect(lowerRate(40)).toBe(30);
  });

  it('gives one setting to change', () => {
    expect(adviceFor(v(LEVEL.STEADY), 120, 'high')).toEqual({ kind: 'none' });
    // An encoder lagging at High or Max gives up quality first.
    expect(adviceFor(v(LEVEL.NOT_STEADY, 'encoder'), 240, 'high')).toEqual({ kind: 'quality', value: 'standard' });
    expect(adviceFor(v(LEVEL.NOT_STEADY, 'encoder'), 240, 'max')).toEqual({ kind: 'quality', value: 'high' });
    // At Standard and below, and for the game or rendering, the rate goes down.
    expect(adviceFor(v(LEVEL.NOT_STEADY, 'encoder'), 240, 'standard')).toEqual({ kind: 'fps', value: 200 });
    expect(adviceFor(v(LEVEL.BORDERLINE, 'game'), 165, 'max')).toEqual({ kind: 'fps', value: 144 });
    expect(adviceFor(v(LEVEL.BORDERLINE, 'rendering'), 120, 'obs_simple')).toEqual({ kind: 'fps', value: 60 });
    expect(adviceText({ kind: 'fps', value: 144 })).toContain('OBS Capture FPS to 144');
    expect(adviceText({ kind: 'quality', value: 'standard' })).toContain('OBS Quality to Standard');
  });

  it('stops the ladder on a clear failure, two shaky rates or an error', () => {
    expect(ladderStops([])).toBe(false);
    expect(ladderStops([{ rate: 60, verdict: v(LEVEL.STEADY) }])).toBe(false);
    expect(ladderStops([{ rate: 60, verdict: v(LEVEL.NOT_STEADY) }])).toBe(true);
    expect(ladderStops([{ rate: 60, verdict: v(LEVEL.STEADY) }, { rate: 120, verdict: v(LEVEL.BORDERLINE) }])).toBe(false);
    expect(ladderStops([{ rate: 120, verdict: v(LEVEL.BORDERLINE) }, { rate: 144, verdict: v(LEVEL.BORDERLINE) }])).toBe(true);
    expect(ladderStops([{ rate: 60, error: 'x' }])).toBe(true);
  });

  it('reports the highest steady rate and what failed above it', () => {
    const results = [
      { rate: 60, verdict: v(LEVEL.STEADY) },
      { rate: 120, verdict: v(LEVEL.STEADY) },
      { rate: 144, verdict: v(LEVEL.BORDERLINE, 'game') },
      { rate: 165, verdict: v(LEVEL.STEADY) },
      { rate: 180, verdict: v(LEVEL.NOT_STEADY, 'encoder') },
    ];
    expect(highestSteady(results)).toBe(165);
    expect(firstFailure(results).rate).toBe(180);
    const lines = ladderSummary(results, 'high');
    expect(lines[0]).toBe('Highest steady rate at OBS Quality High: 165 fps.');
    expect(lines[1]).toContain('180 fps: Not sustainable');
    expect(highestSteady([{ rate: 60, verdict: v(LEVEL.NOT_STEADY) }])).toBeNull();
    expect(ladderSummary([{ rate: 60, verdict: v(LEVEL.NOT_STEADY, 'encoder') }], 'standard')[0]).toContain('not even 60');
    const all = FPS_LADDER.map((rate) => ({ rate, verdict: v(LEVEL.STEADY) }));
    expect(ladderSummary(all, 'standard')[1]).toContain('Every rate up to 240');
  });

  it("writes one clip's result", () => {
    const lines = stepLines({ rate: 240, verdict: v(LEVEL.NOT_STEADY, 'encoder', { skipped_share: 0.95 }), file: { audio_peak_db: -2.04 } }, 'standard');
    expect(lines[0]).toBe('240 fps at OBS Quality Standard: Not sustainable.');
    expect(lines.join('\n')).toContain("OBS's encoder couldn't keep up");
    expect(lines.join('\n')).toContain('95.0% skipped');
    expect(lines.join('\n')).toContain('loudest moment -2.0 dB');
    expect(lines[lines.length - 1]).toContain('OBS Capture FPS to 200');
    const unread = stepLines({ rate: 60, verdict: v(LEVEL.STEADY, 'none', { counters_read: false, sound: 'silent' }), file: {} }, 'high');
    expect(unread.join('\n')).toContain("couldn't be read");
    expect(unread.join('\n')).toContain('silent');
    expect(soundText(v(LEVEL.STEADY, 'none', { sound: 'missing' }))).toContain('no sound track');
    expect(unread.join('\n')).toContain('older than this test');
  });

  it("reports the game's own frame rate from the hook", () => {
    const game = { frames: 1000, avg_fps: 239.6, low1_fps: 201.2, low01_fps: 98.7, max_ms: 14, slower_than_obs: 12 };
    expect(gameText(game, 240)).toBe('The game itself: 240 fps average, 1% low 201 fps, 0.1% low 99 fps; 1.2% of its frames took longer than one 240 fps frame.');
    expect(gameCell(game)).toBe('240 (201)');
    expect(gameCell(null)).toBe('-');
  });

  it("names a test clip's session so the backend knows it", () => {
    expect(testSessionId(144, new Date(2026, 9, 27, 3, 4, 5))).toBe('obs_test_20261027_030405_144fps');
  });
});
