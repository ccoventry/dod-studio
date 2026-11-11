import { describe, it, expect } from 'vitest';
import { unitsToMetres, killDistanceLookup, distanceSummary, playerDistanceSummaries } from './kill_distance.js';

const time = (secs, nanos = 0) => ({ viewdemo_offset: { secs, nanos } });
const kill = (over) => ({
  time: time(10), weapon: 'K98', killer: 'A', victim: 'B', teamkill: false, distance: 1000, ...over,
});

describe('unitsToMetres (#448)', () => {
  it('counts one world unit as one inch', () => {
    expect(unitsToMetres(1000)).toBeCloseTo(25.4, 5);
    expect(unitsToMetres(0)).toBe(0);
  });
});

describe('killDistanceLookup', () => {
  const lookup = killDistanceLookup([
    kill({}),
    kill({ time: time(20, 5), victim: 'C', distance: null }),
    kill({ time: time(30), victim: 'D', teamkill: true, distance: 50 }),
  ]);

  it('finds a kill by killer, victim and demo time', () => {
    expect(lookup('A', 'B', time(10))).toBe(1000);
  });
  it('gives null for an unknown position, a team kill or a kill it has never seen', () => {
    expect(lookup('A', 'C', time(20, 5))).toBeNull();
    expect(lookup('A', 'D', time(30))).toBeNull();
    expect(lookup('A', 'B', time(11))).toBeNull();
    expect(lookup('X', 'B', time(10))).toBeNull();
  });
  it('copes with no kills at all', () => {
    expect(killDistanceLookup(undefined)('A', 'B', time(1))).toBeNull();
  });
});

describe('distanceSummary', () => {
  it('averages the known ones and counts the rest', () => {
    expect(distanceSummary([100, null, 300])).toEqual({ measured: 2, total: 3, average: 200 });
  });
  it('has no average when nothing is known', () => {
    expect(distanceSummary([null, null])).toEqual({ measured: 0, total: 2, average: null });
    expect(distanceSummary([])).toEqual({ measured: 0, total: 0, average: null });
  });
});

describe('playerDistanceSummaries', () => {
  const kills = [
    kill({ distance: 100 }),
    kill({ distance: null }),
    kill({ weapon: 'Mp40', distance: 400 }),
    kill({ weapon: 'Mp40', teamkill: true, distance: 9999 }),
    kill({ killer: 'Z', distance: 7777 }),
  ];

  it('sums one player\'s kills of others, overall and per weapon, without team kills', () => {
    const s = playerDistanceSummaries(kills, 'A');
    expect(s.all).toEqual({ measured: 2, total: 3, average: 250 });
    expect(s.byWeapon.get('K98')).toEqual({ measured: 1, total: 2, average: 100 });
    expect(s.byWeapon.get('Mp40')).toEqual({ measured: 1, total: 1, average: 400 });
  });
  it('is empty for a player with no kills', () => {
    const s = playerDistanceSummaries(kills, 'nobody');
    expect(s.all).toEqual({ measured: 0, total: 0, average: null });
    expect(s.byWeapon.size).toBe(0);
  });
});
