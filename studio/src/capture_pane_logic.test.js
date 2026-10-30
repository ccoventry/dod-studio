import { describe, it, expect } from 'vitest';
import { describeProblemPaths, formatPreviewSize } from './capture_pane_logic.js';

describe('describeProblemPaths', () => {
  it('gives one problem as a plain sentence', () => {
    expect(describeProblemPaths([{ status: 'not_found', path: 'D:/clips' }]))
      .toBe(`"D:/clips" doesn't exist on this computer (check the spelling, or that the drive is connected)`);
  });

  it('names each reason by status', () => {
    const text = describeProblemPaths([
      { status: 'not_absolute', path: 'clips' },
      { status: 'malformed', path: 'D:/a?b' },
      { status: 'not_a_directory', path: 'D:/x.dem' },
    ]);
    expect(text.split('\n')).toHaveLength(3);
    expect(text).toContain(`• "clips" isn't a full path`);
    expect(text).toContain(`• "D:/a?b" isn't a valid path`);
    expect(text).toContain(`• "D:/x.dem" points to a file, not a folder`);
  });

  it('falls back for a status it does not know', () => {
    expect(describeProblemPaths([{ status: 'weird', path: 'Z:/q' }])).toBe('"Z:/q" is unusable');
  });

  it('caps the list at three and counts the rest', () => {
    const problems = ['a', 'b', 'c', 'd', 'e'].map((p) => ({ status: 'not_found', path: p }));
    const lines = describeProblemPaths(problems).split('\n');
    expect(lines).toHaveLength(4);
    expect(lines[3]).toBe('• ...and 2 more');
  });

  it('shows exactly three without a tail', () => {
    const problems = ['a', 'b', 'c'].map((p) => ({ status: 'not_found', path: p }));
    expect(describeProblemPaths(problems).split('\n')).toHaveLength(3);
  });
});

describe('formatPreviewSize', () => {
  it('shows megabytes to two decimals', () => {
    expect(formatPreviewSize(0)).toBe('0.00 MB');
    expect(formatPreviewSize(1024 * 1024)).toBe('1.00 MB');
    expect(formatPreviewSize(5.5 * 1024 * 1024)).toBe('5.50 MB');
  });
});
