import { describe, it, expect } from 'vitest';
import { headshotCounts, hasHeadshot, headshotsPlaceholder, headshotsCell } from './headshots.js';

const streak = (headshots, extra = {}) => ({ headshots, start_index: 0, end_index: headshots.length - 1, ...extra });

describe('headshotCounts', () => {
  it('counts each verdict over the whole highlight', () => {
    expect(headshotCounts(streak(['yes', 'no', 'unknown', 'yes']))).toEqual({ yes: 2, no: 1, unknown: 1, kills: 4 });
  });

  it('counts only the Kill Range', () => {
    const narrowed = streak(['yes', 'no', 'no', 'yes'], { start_index: 1, end_index: 2 });
    expect(headshotCounts(narrowed)).toEqual({ yes: 0, no: 2, unknown: 0, kills: 2 });
  });

  it('is null for a highlight scanned before headshots were kept', () => {
    expect(headshotCounts({ kills: [] })).toBeNull();
    expect(headshotCounts({ headshots: [] })).toBeNull();
    expect(headshotCounts(null)).toBeNull();
  });
});

describe('hasHeadshot', () => {
  it('needs a kill that was one, and does not count unknown', () => {
    expect(hasHeadshot(streak(['no', 'yes']))).toBe(true);
    expect(hasHeadshot(streak(['no', 'unknown']))).toBe(false);
    expect(hasHeadshot({})).toBe(false);
  });
});

describe('headshotsPlaceholder', () => {
  it('gives the count, zero included', () => {
    expect(headshotsPlaceholder(streak(['yes', 'yes', 'no']))).toBe('2');
    expect(headshotsPlaceholder(streak(['no']))).toBe('0');
  });

  it('is a lower bound when some kills are unknown', () => {
    expect(headshotsPlaceholder(streak(['yes', 'unknown']))).toBe('1');
  });

  it('has no value when nothing is known', () => {
    expect(headshotsPlaceholder(streak(['unknown', 'unknown']))).toBeNull();
    expect(headshotsPlaceholder({})).toBeNull();
  });
});

describe('headshotsCell', () => {
  it('shows the count, marks unknown kills, and a dash for no data', () => {
    expect(headshotsCell(streak(['yes', 'no']))).toBe('1');
    expect(headshotsCell(streak(['yes', 'unknown']))).toBe('1+?');
    expect(headshotsCell(streak(['unknown']))).toBe('?');
    expect(headshotsCell({})).toBe('–');
  });
});
