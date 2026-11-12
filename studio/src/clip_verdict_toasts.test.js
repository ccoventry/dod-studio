import { describe, it, expect } from 'vitest';
import { clipVerdictToasts, failedClipCount, shortClipCount, NAMED_IN_A_TOAST } from './clip_verdict_toasts.js';

const dispatch = {
  streaks: [
    { target_player: 'm00cat', kill_count: 4, kills: [[100, 61, 'k98']], start_index: 0, end_index: 0 },
    { target_player: 'warchyld', kill_count: 2, kills: [[200, 61, 'k98']], start_index: 0, end_index: 0 },
  ],
};
const block = (i, over) => ({ demo_name: `demo${i}`, source_streak_indices: [i % 2], ...over });
const failed = (i) => block(i, { captured: false, clip_verdict: { failed: 'no_start' } });
const short = (i) => block(i, { captured: true, clip_verdict: { short: { recorded_secs: 10, planned_secs: 20, kills_outside: 0 } } });

describe('clipVerdictToasts (#1401)', () => {
  it('names the highlight of a failed clip', () => {
    const toasts = clipVerdictToasts({ blocks: [failed(0), block(1, { captured: true })] }, dispatch);
    expect(toasts).toHaveLength(1);
    expect(toasts[0].type).toBe('error');
    expect(toasts[0].text).toContain('demo0 · m00cat · 4 kills');
    expect(toasts[0].text).toContain('not marked Captured');
  });

  it('names the highlight of a short clip as a warning', () => {
    const toasts = clipVerdictToasts({ blocks: [short(1)] }, dispatch);
    expect(toasts).toEqual([{ type: 'warning', text: expect.stringContaining('demo1 · warchyld · 2 kills') }]);
  });

  it('names a few and counts the rest', () => {
    const blocks = Array.from({ length: NAMED_IN_A_TOAST + 2 }, (_, i) => failed(i));
    const [toast] = clipVerdictToasts({ blocks }, dispatch);
    expect(toast.text).toMatch(/ and 2 more\.$/);
    expect(failedClipCount({ blocks })).toBe(NAMED_IN_A_TOAST + 2);
  });

  it('says nothing for a batch whose clips were all whole', () => {
    expect(clipVerdictToasts({ blocks: [block(0, { captured: true })] }, dispatch)).toEqual([]);
    expect(clipVerdictToasts({}, dispatch)).toEqual([]);
    expect(failedClipCount({ blocks: [block(0, { captured: true })] })).toBe(0);
  });

  it('counts the short clips, so the all-verified toast can be skipped', () => {
    expect(shortClipCount({ blocks: [short(0), short(1), block(2, { captured: true })] })).toBe(2);
    expect(shortClipCount({ blocks: [block(0, { captured: true })] })).toBe(0);
    expect(shortClipCount({})).toBe(0);
  });
});
