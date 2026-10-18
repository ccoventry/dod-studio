import { describe, it, expect } from 'vitest';
import {
  defaultPlayers,
  normalizePlayers,
  parseSteamId,
  playerOptions,
  playerFromInput,
  addPlayer,
  updatePlayer,
  removePlayer,
  playerLabel,
  tileRect,
  cleanTile,
  missingPngs,
  missingSprites,
  isPngName,
  isSpriteName,
  OWN_SPRAY,
} from './players.js';

const M00CAT = '76561197972576011';

describe('normalizePlayers', () => {
  it('gives the defaults for a settings file without a table', () => {
    const d = defaultPlayers();
    expect(normalizePlayers(undefined)).toEqual(d);
    expect(d.use_in_game).toBe(true);
    expect(d.pov_fallback).toBe('own');
    expect(d.hltv_fallback).toBe('png');
    expect(d.crosshair_fallback).toBe('game');
    expect(d.crosshair_fallback_sprite).toBe('customXHair.spr');
    expect(d.crosshair_fallback_tile).toBe(1);
  });

  it('never offers "their own" for HLTV, and drops rows with nobody', () => {
    const t = normalizePlayers({
      use_in_game: false,
      hltv_fallback: 'own',
      pov_fallback: 'sometimes',
      crosshair_fallback: 'other',
      crosshair_fallback_tile: 40,
      players: [
        { steam_id: '', name: '  ', png: 'a.png' },
        { steam_id: 'nope', name: 'Bot', png: 'b.png', crosshair_sprite: 'x.spr', crosshair_tile: 3 },
      ],
    });
    expect(t.use_in_game).toBe(false);
    expect(t.hltv_fallback).toBe('png');
    expect(t.pov_fallback).toBe('own');
    expect(t.crosshair_fallback).toBe('game');
    expect(t.crosshair_fallback_tile).toBe(1);
    expect(t.players).toEqual([{ steam_id: '', name: 'Bot', png: 'b.png', crosshair_sprite: 'x.spr', crosshair_tile: 3 }]);
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
    const options = playerOptions([
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
    const options = playerOptions([{ id: M00CAT, name: 'm00cat' }]);
    expect(playerFromInput('m00cat (STEAM_0:1:6155141)', options)).toEqual({ steam_id: M00CAT, name: 'm00cat' });
    expect(playerFromInput('STEAM_0:1:6155141', options)).toEqual({ steam_id: M00CAT, name: '' });
    expect(playerFromInput(' Some Name ', options)).toEqual({ steam_id: '', name: 'Some Name' });
    expect(playerFromInput('  ', options)).toBeNull();
  });
});

describe('rows', () => {
  it('adds a player once: the same SteamID, or the same name without one, finds their row', () => {
    let t = defaultPlayers();
    let at;
    ({ table: t, index: at } = addPlayer(t, { steam_id: M00CAT, name: 'm00cat' }));
    expect(at).toBe(0);
    t = updatePlayer(t, at, { png: 'a.png', crosshair_sprite: 'customXHair.spr', crosshair_tile: 3 });
    ({ table: t, index: at } = addPlayer(t, { steam_id: '', name: 'Bot' }));
    expect(at).toBe(1);
    ({ table: t, index: at } = addPlayer(t, { steam_id: M00CAT, name: '' }));
    expect(at).toBe(0);
    ({ table: t, index: at } = addPlayer(t, { steam_id: '', name: 'BOT' }));
    expect(at).toBe(1);
    // A name row and a SteamID row are different players even with one name.
    ({ table: t, index: at } = addPlayer(t, { steam_id: '', name: 'm00cat' }));
    expect(at).toBe(2);
    expect(t.players).toEqual([
      { steam_id: M00CAT, name: 'm00cat', png: 'a.png', crosshair_sprite: 'customXHair.spr', crosshair_tile: 3 },
      { steam_id: '', name: 'Bot', png: '', crosshair_sprite: '', crosshair_tile: 0 },
      { steam_id: '', name: 'm00cat', png: '', crosshair_sprite: '', crosshair_tile: 0 },
    ]);
    expect(updatePlayer(t, 1, { crosshair_tile: 99 }).players[1].crosshair_tile).toBe(0);
    expect(removePlayer(t, 1).players.map((p) => p.name)).toEqual(['m00cat', 'm00cat']);
    expect(t.players).toHaveLength(3);
  });

  it('labels rows by name and SteamID', () => {
    expect(playerLabel({ steam_id: M00CAT, name: 'm00cat' })).toBe('m00cat (STEAM_0:1:6155141)');
    expect(playerLabel({ steam_id: M00CAT, name: '' })).toBe('STEAM_0:1:6155141');
    expect(playerLabel({ steam_id: '', name: 'Bot' })).toBe('Bot');
  });

  it('lists the PNGs and sprites the table names that the folders lack', () => {
    const t = {
      ...defaultPlayers(),
      players: [
        { steam_id: '', name: 'a', png: 'A.png', crosshair_sprite: 'mine.spr', crosshair_tile: 2 },
        { steam_id: '', name: 'b', png: 'gone.png', crosshair_sprite: 'unused.spr', crosshair_tile: 0 },
      ],
      hltv_fallback_png: 'team.png',
      pov_fallback_png: 'unused.png',
      crosshair_fallback: 'sprite',
      crosshair_fallback_sprite: 'gone.spr',
    };
    expect(missingPngs(t, ['a.png'])).toEqual(['gone.png', 'team.png']);
    expect(missingSprites(t, ['MINE.spr'])).toEqual(['gone.spr']);
    expect(isPngName('x.PNG')).toBe(true);
    expect(isPngName('../x.png')).toBe(false);
    expect(isSpriteName('customXHair.spr')).toBe(true);
    expect(isSpriteName('sprites/x.spr')).toBe(false);
  });
});

describe('the crosshair grid', () => {
  it('numbers tiles the way DoD does: left to right, top to bottom, 64x64', () => {
    expect(tileRect(1)).toEqual({ x: 0, y: 0, w: 64, h: 64 });
    expect(tileRect(2)).toEqual({ x: 64, y: 0, w: 64, h: 64 });
    expect(tileRect(4)).toEqual({ x: 192, y: 0, w: 64, h: 64 });
    expect(tileRect(5)).toEqual({ x: 0, y: 64, w: 64, h: 64 });
    expect(tileRect(16)).toEqual({ x: 192, y: 192, w: 64, h: 64 });
    for (let t = 1; t <= 16; t += 1) {
      const r = tileRect(t);
      expect(r.x + r.w).toBeLessThanOrEqual(256);
      expect(r.y + r.h).toBeLessThanOrEqual(256);
    }
    expect(tileRect(0)).toBeNull();
    expect(tileRect(17)).toBeNull();
    expect(cleanTile('7')).toBe(7);
    expect(cleanTile(2.5)).toBe(0);
  });
});

describe('spray size and From the demo', () => {
  it('defaults the spray size to 64 and keeps only the sizes offered', () => {
    expect(defaultPlayers().spray_size).toBe(64);
    expect(normalizePlayers({ spray_size: 96 }).spray_size).toBe(96);
    expect(normalizePlayers({ spray_size: '128' }).spray_size).toBe(128);
    expect(normalizePlayers({ spray_size: 100 }).spray_size).toBe(64);
    expect(normalizePlayers({}).spray_size).toBe(64);
  });

  it('keeps From the demo on a row and never reports it as a missing PNG', () => {
    const t = normalizePlayers({ players: [{ steam_id: '', name: 'a', png: OWN_SPRAY }] });
    expect(t.players[0].png).toBe('own');
    expect(missingPngs(t, [])).toEqual([]);
  });
});
