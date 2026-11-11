import { describe, it, expect } from 'vitest';
import { STRINGS } from './strings.js';
import {
  teamColor, teamLabel, durSecs, formatGameTime, formatDuration,
  isConnected, playerClientId, mortalityLifespans, groupConsecutiveWeapons,
} from './analyzer_report_logic.js';

describe('team colours and labels', () => {
  it('colours known teams and falls back to white', () => {
    expect(teamColor('Allies')).toBe('#4caf50');
    expect(teamColor('Axis')).toBe('#e05252');
    expect(teamColor('Nobody')).toBe('#ffffff');
  });

  it('labels Allies as British when the match is', () => {
    const A = STRINGS.ANALYZER;
    expect(teamLabel('Allies', true)).toBe(A.BRITISH_LABEL);
    expect(teamLabel('British', false)).toBe(A.ALLIES_LABEL);
    expect(teamLabel('Axis', false)).toBe(A.AXIS_LABEL);
    expect(teamLabel('Spectators', false)).toBe(A.SPECTATORS_LABEL);
    expect(teamLabel('???', false)).toBe(A.UNASSIGNED_LABEL);
  });
});

describe('time text', () => {
  it('reads a Duration as seconds', () => {
    expect(durSecs({ secs: 3, nanos: 500000000 })).toBe(3.5);
    expect(durSecs(null)).toBe(0);
  });

  it('formats the game clock as m:ss:cc, clamping negatives', () => {
    expect(formatGameTime(75.25)).toBe('1:15:25');
    expect(formatGameTime(-4)).toBe('0:00:00');
    expect(formatGameTime(undefined)).toBe('0:00:00');
  });

  it('formatDuration is the shared durationText', () => {
    expect(formatDuration(65)).toBe(formatDuration(65));
    expect(typeof formatDuration(3700)).toBe('string');
  });
});

describe('player connection', () => {
  it('reads a connected player id only', () => {
    const on = { connection: { Connected: { client_id: 7 } } };
    expect(isConnected(on)).toBe(true);
    expect(playerClientId(on)).toBe(7);
    expect(isConnected({ connection: 'Disconnected' })).toBe(false);
    expect(playerClientId({ connection: null })).toBe(null);
  });
});

describe('mortalityLifespans', () => {
  const at = (s) => ({ viewdemo_offset: { secs: s, nanos: 0 } });

  it('measures each Alive-to-Dead stretch', () => {
    const m = [[at(0), 'Alive'], [at(30), 'Dead'], [at(40), 'Alive'], [at(55), 'Dead']];
    expect(mortalityLifespans(m)).toEqual([30, 15]);
  });

  it('ignores a death with no spawn and an empty list', () => {
    expect(mortalityLifespans([[at(5), 'Dead']])).toEqual([]);
    expect(mortalityLifespans(null)).toEqual([]);
  });
});

describe('groupConsecutiveWeapons', () => {
  it('collapses runs and keeps order', () => {
    expect(groupConsecutiveWeapons(['Garand', 'Garand', 'Kabar', 'Garand'])).toBe('Garand x2, Kabar, Garand');
    expect(groupConsecutiveWeapons([])).toBe('');
  });
});
