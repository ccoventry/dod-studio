import { describe, it, expect } from 'vitest';
import { steamIdText, groupPlayers, parsePlayerQuery, findPlayer } from './player_filter.js';

const ME = '76561197977930126';

describe('steamIdText', () => {
  it('converts a SteamID64 and refuses anything else', () => {
    expect(steamIdText(ME)).toBe('STEAM_0:0:8832199');
    expect(steamIdText('PLAYER_2761379')).toBe(null);
    expect(steamIdText('CONNECTION_4')).toBe(null);
    expect(steamIdText('')).toBe(null);
  });
});

describe('groupPlayers', () => {
  it('groups every name one id was seen with, most-used first', () => {
    const options = groupPlayers([
      { id: ME, name: 'chris' },
      { id: ME, name: '[TAG] chris' },
      { id: ME, name: 'chris' },
      { id: 'PLAYER_93', name: 'Las1k' },
    ]);
    expect(options).toHaveLength(2);
    const me = options.find((o) => o.id === ME);
    expect(me.names).toEqual(['chris', '[TAG] chris']);
    expect(me.label).toBe('chris / [TAG] chris (STEAM_0:0:8832199)');
    expect(options.find((o) => o.id === 'PLAYER_93').label).toBe('Las1k');
  });

  it('sorts options by label and skips entries without an id', () => {
    const options = groupPlayers([{ id: 'b', name: 'Zed' }, { id: 'a', name: 'amy' }, { name: 'ghost' }]);
    expect(options.map((o) => o.label)).toEqual(['amy', 'Zed']);
  });
});

describe('parsePlayerQuery and findPlayer', () => {
  const options = groupPlayers([{ id: ME, name: 'chris' }, { id: 'PLAYER_93', name: 'Las1k' }]);
  const players = [{ id: ME, name: '[TAG] chris' }, { id: 'PLAYER_93', name: 'Las1k' }];

  it('an exact option picks that id, whatever name the player had in this demo', () => {
    const q = parsePlayerQuery(options.find((o) => o.id === ME).label, options);
    expect(q).toEqual({ id: ME });
    expect(findPlayer(players, q).name).toBe('[TAG] chris');
  });

  it('other text searches names and SteamIDs', () => {
    expect(findPlayer(players, parsePlayerQuery('las', options)).id).toBe('PLAYER_93');
    expect(findPlayer(players, parsePlayerQuery('STEAM_0:0:8832199', options)).id).toBe(ME);
    expect(findPlayer(players, parsePlayerQuery('nobody', options))).toBe(null);
  });

  it('empty text is no filter', () => {
    expect(parsePlayerQuery('  ', options)).toBe(null);
    expect(findPlayer(players, null)).toBe(null);
  });
});
