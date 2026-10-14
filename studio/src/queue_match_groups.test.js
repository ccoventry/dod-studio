import { describe, it, expect, vi } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({
  queueMatchGroups: vi.fn(),
  queueMatchGroupsCancel: vi.fn(async () => {}),
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));

import { stableNumbers, applyGrouping, groupedQueue, matchHeaderText } from './queue_match_groups.js';
import { highlightValues } from './clip_name.js';

// native::match_groups::Grouping, as serde sends it.
const summary = (number, halves, extra = {}) => ({
  number, map: 'dod_anzio', tags: ['dicE', 'gskiLL'],
  halves: halves.map(([half, demos]) => ({ half, demos, reason: half ? 'EndOfHalfChat' : 'Undecided', detail: 'chat: gh' })),
  ...extra,
});
const placed = (path, match_number, half) => ({ path, match_number, half, half_reason: half ? 'EndOfHalfChat' : 'Undecided', half_detail: 'chat: gh' });

describe('stableNumbers', () => {
  it('numbers a first grouping as the backend did', () => {
    const matches = [summary(1, [[1, ['a']]]), summary(2, [[1, ['b']]])];
    expect([...stableNumbers(new Map(), matches)]).toEqual([[1, 1], [2, 2]]);
  });

  it('keeps a match\'s number when an older match arrives', () => {
    const previous = new Map([['a', 1], ['b', 2]]);
    // The new match is older, so the backend made it 1.
    const matches = [summary(1, [[1, ['new']]]), summary(2, [[1, ['a']]]), summary(3, [[1, ['b']]])];
    expect([...stableNumbers(previous, matches)]).toEqual([[2, 1], [3, 2], [1, 3]]);
  });

  it('gives a number to the match that holds most of its old demos', () => {
    const previous = new Map([['a', 1], ['b', 1], ['c', 1]]);
    // The grouping split the old match 1: two demos one way, one the other.
    const matches = [summary(1, [[1, ['c']]]), summary(2, [[1, ['a', 'b']]])];
    expect([...stableNumbers(previous, matches)]).toEqual([[2, 1], [1, 2]]);
  });
});

describe('applyGrouping', () => {
  it('puts match info on each demo without it reaching the project file', () => {
    const demos = [{ path: 'a' }, { path: 'b' }, { path: 'c' }];
    const grouping = { matches: [summary(1, [[1, ['a']], [2, ['b']]])], demos: [placed('a', 1, 1), placed('b', 1, 2)] };
    const matches = applyGrouping(demos, grouping, new Map([[1, 4]]));
    expect(demos[0].match_info).toEqual({ match: 4, half: 1, reason: 'EndOfHalfChat', detail: 'chat: gh' });
    expect(demos[1].match_info.half).toBe(2);
    expect(demos[2].match_info).toBeNull();
    expect(JSON.parse(JSON.stringify(demos))).toEqual([{ path: 'a' }, { path: 'b' }, { path: 'c' }]);
    expect(matches.map((m) => m.number)).toEqual([4]);
  });

  it('feeds {match} and {half} in clip names, h1/h2 or missing, never a guessed half', () => {
    const demos = [{ path: 'a', streaks: [] }, { path: 'b', streaks: [] }];
    applyGrouping(demos, { matches: [], demos: [placed('a', 3, 1), placed('b', 3, null)] }, new Map());
    expect(highlightValues(demos[0], {})).toMatchObject({ half: 'h1', match: '3' });
    expect(highlightValues(demos[1], {})).toMatchObject({ half: null, match: '3' });
    expect(highlightValues({ path: 'c' }, {})).toMatchObject({ half: null, match: null });
  });
});

describe('groupedQueue', () => {
  const demosOf = (specs) => {
    const demos = specs.map(([path]) => ({ path }));
    applyGrouping(demos, { matches: [], demos: specs.filter(([, m]) => m).map(([path, m, h]) => placed(path, m, h)) }, new Map());
    return demos;
  };

  it('orders matches by number, each first half first, unplaced demos last', () => {
    const demos = demosOf([['h2pov', 2, 2], ['loose', null], ['h1hltv', 2, 1], ['unknown', 2, null], ['m1', 1, 1], ['h1pov', 2, 1]]);
    const groups = groupedQueue(demos, [summary(1, [[1, ['m1']]]), summary(2, [[1, ['h1hltv', 'h1pov']], [2, ['h2pov']]])]);
    expect(groups.map((g) => g.match?.number ?? null)).toEqual([1, 2, null]);
    expect(groups[1].demos.map((d) => d.path)).toEqual(['h1hltv', 'h1pov', 'h2pov', 'unknown']);
    expect(groups[2].demos.map((d) => d.path)).toEqual(['loose']);
  });

  it('leaves out a match none of the visible demos is in', () => {
    const demos = demosOf([['a', 2, 1]]);
    const groups = groupedQueue(demos, [summary(1, [[1, ['hidden']]]), summary(2, [[1, ['a']]])]);
    expect(groups.map((g) => g.match.number)).toEqual([2]);
  });
});

describe('matchHeaderText', () => {
  it('names the match, map, teams and how many demos of each half are shown', () => {
    const demos = [{ path: 'a' }, { path: 'b' }, { path: 'c' }, { path: 'd' }];
    applyGrouping(demos, { matches: [], demos: [placed('a', 3, 1), placed('b', 3, 1), placed('c', 3, 2), placed('d', 3, null)] }, new Map());
    expect(matchHeaderText(summary(3, []), demos)).toBe('Match 3 · dod_anzio · dicE vs gskiLL · h1: 2 demos, h2: 1 demo, half unknown: 1 demo');
  });

  it('leaves the teams out when neither side has a tag', () => {
    const demos = [{ path: 'a' }];
    applyGrouping(demos, { matches: [], demos: [placed('a', 1, null)] }, new Map());
    expect(matchHeaderText(summary(1, [], { tags: [null, null] }), demos)).toBe('Match 1 · dod_anzio · half unknown: 1 demo');
  });

  it('has a line for the demos no match holds', () => {
    expect(matchHeaderText(null, [])).toBe('Not in a match: these demos could not be read');
  });
});
