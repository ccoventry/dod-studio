import { describe, it, expect } from 'vitest';
import { statusCounts, reviewCounts, countsTitle } from './queue_counts.js';
import { REVIEW_COLORS, STATUS_COLORS, reviewCountColor } from './status_colors.js';

const streaks = [
  { status: 'Pending' },
  { status: 'Pending', curation: 'Keep' },
  { status: 'Rendered', curation: 'Maybe' },
  { curation: 'Skip' },
  {},
];

describe('Master Demo Queue Status counts', () => {
  it('counts Pending / Captured / Rendered, in that order, an unset status as none of them', () => {
    expect(statusCounts(streaks).map((p) => [p.key, p.count])).toEqual([
      ['Pending', 2], ['Captured', 0], ['Rendered', 1],
    ]);
  });

  it('greys a zero and colours the rest', () => {
    const [pending, captured] = statusCounts(streaks);
    expect(pending.color).toBe(STATUS_COLORS.Pending);
    expect(captured.color).toBe(STATUS_COLORS.None);
  });
});

describe('Master Demo Queue Review counts', () => {
  it('counts no mark first, then Keep / Maybe / Skip', () => {
    expect(reviewCounts(streaks).map((p) => [p.key, p.count])).toEqual([
      ['Unmarked', 2], ['Keep', 1], ['Maybe', 1], ['Skip', 1],
    ]);
  });

  it('spells the numbers out for the tooltip', () => {
    expect(countsTitle(reviewCounts(streaks))).toBe('2 not reviewed, 1 Keep, 1 Maybe, 1 Skip');
    expect(countsTitle(statusCounts([]))).toBe('0 Pending, 0 Captured, 0 Rendered');
  });

  it('has its own colours, none shared with a status', () => {
    const review = Object.values(REVIEW_COLORS);
    expect(new Set(review).size).toBe(review.length);
    for (const c of review) expect(Object.values(STATUS_COLORS)).not.toContain(c);
    expect(reviewCountColor('Keep', 0)).toBe(STATUS_COLORS.None);
    expect(reviewCountColor('Keep', 3)).toBe(REVIEW_COLORS.Keep);
  });
});
