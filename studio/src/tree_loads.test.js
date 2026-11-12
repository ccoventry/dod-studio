import { describe, expect, it } from 'vitest';
import { unloadedOpenNodes } from './tree_loads.js';

describe('unloadedOpenNodes (#572)', () => {
  it('is every open folder with nothing read and nothing reading', () => {
    const open = new Set(['C:\\', 'C:\\Program Files (x86)', 'C:\\Users', 'C:\\Users\\someone']);
    const cache = new Map([['C:\\', {}], ['C:\\Users', {}]]);
    const pending = new Set(['C:\\Users\\someone']);
    expect(unloadedOpenNodes(open, cache, pending)).toEqual(['C:\\Program Files (x86)']);
  });

  it('is nothing once every open folder is read or being read', () => {
    expect(unloadedOpenNodes(new Set(['a', 'b']), new Map([['a', {}]]), new Set(['b']))).toEqual([]);
  });
});
