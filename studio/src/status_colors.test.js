import { describe, it, expect } from 'vitest';
import { STATUS_COLORS, HIGHLIGHT_STATUS, statusColor, statusCountColor } from './status_colors.js';
import { STRINGS } from './strings.js';

describe('HIGHLIGHT_STATUS (#35)', () => {
  it('is exactly what the status dropdown offers', () => {
    expect(Object.values(HIGHLIGHT_STATUS).sort()).toEqual([...STRINGS.HIGHLIGHTS.STATUS_OPTIONS].sort());
    expect(HIGHLIGHT_STATUS.NONE).toBe(STRINGS.HIGHLIGHTS.STATUS_UNSET_DEFAULT);
  });
});

describe('highlight status colours (#527)', () => {
  it('Pending is orange, Captured blue, Rendered green, None grey', () => {
    expect(statusColor('Pending')).toBe('var(--warn-orange)');
    expect(statusColor('Captured')).toBe('var(--info-bright)');
    expect(statusColor('Rendered')).toBe('var(--ok)');
    expect(statusColor('None')).toBe('var(--text-dim)');
  });

  it('every status the dropdown offers has its own colour', () => {
    for (const s of STRINGS.HIGHLIGHTS.STATUS_OPTIONS) {
      expect(STATUS_COLORS[s], s).toBeDefined();
    }
    const colours = STRINGS.HIGHLIGHTS.STATUS_OPTIONS.map(statusColor);
    expect(new Set(colours).size).toBe(colours.length);
  });

  it('an unknown status falls back to grey', () => {
    expect(statusColor(undefined)).toBe('var(--text-dim)');
    expect(statusColor('Bogus')).toBe('var(--text-dim)');
  });

  it('queue count columns use the status colour, grey at zero', () => {
    expect(statusCountColor('Captured', 3)).toBe('var(--info-bright)');
    expect(statusCountColor('Captured', 0)).toBe('var(--text-dim)');
  });
});
