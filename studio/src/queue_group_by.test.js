import { describe, it, expect, vi } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({
  queueMatchGroups: vi.fn(),
  queueMatchGroupsCancel: vi.fn(async () => {}),
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));

import { GROUP_BY, parseGroupBy, groupDemos } from './queue_group_by.js';

const demo = (name, extra = {}) => ({
  name, path: `C:\\demos\\${name}`, streaks: [], ...extra,
});
const streaks = (n) => Array.from({ length: n }, () => ({ kill_count: 1 }));
const names = (groups) => groups.map((g) => [g.text, ...g.demos.map((d) => d.name)]);

describe('parseGroupBy', () => {
  it('keeps a known value and turns anything else into None', () => {
    expect(parseGroupBy('map')).toBe(GROUP_BY.MAP);
    expect(parseGroupBy(null)).toBe(GROUP_BY.NONE);
    expect(parseGroupBy('true')).toBe(GROUP_BY.NONE);
    expect(parseGroupBy('bogus')).toBe(GROUP_BY.NONE);
  });
});

describe('groupDemos', () => {
  it('is null for None: the queue keeps its own order', () => {
    expect(groupDemos([demo('a.dem')], GROUP_BY.NONE)).toBeNull();
  });

  it('never adds or drops a demo, whatever the grouping', () => {
    const shown = [
      demo('a.dem', { map_name: 'dod_anzio', is_pov: true, recorder_id: '1', modified_unix_secs: 1_700_000_000 }),
      demo('b.dem', { is_pov: false }),
      demo('c.dem', { map_name: 'dod_avalanche', path: 'D:/other/c.dem' }),
    ];
    for (const by of Object.values(GROUP_BY).filter((v) => v !== GROUP_BY.NONE)) {
      const out = groupDemos(shown, by, { recorderOf: (d) => (d.recorder_id ? { id: d.recorder_id, name: 'x' } : null) });
      expect(out.flatMap((g) => g.demos).map((d) => d.name).sort()).toEqual(['a.dem', 'b.dem', 'c.dem']);
    }
  });

  it('groups by map, names in order, unknown last, with demo and highlight counts', () => {
    const shown = [
      demo('1.dem', { map_name: 'dod_avalanche', streaks: streaks(2) }),
      demo('2.dem'),
      demo('3.dem', { map_name: 'dod_anzio', streaks: streaks(1) }),
      demo('4.dem', { map_name: 'dod_Avalanche', streaks: streaks(3) }),
    ];
    expect(names(groupDemos(shown, GROUP_BY.MAP))).toEqual([
      ['dod_anzio · 1 demo, 1 highlight', '3.dem'],
      ['dod_avalanche · 2 demos, 5 highlights', '1.dem', '4.dem'],
      ['Map not known · 1 demo, 0 highlights', '2.dem'],
    ]);
  });

  it('groups by player with the Player filter labels, HLTV and unknown after', () => {
    const recorders = { 'p1.dem': { id: '76561197972576011', name: 'm00cat' }, 'p2.dem': { id: 'PLAYER_7', name: 'Brain' }, 'p3.dem': { id: '76561197972576011', name: 'm00' } };
    const shown = [
      demo('p1.dem', { is_pov: true }),
      demo('hltv.dem', { is_pov: false }),
      demo('p2.dem', { is_pov: true }),
      demo('lost.dem', { is_pov: true }),
      demo('p3.dem', { is_pov: true }),
    ];
    const out = groupDemos(shown, GROUP_BY.PLAYER, { recorderOf: (d) => recorders[d.name] || null });
    expect(names(out)).toEqual([
      ['Brain · 1 demo, 0 highlights', 'p2.dem'],
      ['m00 / m00cat (STEAM_0:1:6155141) · 2 demos, 0 highlights', 'p1.dem', 'p3.dem'],
      ['HLTV demos · 1 demo, 0 highlights', 'hltv.dem'],
      ['No recording player found · 1 demo, 0 highlights', 'lost.dem'],
    ]);
  });

  it('groups by folder without case or slash differences, the full folder in the tooltip', () => {
    const shown = [
      demo('a.dem', { path: 'C:\\Demos\\Match\\a.dem' }),
      demo('b.dem', { path: 'C:\\demos\\aaa\\b.dem' }),
      demo('c.dem', { path: 'c:/demos/match/c.dem' }),
    ];
    const out = groupDemos(shown, GROUP_BY.FOLDER);
    expect(names(out)).toEqual([
      ['C:\\demos\\aaa · 1 demo, 0 highlights', 'b.dem'],
      ['C:\\Demos\\Match · 2 demos, 0 highlights', 'a.dem', 'c.dem'],
    ]);
    expect(out[1].title).toBe('C:\\Demos\\Match');
  });

  it('groups by the day the file changed, oldest first, unknown last', () => {
    const day = (y, m, d, h = 12) => new Date(y, m - 1, d, h).getTime() / 1000;
    const shown = [
      demo('late.dem', { modified_unix_secs: day(2026, 10, 2) }),
      demo('none.dem'),
      demo('early.dem', { modified_unix_secs: day(2026, 9, 30, 23) }),
      demo('late2.dem', { modified_unix_secs: day(2026, 10, 2, 1) }),
    ];
    const out = groupDemos(shown, GROUP_BY.DATE);
    expect(names(out)).toEqual([
      ['2026-09-30 · 1 demo, 0 highlights', 'early.dem'],
      ['2026-10-02 · 2 demos, 0 highlights', 'late.dem', 'late2.dem'],
      ['Date not known · 1 demo, 0 highlights', 'none.dem'],
    ]);
    expect(out[0].title).toBe('Demo files last changed on 2026-09-30');
  });

  it('groups by demo type: POV, then HLTV, then not known', () => {
    const shown = [demo('h.dem', { is_pov: false }), demo('u.dem'), demo('p.dem', { is_pov: true })];
    expect(names(groupDemos(shown, GROUP_BY.TYPE))).toEqual([
      ['POV demos · 1 demo, 0 highlights', 'p.dem'],
      ['HLTV demos · 1 demo, 0 highlights', 'h.dem'],
      ['Demo type not known · 1 demo, 0 highlights', 'u.dem'],
    ]);
  });

  it('groups by match with the match header and the half tooltip', () => {
    const info = (match, half) => ({ match, half, reason: 'EndOfHalfChat', detail: 'chat: gh 3, gg 0' });
    const shown = [demo('h2.dem'), demo('loose.dem'), demo('h1.dem')];
    Object.defineProperty(shown[0], 'match_info', { value: info(1, 2) });
    Object.defineProperty(shown[2], 'match_info', { value: info(1, 1) });
    const matches = [{ number: 1, map: 'dod_anzio', tags: ['dicE', 'gskiLL'], halves: [] }];
    const out = groupDemos(shown, GROUP_BY.MATCH, { matches });
    expect(names(out)).toEqual([
      ['Match 1 · dod_anzio · dicE vs gskiLL · h1: 1 demo, h2: 1 demo', 'h1.dem', 'h2.dem'],
      ['Not in a match: these demos could not be read', 'loose.dem'],
    ]);
    expect(out[0].title).toBe('What decided the half: chat: gh 3, gg 0');
    expect(out[1].title).toBe('');
  });
});
