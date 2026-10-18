import { describe, it, expect } from 'vitest';
import {
  defaultSprays,
  normalizeSprays,
  parseSteamId,
  sprayPlayerOptions,
  playerFromInput,
  setPlayerSpray,
  removePlayer,
  playerLabel,
  missingPngs,
  isPngName,
} from './sprays.js';

const M00CAT = '76561197972576011';

describe('normalizeSprays', () => {
  it('gives the defaults for a settings file without a table', () => {
    expect(normalizeSprays(undefined)).toEqual(defaultSprays());
    expect(defaultSprays().pov_fallback).toBe('own');
    expect(defaultSprays().hltv_fallback).toBe('png');
  });

  it('never offers "their own" for HLTV, and drops rows with nobody', () => {
    const t = normalizeSprays({
      hltv_fallback: 'own',
      pov_fallback: 'sometimes',
      players: [{ steam_id: '', name: '  ', png: 'a.png' }, { steam_id: 'nope', name: 'Bot', png: 'b.png' }],
    });
    expect(t.hltv_fallback).toBe('png');
    expect(t.pov_fallback).toBe('own');
    expect(t.players).toEqual([{ steam_id: '', name: 'Bot', png: 'b.png' }]);
  });
});

describe('players', () => {
  it('reads a SteamID in every spelling', () => {
    expect(parseSteamId('STEAM_0:1:6155141')).toBe(M00CAT);
    expect(parseSteamId('[U:1:12310283]')).toBe(M00CAT);
    expect(parseSteamId(M00CAT)).toBe(M00CAT);
    expect(parseSteamId('m00cat')).toBeNull();
    expect(parseSteamId('90071996842377216')).toBeNull();
  });

  it('groups the queue players by SteamID, else by the analyzer id', () => {
    const options = sprayPlayerOptions([
      { id: M00CAT, name: 'dicE[: :]m00cat :D' },
      { id: M00CAT, name: 'm00cat' },
      { id: M00CAT, name: 'dicE[: :]m00cat :D' },
      { id: 'CONNECTION_3', name: 'Cpl. Preddy' },
    ]);
    expect(options).toEqual([
      { steam_id: '', name: 'Cpl. Preddy', label: 'Cpl. Preddy' },
      { steam_id: M00CAT, name: 'dicE[: :]m00cat :D', label: 'dicE[: :]m00cat :D / m00cat (STEAM_0:1:6155141)' },
    ]);
  });

  it('takes a picked option, a typed SteamID, or a typed name', () => {
    const options = sprayPlayerOptions([{ id: M00CAT, name: 'm00cat' }]);
    expect(playerFromInput('m00cat (STEAM_0:1:6155141)', options)).toEqual({ steam_id: M00CAT, name: 'm00cat' });
    expect(playerFromInput('STEAM_0:1:6155141', options)).toEqual({ steam_id: M00CAT, name: '' });
    expect(playerFromInput(' Some Name ', options)).toEqual({ steam_id: '', name: 'Some Name' });
    expect(playerFromInput('  ', options)).toBeNull();
  });
});

describe('rows', () => {
  it('adds a player once: the same SteamID, or the same name without one, updates his row', () => {
    let t = defaultSprays();
    t = setPlayerSpray(t, { steam_id: M00CAT, name: 'm00cat' }, 'a.png');
    t = setPlayerSpray(t, { steam_id: '', name: 'Bot' }, 'b.png');
    t = setPlayerSpray(t, { steam_id: M00CAT, name: '' }, 'c.png');
    t = setPlayerSpray(t, { steam_id: '', name: 'BOT' }, 'd.png');
    // A name row and a SteamID row are different players even with one name.
    t = setPlayerSpray(t, { steam_id: '', name: 'm00cat' }, 'e.png');
    expect(t.players).toEqual([
      { steam_id: M00CAT, name: 'm00cat', png: 'c.png' },
      { steam_id: '', name: 'Bot', png: 'd.png' },
      { steam_id: '', name: 'm00cat', png: 'e.png' },
    ]);
    expect(removePlayer(t, 1).players.map((p) => p.png)).toEqual(['c.png', 'e.png']);
    expect(t.players).toHaveLength(3);
  });

  it('labels rows by name and SteamID', () => {
    expect(playerLabel({ steam_id: M00CAT, name: 'm00cat' })).toBe('m00cat (STEAM_0:1:6155141)');
    expect(playerLabel({ steam_id: M00CAT, name: '' })).toBe('STEAM_0:1:6155141');
    expect(playerLabel({ steam_id: '', name: 'Bot' })).toBe('Bot');
  });

  it('lists the PNGs the table names that logos lacks', () => {
    const t = {
      ...defaultSprays(),
      players: [{ steam_id: '', name: 'a', png: 'A.png' }, { steam_id: '', name: 'b', png: 'gone.png' }],
      hltv_fallback_png: 'team.png',
      pov_fallback_png: 'unused.png',
    };
    expect(missingPngs(t, ['a.png'])).toEqual(['gone.png', 'team.png']);
    expect(isPngName('x.PNG')).toBe(true);
    expect(isPngName('../x.png')).toBe(false);
  });
});
