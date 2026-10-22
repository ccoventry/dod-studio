import { describe, it, expect } from 'vitest';
import { renameDemoInTakeIndex } from './take_index.js';

describe('renameDemoInTakeIndex (#21)', () => {
  it('moves only the relocated demo\'s uids to the new path', () => {
    const index = {
      'takes/a_1': ['C:/old/a.dem#3#100#200', 'C:/keep/b.dem#1#5#9'],
      'takes/a_2': ['C:/old/a.dem#3#300#400'],
    };
    renameDemoInTakeIndex(index, 'C:/old/a.dem', 'D:/new/a.dem');
    expect(index).toEqual({
      'takes/a_1': ['D:/new/a.dem#3#100#200', 'C:/keep/b.dem#1#5#9'],
      'takes/a_2': ['D:/new/a.dem#3#300#400'],
    });
  });

  it('does not touch a demo whose path merely starts the same', () => {
    const index = { k: ['C:/old/a.dem2#1#1#1'] };
    renameDemoInTakeIndex(index, 'C:/old/a.dem', 'D:/a.dem');
    expect(index.k).toEqual(['C:/old/a.dem2#1#1#1']);
  });

  it('is a no-op without an index', () => {
    expect(() => renameDemoInTakeIndex(null, 'a', 'b')).not.toThrow();
  });
});
