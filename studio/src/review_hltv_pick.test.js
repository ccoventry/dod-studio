import { describe, it, expect } from 'vitest';
import {
  hltvDemoPlayers, nameCarriesTag, teamPlayers, hltvTeams, emptyHltvPick, normalizeHltvPick, savedHltvPick,
  addPickedPlayers, removePickedPlayer,
} from './review_hltv_pick.js';
import { buildReviewQueue, hltvPlayersFor, HLTV_PLAYERS, REVIEW_COVERS } from './review_queue.js';
import { emptyProjectTeams } from './project_teams.js';

const life = (player, index, at, extra = {}) => ({
  target_player: player, player_index: index, kills: [[at, at, 'K98']], viewdemo_times: [at], kill_count: 1, ...extra,
});
const STEALTH = '76561197960265729';
const GORILLA = '76561197960265731';

// Two HLTV demos of one match: dicE on Allies, then on Axis; the
// gorilla used two names; `oldie` comes from a project with no SteamIDs.
const h1 = {
  path: 'h1.dem', is_pov: false,
  teams: [{ side: 'Allies', tag: 'dicE' }, { side: 'Axis', tag: 'over' }],
  streaks: [
    life('dicE | stealth', 2, 10, { steam_id: STEALTH, faction: 'Allies' }),
    life('gorilla', 3, 20, { steam_id: GORILLA, faction: 'Allies' }),
    life('overlord', 5, 30, { steam_id: '76561197960265733', faction: 'Axis' }),
    life('oldie', 6, 40),
  ],
};
const h2 = {
  path: 'h2.dem', is_pov: false,
  teams: [{ side: 'Allies', tag: 'over' }, { side: 'Axis', tag: 'dicE' }],
  streaks: [
    life('dicE | gorilla', 3, 50, { steam_id: GORILLA, faction: 'Axis' }),
    life('stealth', 2, 60, { faction: 'Axis' }),
  ],
};
// A POV demo: its highlights are the recorder's, not HLTV ones.
const pov = { path: 'p.dem', is_pov: true, streaks: [life('m00cat', 1, 70, { steam_id: '76561197960265730' })] };
const demos = [h1, h2, pov];

describe('players in the queue\'s HLTV demos (#828)', () => {
  it('one entry per SteamID with every name, POV highlights left out', () => {
    const players = hltvDemoPlayers(demos);
    // By label, as the Player filter sorts: the gorilla's names tie, so A to Z.
    expect(players.map((p) => p.id)).toEqual([GORILLA, STEALTH, 'name:oldie', '76561197960265733', 'name:stealth']);
    expect(players.find((p) => p.id === GORILLA).names.sort()).toEqual(['dicE | gorilla', 'gorilla']);
    expect(players.find((p) => p.id === GORILLA).label).toMatch(/\(STEAM_0:1:1\)$/);
  });

  it('a name-only highlight under a name a SteamID player used is that player, not a second entry', () => {
    // h2's `stealth` has no SteamID; the SteamID entry has `dicE | stealth`
    // only, so it is its own entry here. Give the SteamID player that name.
    const withName = [{ ...h1, streaks: [...h1.streaks, life('stealth', 2, 15, { steam_id: STEALTH })] }, h2];
    const ids = hltvDemoPlayers(withName).map((p) => p.id);
    expect(ids).not.toContain('name:stealth');
    expect(ids).toContain(STEALTH);
  });
});

describe('a team\'s players', () => {
  it('a name carries a tag at its start or end, set off from the rest', () => {
    expect(nameCarriesTag('dicE | stealth', 'dice')).toBe(true);
    expect(nameCarriesTag('[dicE]m00cat', 'dicE')).toBe(true);
    expect(nameCarriesTag('m00cat.dicE', 'dicE')).toBe(true);
    expect(nameCarriesTag('overlord', 'over')).toBe(false);
    expect(nameCarriesTag('jover', 'over')).toBe(false);
    expect(nameCarriesTag('', 'over')).toBe(false);
  });

  it('whoever played on the team\'s side, or carries its tag', () => {
    const ids = teamPlayers(demos, emptyProjectTeams(), 'dicE').map((p) => p.id);
    // stealth by tag and side, gorilla by side in h1 even without the tag,
    // and h2's name-only `stealth` by side.
    expect(ids.sort()).toEqual(['name:stealth', GORILLA, STEALTH].sort());
    expect(teamPlayers(demos, emptyProjectTeams(), 'over').map((p) => p.id)).toEqual(['76561197960265733']);
  });

  it('a merged tag counts for its team', () => {
    const teams = { names: { dicE: 'DICE' }, merged: { over: 'dicE' } };
    expect(teamPlayers(demos, teams, 'dicE').map((p) => p.id)).toContain('76561197960265733');
    const listed = hltvTeams(demos, teams);
    expect(listed.map((t) => t.name)).toEqual(['DICE']);
  });

  it('the Teams list\'s teams, each with its players', () => {
    const listed = hltvTeams(demos, emptyProjectTeams());
    expect(listed.map((t) => [t.tag, t.players.length])).toEqual([['dicE', 3], ['over', 1]]);
  });
});

describe('the pick and the review list', () => {
  it('adds each player once, removes by id', () => {
    const players = hltvDemoPlayers(demos);
    let picked = addPickedPlayers([], [players[0], players[1]]);
    picked = addPickedPlayers(picked, [players[1], players[2]]);
    expect(picked.map((p) => p.id)).toEqual([GORILLA, STEALTH, 'name:oldie']);
    expect(removePickedPlayer(picked, GORILLA).map((p) => p.id)).toEqual([STEALTH, 'name:oldie']);
  });

  it('reviews only the picked players\' HLTV highlights, by SteamID or by name, and every POV one', () => {
    const players = hltvDemoPlayers(demos);
    const picked = addPickedPlayers([], players.filter((p) => [GORILLA, 'name:oldie'].includes(p.id)));
    const rows = buildReviewQueue(demos, ['h1.dem', 'h2.dem', 'p.dem'], 1, REVIEW_COVERS.ALL,
      hltvPlayersFor(HLTV_PLAYERS.PICKED, [], { picked })).highlights;
    expect(rows.map((h) => h.player)).toEqual(['gorilla', 'oldie', 'dicE | gorilla', '']);
  });

  it('nobody picked: no HLTV highlight, each counted as left out', () => {
    const result = buildReviewQueue(demos, ['h1.dem'], 1, REVIEW_COVERS.ALL,
      hltvPlayersFor(HLTV_PLAYERS.PICKED, [], { picked: [] }));
    expect(result.highlights).toEqual([]);
    expect(result.notChosen).toBe(4);
  });

  it('round-trips through the project file; an older file has none', () => {
    const picked = addPickedPlayers([], hltvDemoPlayers(demos).slice(0, 2));
    const saved = JSON.parse(JSON.stringify(savedHltvPick({ choice: HLTV_PLAYERS.PICKED, players: picked })));
    const back = normalizeHltvPick(saved);
    expect(back.choice).toBe(HLTV_PLAYERS.PICKED);
    expect(back.players.map((p) => [p.id, p.label])).toEqual(picked.map((p) => [p.id, p.label]));
    expect(normalizeHltvPick(undefined)).toEqual(emptyHltvPick());
    expect(normalizeHltvPick({ hltv: 5, hltv_players: [null, { id: '' }, { id: 'x', names: [3, 'a'] }, { id: 'x' }] }))
      .toEqual({ choice: HLTV_PLAYERS.MISSING, players: [{ id: 'x', names: ['a'], label: 'a' }] });
  });
});
