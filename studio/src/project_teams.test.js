import { describe, it, expect } from 'vitest';
import {
  emptyProjectTeams, normalizeProjectTeams, demoHasTeams, rootTag, teamName,
  buildTeamsList, renameTeam, mergeTeam, unmergeTeam, sideTag, teamsForHighlight,
} from './project_teams.js';

const demo = (name, allied, axis, alliedSide = 'Allies') => ({
  name,
  path: `C:\\demos\\${name}`,
  teams: [{ side: alliedSide, tag: allied }, { side: 'Axis', tag: axis }],
});

describe('buildTeamsList', () => {
  it('lists every tag with how many demos it is in, most first', () => {
    const demos = [
      demo('a.dem', 'dicE', 'TEK'),
      demo('b.dem', 'krod', 'dicE'),
      demo('c.dem', 'dicE', null),
    ];
    const { rows, unread } = buildTeamsList(demos, emptyProjectTeams());
    expect(unread).toBe(0);
    expect(rows.map((r) => [r.tag, r.demoCount])).toEqual([['dicE', 3], ['krod', 1], ['TEK', 1]]);
    expect(rows[0].demoNames).toEqual(['a.dem', 'b.dem', 'c.dem']);
    expect(rows[0].name).toBe('dicE');
    expect(rows[0].customName).toBe(false);
  });

  it('counts demos scanned before teams were read, and old projects still load', () => {
    const old = { name: 'old.dem', path: 'C:\\demos\\old.dem', streaks: [] };
    expect(demoHasTeams(old)).toBe(false);
    const { rows, unread } = buildTeamsList([old, demo('a.dem', 'dicE', null)], normalizeProjectTeams(undefined));
    expect(unread).toBe(1);
    expect(rows).toHaveLength(1);
  });

  it('shows a renamed team by its new name, keyed on the tag', () => {
    const teams = emptyProjectTeams();
    renameTeam(teams, 'has bad manners', 'bad manners');
    const { rows } = buildTeamsList([demo('a.dem', 'has bad manners', 'pb j')], teams);
    const row = rows.find((r) => r.tag === 'has bad manners');
    expect(row.name).toBe('bad manners');
    expect(row.customName).toBe(true);
  });

  it('folds merged tags into one row, counting each demo once', () => {
    const teams = emptyProjectTeams();
    mergeTeam(teams, 'jover', 'over');
    const demos = [demo('a.dem', 'over', 'TEK'), demo('b.dem', 'jover', 'TEK'), demo('c.dem', 'over', 'jover')];
    const { rows } = buildTeamsList(demos, teams);
    expect(rows.map((r) => r.tag)).toEqual(['over', 'TEK']);
    expect(rows[0].demoCount).toBe(3);
    expect(rows[0].merged).toEqual(['jover']);
  });
});

describe('renaming', () => {
  it('a blank name, or the tag itself, goes back to the tag', () => {
    const teams = emptyProjectTeams();
    expect(renameTeam(teams, 'pb j', '  PB&J ')).toBe(true);
    expect(teamName(teams, 'pb j')).toBe('PB&J');
    expect(renameTeam(teams, 'pb j', 'PB&J')).toBe(false);
    expect(renameTeam(teams, 'pb j', '')).toBe(true);
    expect(teamName(teams, 'pb j')).toBe('pb j');
    renameTeam(teams, 'pb j', 'pb j');
    expect(teams.names).toEqual({});
  });

  it('renaming a merged tag names its team', () => {
    const teams = emptyProjectTeams();
    mergeTeam(teams, 'pbj', 'pb j');
    renameTeam(teams, 'pbj', 'PB&J');
    expect(teams.names).toEqual({ 'pb j': 'PB&J' });
    expect(teamName(teams, 'pbj')).toBe('PB&J');
  });
});

describe('merging', () => {
  it('follows chains and refuses loops', () => {
    const teams = emptyProjectTeams();
    expect(mergeTeam(teams, 'a', 'b')).toBe(true);
    expect(mergeTeam(teams, 'b', 'c')).toBe(true);
    expect(rootTag(teams, 'a')).toBe('c');
    expect(mergeTeam(teams, 'c', 'a')).toBe(false);
    expect(mergeTeam(teams, 'a', 'a')).toBe(false);
    // A loop written into a hand-edited file stops instead of spinning.
    const broken = normalizeProjectTeams({ merged: { x: 'y', y: 'x' } });
    expect(['x', 'y']).toContain(rootTag(broken, 'x'));
  });

  it('keeps the target team name, and the merged tag gets its own back when split out', () => {
    const teams = emptyProjectTeams();
    renameTeam(teams, 'over', 'Over');
    renameTeam(teams, 'jover', 'J Over');
    mergeTeam(teams, 'jover', 'over');
    expect(teamName(teams, 'jover')).toBe('Over');
    expect(unmergeTeam(teams, 'jover')).toBe(true);
    expect(teamName(teams, 'jover')).toBe('J Over');
    expect(unmergeTeam(teams, 'jover')).toBe(false);
  });
});

describe('normalizeProjectTeams', () => {
  it('keeps names and merges, drops anything malformed', () => {
    const teams = normalizeProjectTeams({
      names: { dicE: 'Dice', bad: 3, '': 'x' },
      merged: ['nope'],
    });
    expect(teams).toEqual({ names: { dicE: 'Dice' }, merged: {} });
    expect(normalizeProjectTeams(null)).toEqual(emptyProjectTeams());
  });

  it('survives the project file round trip', () => {
    const teams = emptyProjectTeams();
    renameTeam(teams, 'över', 'Över');
    mergeTeam(teams, 'over', 'över');
    expect(normalizeProjectTeams(JSON.parse(JSON.stringify(teams)))).toEqual(teams);
  });
});

describe('teamsForHighlight', () => {
  const krodVsDice = demo('a.dem', 'krod', 'dicE', 'British');

  it('names the player team and the first victim team', () => {
    const teams = emptyProjectTeams();
    renameTeam(teams, 'krod', 'KROD');
    const streak = { faction: 'British', victim_factions: ['Axis', 'Axis'], start_index: 0 };
    expect(teamsForHighlight(krodVsDice, streak, teams)).toEqual({ team_name: 'KROD', opponent: 'dicE' });
  });

  it('Allies and British are the same side', () => {
    expect(sideTag(krodVsDice, 'Allies')).toBe('krod');
    expect(sideTag(krodVsDice, 'British')).toBe('krod');
    expect(sideTag(krodVsDice, 'Spectator')).toBe(null);
  });

  it('reads the first victim of the chosen Kill Range, skipping unknown sides', () => {
    const streak = { faction: 'Axis', victim_factions: ['Axis', 'Unknown', 'British'], start_index: 1 };
    expect(teamsForHighlight(krodVsDice, streak, emptyProjectTeams())).toEqual({ team_name: 'dicE', opponent: 'krod' });
  });

  it('falls back to the other side without victim sides', () => {
    const streak = { faction: 'Axis' };
    expect(teamsForHighlight(krodVsDice, streak, emptyProjectTeams())).toEqual({ team_name: 'dicE', opponent: 'krod' });
  });

  it('gives null, not the faction, for a side with no tag', () => {
    const pub = demo('b.dem', null, 'TEK');
    const streak = { faction: 'Axis', victim_factions: ['Allies'] };
    expect(teamsForHighlight(pub, streak, emptyProjectTeams())).toEqual({ team_name: 'TEK', opponent: null });
  });

  it('gives null for both on a demo or streak that predates the data', () => {
    expect(teamsForHighlight({ name: 'old.dem' }, { faction: 'Axis' }, emptyProjectTeams()))
      .toEqual({ team_name: null, opponent: null });
    expect(teamsForHighlight(krodVsDice, {}, emptyProjectTeams())).toEqual({ team_name: null, opponent: null });
  });

  it('a merged tag gives its team name', () => {
    const teams = emptyProjectTeams();
    mergeTeam(teams, 'dicE', 'DICE');
    renameTeam(teams, 'DICE', 'Dice');
    expect(teamsForHighlight(krodVsDice, { faction: 'British', victim_factions: ['Axis'] }, teams).opponent).toBe('Dice');
  });
});
