import { describe, it, expect } from 'vitest';
import { playersOf, matchesPlayer } from './detail_player_filter.js';

const demo = {
  streaks: [
    { target_player: 'b', kill_count: 1 },
    { target_player: 'a', kill_count: 3 },
    { target_player: 'b', kill_count: 2 },
    { target_player: 'c', kill_count: 1 },
  ],
};

describe('playersOf', () => {
  it('lists each player once, most highlights first, then by name', () => {
    expect(playersOf(demo)).toEqual([
      { name: 'b', count: 2 },
      { name: 'a', count: 1 },
      { name: 'c', count: 1 },
    ]);
  });
});

describe('matchesPlayer', () => {
  it('keeps everyone with no pick', () => {
    expect(demo.streaks.every((s) => matchesPlayer(demo, s, ''))).toBe(true);
  });

  it("keeps only the picked player's highlights", () => {
    expect(demo.streaks.filter((s) => matchesPlayer(demo, s, 'b')).length).toBe(2);
  });

  it("keeps everyone when the picked player isn't in this demo", () => {
    expect(demo.streaks.every((s) => matchesPlayer(demo, s, 'z'))).toBe(true);
  });
});
