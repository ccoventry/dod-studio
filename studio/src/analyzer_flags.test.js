import { describe, it, expect } from 'vitest';
import { flagSummary, takenFromEnemy, flagLabel } from './analyzer_flags.js';

const t = (secs) => ({ viewdemo_offset: { secs, nanos: 0 }, real_offset: { secs, nanos: 0 }, frame_index: 1 });

const STATE = {
  allies_are_british: false,
  players: [
    { id: '1', name: 'krod', team: 'Axis', cap_credits: 2, obj_points: 3 },
    { id: '2', name: 'milo', team: 'Axis', cap_credits: 1, obj_points: 1 },
    { id: '3', name: 'dyelife', team: 'Allies', cap_credits: 1, cap_breaks: 2, obj_points: 1 },
    { id: '4', name: 'idle', team: 'Allies', cap_credits: 0, obj_points: 0 },
    { id: '5', name: 'breaker', team: 'Allies', cap_credits: 0, cap_breaks: 1, obj_points: 0 },
  ],
  objectives: {
    flags: [
      { area_index: 0, name: 'Hill', owner: 'Axis' },
      { area_index: 1, name: null, owner: 'Allies' },
    ],
    captures: [
      { time: t(70), flag_name: 'Hill', area_index: 0, team: 'Allies', capper: '3', co_cappers: [], previous_owner: 'Unassigned' },
      { time: t(130), flag_name: 'Hill', area_index: 0, team: 'Axis', capper: '1', co_cappers: ['2'], previous_owner: 'Allies' },
      { time: t(200), flag_name: 'Bridge', area_index: null, team: 'Axis', capper: '1', co_cappers: [], previous_owner: null },
    ],
    attempts: [
      { area_index: 1, team: 'Axis', outcome: 'Cancelled' },
      { area_index: 1, team: 'Axis', outcome: 'Cancelled' },
      { area_index: 0, team: 'Axis', outcome: 'Captured' },
      { area_index: 0, team: 'Allies', outcome: 'RoundEnded' },
    ],
  },
};

describe('analyzer Flags tab (#192)', () => {
  it('a capture taken from the other side, never from a neutral or unknown owner', () => {
    expect(takenFromEnemy({ team: 'Axis', previous_owner: 'Allies' })).toBe(true);
    expect(takenFromEnemy({ team: 'British', previous_owner: 'Axis' })).toBe(true);
    expect(takenFromEnemy({ team: 'Allies', previous_owner: 'Unassigned' })).toBe(false);
    expect(takenFromEnemy({ team: 'Axis', previous_owner: null })).toBe(false);
    expect(takenFromEnemy({ team: 'Allies', previous_owner: 'British' })).toBe(false);
  });

  it('counts each side: captures, breaks, blocks for the defenders, attempts', () => {
    const { teams } = flagSummary(STATE);
    expect(teams.axis).toEqual({ captures: 2, fromEnemy: 1, blocks: 0, attempts: 3 });
    expect(teams.allied).toEqual({ captures: 1, fromEnemy: 0, blocks: 2, attempts: 1 });
  });

  it('lists the flags, the captures with their cappers, and the cappers', () => {
    const s = flagSummary(STATE);
    expect(s.flags).toEqual([
      { area: 0, name: 'Hill', owner: 'Axis', captures: 2, blocked: 0 },
      { area: 1, name: null, owner: 'Allies', captures: 0, blocked: 2 },
    ]);
    expect(s.captures.map((c) => [c.flag, c.fromEnemy, c.cappers.join('+')])).toEqual([
      ['Hill', false, 'dyelife'], ['Hill', true, 'krod+milo'], ['Bridge', false, 'krod'],
    ]);
    // A player with only cap breaks (#192) is listed too.
    expect(s.players.map((p) => [p.name, p.caps, p.breaks, p.points])).toEqual([
      ['krod', 2, 0, 3], ['dyelife', 1, 2, 1], ['milo', 1, 0, 1], ['breaker', 0, 1, 0],
    ]);
  });

  it('reads a stock map token as a name, and keeps a literal one', () => {
    expect(flagLabel('POINT_ANZIO_PLAZA', 'dod_anzio')).toBe('Plaza');
    expect(flagLabel('POINT_BRIDGE', 'dod_anzio')).toBe('Bridge');
    expect(flagLabel('POINT_ANZIO_HILL', null)).toBe('Anzio Hill');
    expect(flagLabel('the alley', 'dod_lennon2')).toBe('the alley');
    expect(flagLabel(null, 'dod_anzio')).toBeNull();
  });

  it('copes with a demo that has no flag data', () => {
    expect(flagSummary({})).toEqual({ teams: { allied: { captures: 0, fromEnemy: 0, blocks: 0, attempts: 0 }, axis: { captures: 0, fromEnemy: 0, blocks: 0, attempts: 0 } }, flags: [], captures: [], players: [] });
  });
});
