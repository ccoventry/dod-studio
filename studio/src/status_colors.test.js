import { describe, it, expect } from 'vitest';
import { STATUS_COLORS, statusColor, statusCountColor } from './status_colors.js';
import { STRINGS } from './strings.js';

describe('highlight status colours (#527)', () => {
  it('Pending is orange, Captured blue, Rendered green, None grey', () => {
    expect(statusColor('Pending')).toBe('#ffa726');
    expect(statusColor('Captured')).toBe('#2196f3');
    expect(statusColor('Rendered')).toBe('#4caf50');
    expect(statusColor('None')).toBe('#555');
  });

  it('every status the dropdown offers has its own colour', () => {
    for (const s of STRINGS.HIGHLIGHTS.STATUS_OPTIONS) {
      expect(STATUS_COLORS[s], s).toBeDefined();
    }
    const colours = STRINGS.HIGHLIGHTS.STATUS_OPTIONS.map(statusColor);
    expect(new Set(colours).size).toBe(colours.length);
  });

  it('an unknown status falls back to grey', () => {
    expect(statusColor(undefined)).toBe('#555');
    expect(statusColor('Bogus')).toBe('#555');
  });

  it('queue count columns use the status colour, grey at zero', () => {
    expect(statusCountColor('Captured', 3)).toBe('#2196f3');
    expect(statusCountColor('Captured', 0)).toBe('#555');
  });
});
