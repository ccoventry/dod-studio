import { describe, it, expect } from 'vitest';
import { listedStreaks, dropOtherPlayersSavedStreaks, matchesQuickFilters, KILLS_FILTER } from './queue_filters.js';

const streak = (player_index, kill_count) => ({ player_index, kill_count });

describe('listedStreaks', () => {
  it('lists every highlight the scan kept, whoever recorded the demo', () => {
    // The scan already kept a POV demo's recorder's alone (#573).
    const hltv = { local_player_index: null, streaks: [streak(2, 1), streak(5, 3)] };
    expect(listedStreaks(hltv)).toHaveLength(2);
    expect(listedStreaks({ local_player_index: 2, streaks: [streak(2, 1)] })).toHaveLength(1);
    expect(listedStreaks({})).toEqual([]);
  });
});

describe('dropOtherPlayersSavedStreaks', () => {
  it('keeps the recorder\'s highlights in a project saved before #573', () => {
    const demo = { local_player_index: 2, streaks: [streak(2, 1), streak(5, 3), streak(2, 2)] };
    expect(dropOtherPlayersSavedStreaks(demo)).toBe(true);
    expect(demo.streaks.map((s) => s.kill_count)).toEqual([1, 2]);
    expect(demo.streaks.every((s) => s.recorded_this_demo)).toBe(true);
    // Once is enough: the kept rows now carry the flag.
    expect(dropOtherPlayersSavedStreaks(demo)).toBe(false);
  });

  it('leaves new scans and demos with no recorder alone', () => {
    const scanned = { local_player_index: 2, streaks: [{ ...streak(5, 3), recorded_this_demo: false }] };
    expect(dropOtherPlayersSavedStreaks(scanned)).toBe(false);
    expect(scanned.streaks).toHaveLength(1);
    const hltv = { local_player_index: null, streaks: [streak(2, 1), streak(5, 3)] };
    expect(dropOtherPlayersSavedStreaks(hltv)).toBe(false);
    expect(hltv.streaks).toHaveLength(2);
    expect(dropOtherPlayersSavedStreaks({ local_player_index: 2 })).toBe(false);
  });
});

describe('matchesQuickFilters', () => {
  const noKills = { local_player_index: 2, streaks: [] };
  const singles = { local_player_index: 2, streaks: [streak(2, 1), streak(2, 1)] };
  const multi = { local_player_index: 2, streaks: [streak(2, 1), streak(2, 3)] };
  const ownerless = { local_player_index: null, streaks: [streak(5, 4)] };

  it('shows everything by default', () => {
    [noKills, singles, multi, ownerless].forEach((d) => expect(matchesQuickFilters(d)).toBe(true));
  });

  it('with kills hides demos where the recorder never killed anyone', () => {
    const f = { kills: KILLS_FILTER.WITH_KILLS };
    expect(matchesQuickFilters(noKills, f)).toBe(false);
    expect(matchesQuickFilters(singles, f)).toBe(true);
  });

  it('multi-kill needs a highlight of two or more kills by the recorder', () => {
    const f = { kills: KILLS_FILTER.MULTI_KILL };
    expect(matchesQuickFilters(singles, f)).toBe(false);
    expect(matchesQuickFilters(multi, f)).toBe(true);
    expect(matchesQuickFilters(noKills, f)).toBe(false);
  });

  it('recorder-only hides demos with no resolvable recording player', () => {
    expect(matchesQuickFilters(ownerless, { ownerOnly: true })).toBe(false);
    expect(matchesQuickFilters(multi, { ownerOnly: true })).toBe(true);
  });

  it('tracked only hides demos with no tracked work, as the bookmark badge reads it', () => {
    const noted = { path: 'a.dem', streaks: [streak(2, 1), { ...streak(2, 2), notes: 'clutch' }] };
    const marked = { path: 'c.dem', streaks: [{ ...streak(2, 2), curation: 'Keep' }] };
    const plain = { path: 'b.dem', streaks: [{ ...streak(2, 3), selected: true }, streak(2, 1)] };
    const f = { trackedOnly: true };
    expect(matchesQuickFilters(noted, f)).toBe(true);
    expect(matchesQuickFilters(marked, f)).toBe(true);
    // A tick alone isn't tracked work (isDemoTracked).
    expect(matchesQuickFilters(plain, f)).toBe(false);
    expect(matchesQuickFilters(noKills, f)).toBe(false);
    expect(matchesQuickFilters(plain)).toBe(true);
  });

  it('tracked only keeps the demo open in Highlight Details listed', () => {
    const open = { path: 'open.dem', streaks: [streak(2, 1)] };
    expect(matchesQuickFilters(open, { trackedOnly: true, keepPath: 'open.dem' })).toBe(true);
    expect(matchesQuickFilters(open, { trackedOnly: true, keepPath: 'other.dem' })).toBe(false);
    // The other filters still apply to it.
    const ownerless = { path: 'open.dem', local_player_index: null, streaks: [] };
    expect(matchesQuickFilters(ownerless, { trackedOnly: true, ownerOnly: true, keepPath: 'open.dem' })).toBe(false);
  });
});
