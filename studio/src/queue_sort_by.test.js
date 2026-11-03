import { describe, it, expect } from 'vitest';
import { sortDemos, parseSortBy, SORT_BY } from './queue_sort_by.js';
import { groupDemos, GROUP_BY } from './queue_group_by.js';

const demo = (name, modified_unix_secs, kills = 0) => ({
  name,
  path: `C:\\demos\\${name}`,
  modified_unix_secs,
  streaks: Array.from({ length: kills }, () => ({ kills: [] })),
});

// The order demos went into the queue.
const queue = [
  demo('b10.dem', 300, 1),
  demo('a.dem', 100, 3),
  demo('b2.dem', undefined, 0),
  demo('c.dem', 200, 3),
];
const names = (list) => list.map((d) => d.name);

describe('sortDemos', () => {
  it('Added keeps the queue order', () => {
    expect(names(sortDemos(queue, SORT_BY.ADDED))).toEqual(['b10.dem', 'a.dem', 'b2.dem', 'c.dem']);
  });

  it('Name sorts numbers in order, ignoring case', () => {
    expect(names(sortDemos(queue, SORT_BY.NAME))).toEqual(['a.dem', 'b2.dem', 'b10.dem', 'c.dem']);
  });

  it('Newest and Oldest put demos without a date last', () => {
    expect(names(sortDemos(queue, SORT_BY.NEWEST))).toEqual(['b10.dem', 'c.dem', 'a.dem', 'b2.dem']);
    expect(names(sortDemos(queue, SORT_BY.OLDEST))).toEqual(['a.dem', 'c.dem', 'b10.dem', 'b2.dem']);
  });

  it('Most highlights first, ties by name', () => {
    expect(names(sortDemos(queue, SORT_BY.HIGHLIGHTS))).toEqual(['a.dem', 'c.dem', 'b10.dem', 'b2.dem']);
  });

  it('does not change the array it was given', () => {
    const copy = [...queue];
    sortDemos(queue, SORT_BY.NAME);
    expect(queue).toEqual(copy);
  });

  it('anything unknown is Added', () => {
    expect(parseSortBy('bogus')).toBe(SORT_BY.ADDED);
    expect(parseSortBy(null)).toBe(SORT_BY.ADDED);
    expect(parseSortBy('name')).toBe(SORT_BY.NAME);
  });

  it('sorts inside groups, which keep their own order', () => {
    const list = [
      { ...demo('z.dem', 1), map_name: 'dod_anzio' },
      { ...demo('y.dem', 2), map_name: 'dod_armory' },
      { ...demo('x.dem', 3), map_name: 'dod_anzio' },
    ];
    const groups = groupDemos(sortDemos(list, SORT_BY.NAME), GROUP_BY.MAP);
    expect(groups.map((g) => names(g.demos))).toEqual([['x.dem', 'z.dem'], ['y.dem']]);
  });
});
