import { describe, it, expect } from 'vitest';
import { setStatusByHand, restoreStatus, setVerifiedStatus, setCuration, isSkipped, isDemoTracked, CURATION } from './take_index.js';
import { applyRescanWork, matchRescanHighlights } from './rescan_carry.js';

/** What a queue rescan does to a demo's work (queue_scan.js). */
function rescanned(previous, fresh) {
  applyRescanWork(previous, matchRescanHighlights(previous, fresh));
  return fresh;
}

const streak = (extra = {}) => ({ player_index: 1, kills: [[100, 1.0, 'K98'], [200, 3.0, 'K98']], ...extra });

describe('status source (#105)', () => {
  it('a status set by hand carries the mark', () => {
    const s = streak();
    setStatusByHand(s, 'Rendered');
    expect(s.status).toBe('Rendered');
    expect(s.statusByHand).toBe(true);
  });

  it('moves backwards too', () => {
    const s = streak({ status: 'Rendered' });
    setStatusByHand(s, 'Pending');
    expect(s.status).toBe('Pending');
    expect(s.statusByHand).toBe(true);
  });

  it('Undo restores a verified status and drops the mark', () => {
    const s = streak({ status: 'Captured' });
    const previous = setStatusByHand(s, 'None');
    restoreStatus(s, previous);
    expect(s.status).toBe('Captured');
    expect('statusByHand' in s).toBe(false);
  });

  it('Undo restores an earlier hand-set status with its mark', () => {
    const s = streak();
    setStatusByHand(s, 'Pending');
    const previous = setStatusByHand(s, 'Rendered');
    restoreStatus(s, previous);
    expect(s.status).toBe('Pending');
    expect(s.statusByHand).toBe(true);
  });

  it('Undo of the first change leaves the status unset', () => {
    const s = streak();
    restoreStatus(s, setStatusByHand(s, 'Captured'));
    expect('status' in s).toBe(false);
    expect('statusByHand' in s).toBe(false);
  });

  it('a verified capture or render clears the mark', () => {
    const s = streak();
    setStatusByHand(s, 'Captured');
    expect(setVerifiedStatus(s, 'Captured')).toBe(false);
    expect('statusByHand' in s).toBe(false);
    expect(setVerifiedStatus(s, 'Rendered')).toBe(true);
    expect(s.status).toBe('Rendered');
  });

  it('the mark survives a project save/load round trip', () => {
    const demo = { path: 'C:/demos/a.dem', streaks: [streak()] };
    setStatusByHand(demo.streaks[0], 'Rendered');
    const loaded = JSON.parse(JSON.stringify({ demos: [demo] })).demos[0];
    expect(loaded.streaks[0].status).toBe('Rendered');
    expect(loaded.streaks[0].statusByHand).toBe(true);
  });

  it('the mark survives a re-scan', () => {
    const previous = { path: 'C:/demos/a.dem', streaks: [streak()] };
    setStatusByHand(previous.streaks[0], 'Captured');
    const fresh = rescanned(previous, { path: 'C:/demos/a.dem', streaks: [streak()] });
    expect(fresh.streaks[0].status).toBe('Captured');
    expect(fresh.streaks[0].statusByHand).toBe(true);
  });

  it('a re-scan does not invent the mark', () => {
    const previous = { path: 'C:/demos/a.dem', streaks: [streak({ status: 'Captured' })] };
    const fresh = rescanned(previous, { path: 'C:/demos/a.dem', streaks: [streak()] });
    expect('statusByHand' in fresh.streaks[0]).toBe(false);
  });
});

describe('curation (#44)', () => {
  it('Skip unticks and marks the row; clearing it leaves the tick alone', () => {
    const s = streak({ selected: true });
    setCuration(s, CURATION.SKIP);
    expect(isSkipped(s)).toBe(true);
    expect(s.selected).toBe(false);
    setCuration(s, '');
    expect(s.curation).toBeUndefined();
    expect(isSkipped(s)).toBe(false);
  });

  it('Keep does not change the tick, and any mark counts as tracked', () => {
    const s = streak({ selected: true });
    setCuration(s, CURATION.KEEP);
    expect(s.selected).toBe(true);
    expect(isDemoTracked({ streaks: [s] })).toBe(true);
  });

  it('Maybe keeps the row tickable and counts as tracked (#661)', () => {
    const s = streak({ selected: true });
    setCuration(s, CURATION.MAYBE);
    expect(s.curation).toBe('Maybe');
    expect(s.selected).toBe(true);
    expect(isSkipped(s)).toBe(false);
    expect(isDemoTracked({ streaks: [s] })).toBe(true);
  });

  it('survives a re-scan', () => {
    const before = { path: 'd.dem', streaks: [streak({ curation: CURATION.SKIP })] };
    const after = rescanned(before, { path: 'd.dem', streaks: [streak()] });
    expect(after.streaks[0].curation).toBe(CURATION.SKIP);
  });
});

describe('markBlockCaptured', () => {
  it('marks the highlights a block covers Captured and records the take for its render', async () => {
    const { markBlockCaptured, resolveTake, streakUid } = await import('./take_index.js');
    const { HIGHLIGHT_STATUS } = await import('./status_colors.js');
    const a = streak({ status: HIGHLIGHT_STATUS.PENDING });
    const b = streak({ player_index: 2, status: HIGHLIGHT_STATUS.RENDERED });
    const dispatch = { streaks: [a, b], demoPaths: ['d1.dem', 'd1.dem'] };
    const takeIndex = {};
    const marked = markBlockCaptured({ take_key: 's/b0', source_streak_indices: [0, 1] }, dispatch, takeIndex);
    expect(a.status).toBe(HIGHLIGHT_STATUS.CAPTURED);
    // Status only moves forward: a verified Rendered stays.
    expect(b.status).toBe(HIGHLIGHT_STATUS.RENDERED);
    expect(marked).toEqual({ advanced: 1, markCleared: false });
    expect(resolveTake(takeIndex, 's/b0')).toEqual([streakUid('d1.dem', a), streakUid('d1.dem', b)]);
    // Seen again at the batch's end (#861 marks it first): nothing changes.
    expect(markBlockCaptured({ take_key: 's/b0', source_streak_indices: [0, 1] }, dispatch, takeIndex))
      .toEqual({ advanced: 0, markCleared: false });
  });
});
