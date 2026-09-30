import { describe, it, expect } from 'vitest';
import { STRINGS } from './strings.js';

// The toast formatters: plural and count wording that is easy to break and
// only ever seen at the end of a long scan or before a capture.

describe('scanCompleteToast', () => {
  it('says how many demos were found', () => {
    expect(STRINGS.MAIN.scanCompleteToast(12)).toBe('Scan complete (12 demo(s) found)');
  });

  it('mentions unchanged demos only when there are some (#456)', () => {
    expect(STRINGS.MAIN.scanCompleteToast(3, 0)).toBe('Scan complete (3 demo(s) found)');
    expect(STRINGS.MAIN.scanCompleteToast(3, 40)).toBe(
      'Scan complete (3 new or changed demo(s) found, 40 already in the queue and unchanged)'
    );
  });
});

describe('skippedDemosToast', () => {
  const skipped = (n) =>
    Array.from({ length: n }, (_, i) => ({ name: `d${i + 1}.dem`, reason: 'too short' }));

  it('names up to three demos with their reasons', () => {
    expect(STRINGS.MAIN.skippedDemosToast(skipped(2))).toBe(
      '2 demo(s) could not be read and were skipped: d1.dem (too short); d2.dem (too short)'
    );
  });

  it('counts the rest past three instead of listing them', () => {
    const text = STRINGS.MAIN.skippedDemosToast(skipped(5));
    expect(text).toContain('d3.dem (too short); and 2 more');
    expect(text).not.toContain('d4.dem');
    expect(text.startsWith('5 demo(s)')).toBe(true);
  });

  it('has no "and N more" at exactly three', () => {
    expect(STRINGS.MAIN.skippedDemosToast(skipped(3))).not.toContain('more');
  });
});

describe('bannedCommandsWarning', () => {
  it('is singular for one command', () => {
    const text = STRINGS.CAPTURE.bannedCommandsWarning(1);
    expect(text).toMatch(/^1 command in /);
    expect(text).toContain('fix or remove it');
  });

  it('is plural otherwise', () => {
    const text = STRINGS.CAPTURE.bannedCommandsWarning(3);
    expect(text).toMatch(/^3 commands in /);
    expect(text).toContain('fix or remove them');
  });
});
