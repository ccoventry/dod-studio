import { describe, it, expect } from 'vitest';
import { outdatedLines } from './hd_outdated.js';

describe('outdatedLines', () => {
  it('says nothing for an empty or missing report', () => {
    expect(outdatedLines([])).toEqual([]);
    expect(outdatedLines(undefined)).toEqual([]);
    expect(outdatedLines(null)).toEqual([]);
  });

  it('gives one line per style, naming what changed and the cost', () => {
    const lines = outdatedLines([
      { style: 'ultrasharp', step: 'killfeed_icons' },
      { style: 'remacri', step: 'killfeed_icons' },
    ]);
    expect(lines).toHaveLength(2);
    expect(lines[0]).toBe('ultrasharp was built with an older version: build it again to get the new kill-feed icons (about 5 minutes).');
    expect(lines[1]).toContain('remacri');
  });

  it('joins two steps of one style into one line', () => {
    const lines = outdatedLines([
      { style: 'plain', step: 'killfeed_icons' },
      { style: 'plain', step: 'killfeed_icons' },
    ]);
    expect(lines).toHaveLength(1);
  });

  it('skips a step it has no words for', () => {
    expect(outdatedLines([{ style: 'plain', step: 'something_new' }])).toEqual([]);
  });
});
