import { describe, it, expect } from 'vitest';
import { takeShortfall, takesShortfallToast } from './takes_shortfall_toast.js';

const taken = (n) => Array.from({ length: n }, () => ({ captured: true }));
const unreached = (n) => Array.from({ length: n }, () => ({ captured: false, not_reached: true }));
const missing = (n) => Array.from({ length: n }, () => ({ captured: false }));
const cut = (cutBy = 'cancel') => ({ captured: false, cut_short: true, cut_by: cutBy });

/** A verified payload of `blocks`, ended by `endedBy`. */
function payload(blocks, endedBy = 'cancel') {
  return { ended_by: endedBy, blocks };
}

describe('takeShortfall', () => {
  it('counts unreached clips apart from missing ones', () => {
    const p = payload([...taken(1), ...missing(1), cut(), ...unreached(2)]);
    expect(takeShortfall(p, 1, 5)).toMatchObject({ missing: 1, cutShort: 1, notReached: 2, endedBy: 'cancel' });
  });
});

describe('takesShortfallToast (#1434)', () => {
  it("is info, not error, for a Cancel that cut one clip and left the rest unreached (the issue's case)", () => {
    const p = payload([...taken(174), cut(), ...unreached(165)]);
    expect(takesShortfallToast(p, 174, 340)).toEqual({
      text: '174/340 takes found on disk — 165 not reached (batch cancelled), 1 cut short by Cancel (partial take, not captured).',
      type: 'info',
    });
  });

  it('is info for a Cancel between clips', () => {
    const p = payload([...taken(1), ...unreached(2)]);
    expect(takesShortfallToast(p, 1, 3)).toEqual({ text: '1/3 takes found on disk — 2 not reached (batch cancelled).', type: 'info' });
  });

  it('stays an error when a take the batch reached is missing', () => {
    const p = payload([...taken(1), ...missing(1), ...unreached(1)]);
    expect(takesShortfallToast(p, 1, 3)).toEqual({
      text: '1/3 takes found on disk — 1 missing, 1 not reached (batch cancelled).',
      type: 'error',
    });
  });

  it('stays an error when the game closing, not the user, ended the batch', () => {
    const p = payload([...taken(1), cut('game_closed'), ...unreached(1)], 'game_closed');
    expect(takesShortfallToast(p, 1, 3)).toEqual({
      text: '1/3 takes found on disk — 1 not reached (the game closed), 1 cut short when the game closed (partial take, not captured).',
      type: 'error',
    });
  });

  it('stays an error for a failed clip or a take that stops early', () => {
    const failed = payload([...taken(1), { captured: false, clip_verdict: { failed: 'no_start' } }]);
    expect(takesShortfallToast(failed, 1, 2).type).toBe('error');
    const short = payload([...taken(1), { captured: false, cut_short: true, short_take: { video_secs: 1, audio_secs: 5 } }]);
    expect(takesShortfallToast(short, 1, 2).type).toBe('error');
  });

  it('is an error for a complete batch missing a take, as before', () => {
    const p = payload([...taken(1), ...missing(1)], 'cancel');
    expect(takesShortfallToast(p, 1, 2)).toEqual({ text: '1/2 takes found on disk — 1 missing.', type: 'error' });
  });
});
