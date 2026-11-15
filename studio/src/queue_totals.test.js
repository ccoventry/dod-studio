import { describe, it, expect } from 'vitest';
import { queueTotals } from './queue_totals.js';
import { STRINGS } from './strings.js';

const demos = [
  { streaks: [
    { status: 'Captured', curation: 'Keep', selected: true },
    { status: 'Pending', curation: 'Keep', selected: true },
    { curation: 'Skip' },
  ] },
  { streaks: [{ status: 'Rendered', curation: 'Keep', selected: false }, { status: 'Captured', curation: 'Maybe', selected: true }] },
  { streaks: [] },
];

const counts = (parts) => Object.fromEntries(parts.map((p) => [p.key, p.count]));

describe('queueTotals', () => {
  it('adds up every column the rows show', () => {
    const t = queueTotals(demos);
    expect(t.demos).toBe(3);
    expect(t.highlights).toBe(5);
    expect(t.selected).toBe(3);
    expect(counts(t.status)).toEqual({ Pending: 1, Captured: 2, Rendered: 1 });
    expect(counts(t.review)).toEqual({ Unmarked: 0, Keep: 3, Maybe: 1, Skip: 1 });
  });

  it('is all zeros for no demos', () => {
    const t = queueTotals([]);
    expect([t.demos, t.highlights, t.selected]).toEqual([0, 0, 0]);
    expect(counts(t.status)).toEqual({ Pending: 0, Captured: 0, Rendered: 0 });
  });
});

describe('totalsLabel', () => {
  it('says how many demos, and of how many when filters hide some', () => {
    expect(STRINGS.WORKSPACE.totalsLabel(118, 118)).toBe('Total: 118 demos');
    expect(STRINGS.WORKSPACE.totalsLabel(1, 1)).toBe('Total: 1 demo');
    expect(STRINGS.WORKSPACE.totalsLabel(40, 158)).toBe('Total: 40 of 158 demos');
  });
});
