import { describe, it, expect } from 'vitest';
import { matching, moveIndex } from './searchable_select.js';

const options = [
  { value: '', text: 'Bundled (none in a local dev build)' },
  { value: 'a', text: '910 · Oct 22 17:54' },
  { value: 'b', text: '918 · Oct 22 01:10' },
  { value: 'c', text: '889 · Oct 22 17:54' },
];

describe('matching', () => {
  it('keeps the options that contain what is typed, in any case', () => {
    expect(matching(options, '91').map((o) => o.value)).toEqual(['a', 'b']);
    expect(matching(options, 'BUNDLED').map((o) => o.value)).toEqual(['']);
    expect(matching(options, 'nothing')).toEqual([]);
  });

  it('keeps every option when nothing is typed', () => {
    expect(matching(options, '')).toEqual(options);
    expect(matching(options, '   ')).toEqual(options);
  });
});

describe('moveIndex', () => {
  it('moves one row and stops at either end', () => {
    expect(moveIndex(1, 1, 4)).toBe(2);
    expect(moveIndex(3, 1, 4)).toBe(3);
    expect(moveIndex(0, -1, 4)).toBe(0);
  });

  it('starts at the top going down and the bottom going up', () => {
    expect(moveIndex(-1, 1, 4)).toBe(0);
    expect(moveIndex(-1, -1, 4)).toBe(3);
  });

  it('is -1 with no rows', () => {
    expect(moveIndex(0, 1, 0)).toBe(-1);
  });
});
