import { describe, it, expect } from 'vitest';
import { recordingPlayerStreaks, matchesQuickFilters, KILLS_FILTER } from './queue_filters.js';

const streak = (player_index, kill_count) => ({ player_index, kill_count });

describe('recordingPlayerStreaks', () => {
  it('keeps only the recording player\'s streaks, or all when there is none', () => {
    const demo = { local_player_index: 2, streaks: [streak(2, 1), streak(5, 3)] };
    expect(recordingPlayerStreaks(demo)).toEqual([streak(2, 1)]);
    expect(recordingPlayerStreaks({ ...demo, local_player_index: null })).toHaveLength(2);
  });
});

describe('matchesQuickFilters', () => {
  const noKills = { local_player_index: 2, streaks: [streak(5, 4)] };
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
});
