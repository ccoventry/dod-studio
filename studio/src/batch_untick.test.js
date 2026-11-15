import { describe, it, expect, vi } from 'vitest';
import { blockUids, capturedUids, untickUids, createBatchUntick } from './batch_untick.js';
import { streakUid } from './take_index.js';

const streak = (player, first, last, extra = {}) => ({
  player_index: player, kills: [[first], [last]], start_tick: first, end_tick: last, selected: true, ...extra,
});

const demos = () => [
  { path: 'a.dem', streaks: [streak(1, 10, 20), streak(2, 30, 40), streak(3, 50, 60), streak(4, 65, 75)] },
  { path: 'b.dem', streaks: [streak(1, 70, 80)] },
];

// a0 captured, a1 captured but short, a2 failed (no take), b0 never reached.
function batch(d) {
  return {
    streak_uids: [streakUid('a.dem', d[0].streaks[0]), streakUid('a.dem', d[0].streaks[1]), streakUid('a.dem', d[0].streaks[2]), streakUid('b.dem', d[1].streaks[0])],
    blocks: [
      { captured: true, source_streak_indices: [0] },
      { captured: true, clip_verdict: { short: { recorded_secs: 1, planned_secs: 5, kills_outside: 0 } }, source_streak_indices: [1] },
      { captured: false, clip_verdict: { failed: 'no_start' }, source_streak_indices: [2] },
      { captured: false, source_streak_indices: [3, 9] },
    ],
  };
}

describe('capturedUids', () => {
  it('lists captured highlights and keeps short ones apart', () => {
    const d = demos();
    const got = capturedUids(batch(d));
    expect(got.done).toEqual([streakUid('a.dem', d[0].streaks[0])]);
    expect(got.short).toEqual([streakUid('a.dem', d[0].streaks[1])]);
  });

  it('keeps a highlight merged into a take with a short one ticked', () => {
    const b = { streak_uids: ['u0', 'u1'], blocks: [
      { captured: true, source_streak_indices: [0] },
      { captured: true, clip_verdict: { short: {} }, source_streak_indices: [0, 1] },
    ] };
    expect(capturedUids(b)).toEqual({ done: [], short: ['u0', 'u1'] });
  });

  it('tolerates a batch with nothing in it', () => {
    expect(capturedUids(null)).toEqual({ done: [], short: [] });
    expect(blockUids(null, null)).toEqual([]);
  });
});

describe('untickUids', () => {
  it('unticks only the ticked highlights it is given', () => {
    const d = demos();
    d[0].streaks[0].selected = false;
    const n = untickUids(d, [streakUid('a.dem', d[0].streaks[0]), streakUid('b.dem', d[1].streaks[0]), 'not-in-queue']);
    expect(n).toBe(1);
    expect(d[1].streaks[0].selected).toBe(false);
    expect(d[0].streaks[1].selected).toBe(true);
  });
});

describe('createBatchUntick', () => {
  it('unticks captured, leaves short, failed and unreached ticked, redraws and logs counts', () => {
    const d = demos();
    const onSelectionChange = vi.fn();
    const log = vi.fn(() => Promise.resolve());
    const n = createBatchUntick({ getDemos: () => d, onSelectionChange, log }).apply(batch(d));
    expect(n).toBe(1);
    expect(d[0].streaks.map((s) => s.selected)).toEqual([false, true, true, true]);
    expect(d[1].streaks[0].selected).toBe(true);
    expect(onSelectionChange).toHaveBeenCalledTimes(1);
    expect(log).toHaveBeenCalledWith(expect.stringContaining('unticked 1'));
  });

  it('does nothing when the captured highlights are not in the queue', () => {
    const onSelectionChange = vi.fn();
    const log = vi.fn();
    const n = createBatchUntick({ getDemos: () => [], onSelectionChange, log }).apply(batch(demos()));
    expect(n).toBe(0);
    expect(onSelectionChange).not.toHaveBeenCalled();
  });
});
