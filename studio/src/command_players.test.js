import { describe, it, expect } from 'vitest';
import {
  steamId64, normalisePlayers, playerChipLabel, queuePlayerOptions, playerFromText, addPlayer,
} from './command_players.js';
import { profileLists, sameLists, saveProfile, normaliseProfiles } from './command_profiles.js';

const A = '76561197961831029';
const B = '76561197972575011';

describe('Scheduled Command players', () => {
  it('reads a SteamID64 or STEAM_X:Y:Z, nothing else', () => {
    expect(steamId64(A)).toBe(A);
    expect(steamId64('STEAM_0:1:782650')).toBe(A);
    expect(steamId64('steam_1:1:782650')).toBe(A);
    expect(steamId64('m00cat')).toBe(null);
    expect(steamId64('PLAYER_3')).toBe(null);
    expect(steamId64(null)).toBe(null);
  });

  it('cleans a list: trimmed, empty entries dropped, one per player', () => {
    expect(normalisePlayers([
      { id: A, name: ' m00cat ' },
      { id: A, name: 'renamed' },
      { id: null, name: 'Brain' },
      { id: null, name: 'brain' },
      { id: 'PLAYER_3', name: '' },
      null,
    ])).toEqual([{ id: A, name: 'm00cat' }, { id: null, name: 'Brain' }]);
    expect(normalisePlayers(undefined)).toEqual([]);
  });

  it('a chip shows the name, else the SteamID', () => {
    expect(playerChipLabel({ id: A, name: 'm00cat' })).toBe('m00cat');
    expect(playerChipLabel({ id: A, name: '' })).toBe('STEAM_0:1:782650');
  });

  it('lists POV recorders and HLTV players from the queue', () => {
    const demos = [
      { path: 'pov.dem', recorder_id: A, streaks: [{ target_player: 'm00cat', steam_id: A }] },
      // A POV demo with no highlights still offers its recorder.
      { path: 'pov2.dem', streaks: [] },
      { path: 'hltv.dem', streaks: [
        { target_player: 'Brain', steam_id: B, spectated: true },
        { target_player: 'NoSteam', steam_id: null, spectated: true },
      ] },
    ];
    const recorderOf = (d) => (d.path === 'pov2.dem' ? { id: 'PLAYER_9', name: 'Bot' } : null);
    const options = queuePlayerOptions(demos, recorderOf);
    expect(options.map((o) => o.player)).toEqual([
      { id: null, name: 'Bot' },
      { id: B, name: 'Brain' },
      { id: A, name: 'm00cat' },
      { id: null, name: 'NoSteam' },
    ]);
    expect(options.find((o) => o.player.id === A).label).toBe('m00cat (STEAM_0:1:782650)');
  });

  it('takes a picked label, a typed SteamID or a typed name', () => {
    const options = queuePlayerOptions([{ streaks: [{ target_player: 'm00cat', steam_id: A }] }]);
    expect(playerFromText('m00cat (STEAM_0:1:782650)', options)).toEqual({ id: A, name: 'm00cat' });
    expect(playerFromText('STEAM_0:1:782650', options)).toEqual({ id: A, name: 'm00cat' });
    expect(playerFromText(B, options)).toEqual({ id: B, name: '' });
    expect(playerFromText('M00CAT', options)).toEqual({ id: A, name: 'm00cat' });
    expect(playerFromText(' Someone ', options)).toEqual({ id: null, name: 'Someone' });
    expect(playerFromText('  ', options)).toBe(null);
    expect(addPlayer([{ id: A, name: 'm00cat' }], { id: A, name: 'm00cat' })).toHaveLength(1);
  });

  it('profiles keep players, and read a command without them as before', () => {
    const lists = {
      init_commands: [],
      custom_commands: [
        { command: 'cl_xhair_style 1', relation: 'Before', offset_seconds: 5, players: [{ id: A, name: 'm00cat' }] },
        { command: 'cl_xhair_style 2', relation: 'Before', offset_seconds: 5, players: [{ id: null, name: 'Brain' }] },
        { command: 'echo all', relation: 'Before', offset_seconds: 5, players: [] },
      ],
    };
    const [saved] = normaliseProfiles(JSON.parse(JSON.stringify(saveProfile([], 'Crosshairs', lists))));
    expect(saved.custom_commands[0].players).toEqual([{ id: A, name: 'm00cat' }]);
    expect(saved.custom_commands[1].players).toEqual([{ id: null, name: 'Brain' }]);
    expect(saved.custom_commands[2]).toEqual({ command: 'echo all', relation: 'Before', offset_seconds: 5 });
    expect(sameLists(saved, lists)).toBe(true);
    // Changing a row's players is an edit to the profile.
    const edited = structuredClone(lists);
    edited.custom_commands[0].players = [];
    expect(sameLists(saved, edited)).toBe(false);
    // A command saved before the field: everyone, unchanged.
    expect(profileLists({ custom_commands: [{ command: 'x', relation: 'After', offset_seconds: 1 }] }).custom_commands)
      .toEqual([{ command: 'x', relation: 'After', offset_seconds: 1 }]);
  });
});
