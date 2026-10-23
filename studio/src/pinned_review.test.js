import { describe, it, expect } from 'vitest';
import { pinnedReviewAction, foldersToUnpin, offerPinnedReview, getPinnedReviewed, setPinnedReviewed } from './pinned_review.js';

describe('pinnedReviewAction (#533)', () => {
  it('offers the list once, when there are pins', () => {
    expect(pinnedReviewAction({ reviewed: false, pinned: ['D:/demos'], setupOpen: false })).toBe('offer');
    expect(pinnedReviewAction({ reviewed: true, pinned: ['D:/demos'], setupOpen: false })).toBe('skip');
  });

  it('marks it done with no pins, so later pins are never listed', () => {
    expect(pinnedReviewAction({ reviewed: false, pinned: [], setupOpen: false })).toBe('mark');
    expect(pinnedReviewAction({ reviewed: false, pinned: undefined, setupOpen: true })).toBe('mark');
  });

  it('waits for the next launch while the Setup window is open', () => {
    expect(pinnedReviewAction({ reviewed: false, pinned: ['D:/demos'], setupOpen: true })).toBe('skip');
  });
});

describe('foldersToUnpin', () => {
  it('is the folders left unticked, in order', () => {
    expect(foldersToUnpin(['A', 'B', 'C'], ['B'])).toEqual(['A', 'C']);
    expect(foldersToUnpin(['A', 'B'], ['A', 'B'])).toEqual([]);
  });
});

describe('offerPinnedReview without pins', () => {
  it('saves that the look ran and touches no pins', async () => {
    setPinnedReviewed(false);
    let saves = 0;
    let unpinned = null;
    await offerPinnedReview({
      getPinned: () => [],
      unpin: async (d) => { unpinned = d; },
      persist: async () => { saves += 1; },
    });
    expect(getPinnedReviewed()).toBe(true);
    expect(saves).toBe(1);
    expect(unpinned).toBe(null);
  });

  it('does nothing once it has run', async () => {
    setPinnedReviewed(true);
    let saves = 0;
    await offerPinnedReview({ getPinned: () => ['D:/x'], unpin: async () => {}, persist: async () => { saves += 1; } });
    expect(saves).toBe(0);
  });

  it('reads a missing setting as not run yet', () => {
    setPinnedReviewed(undefined);
    expect(getPinnedReviewed()).toBe(false);
  });
});
