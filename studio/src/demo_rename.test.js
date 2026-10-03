import { describe, it, expect } from 'vitest';
import {
  DEFAULT_POV_TEMPLATE, DEFAULT_HLTV_TEMPLATE, parseDemoTemplate, demoValues, planRenames, renamePairs,
} from './demo_rename.js';
import { emptyProjectTeams } from './project_teams.js';

const DIR = 'C:\\demos\\';
// 2026-09-28 12:00 local time, whatever the test machine's zone.
const MODIFIED = new Date(2026, 8, 28, 12).getTime() / 1000;

const pov = (fileName, extra = {}) => ({
  path: DIR + fileName, file_name: fileName, map: 'dod_anzio', modified_unix_secs: MODIFIED,
  demo_type: 'pov', name: 'krod', kills: 31, deaths: 12, side: 'Axis',
  teams: [{ side: 'Allies', tag: 'dicE' }, { side: 'Axis', tag: 'gskiLL' }], error: null, ...extra,
});
const hltv = (fileName, extra = {}) => pov(fileName, {
  demo_type: 'hltv', name: null, kills: null, deaths: null, side: null, ...extra,
});

const plan = (facts, options = {}) => planRenames(facts, {
  povTemplate: DEFAULT_POV_TEMPLATE,
  hltvTemplate: DEFAULT_HLTV_TEMPLATE,
  projectTeams: emptyProjectTeams(),
  selected: new Set(facts.map((f) => f.path)),
  ...options,
});

describe('demo templates (#469)', () => {
  it('accepts both defaults and the demo placeholders', () => {
    expect(parseDemoTemplate(DEFAULT_POV_TEMPLATE).errors).toEqual([]);
    expect(parseDemoTemplate(DEFAULT_HLTV_TEMPLATE).errors).toEqual([]);
    expect(parseDemoTemplate('{demo_type}_{team1}_v_{team2}_{deaths}d_{demo}').errors).toEqual([]);
    expect(parseDemoTemplate('{name|hltv}_{allies:lower|mix}').errors).toEqual([]);
  });

  it("refuses clip-only placeholders, and a fallback on a value every demo has", () => {
    expect(parseDemoTemplate('{row}_{map}').errors[0]).toContain('{row}');
    expect(parseDemoTemplate('{map|x}').errors[0]).toContain('{map|x}');
  });

  it('never warns about telling names apart: clashes are numbered instead', () => {
    expect(parseDemoTemplate('{map}').warnings).toEqual([]);
  });
});

describe('demoValues', () => {
  it("reads the recorder's side for team_name and opponent", () => {
    const values = demoValues(pov('a.dem'), emptyProjectTeams());
    expect(values).toMatchObject({
      name: 'krod', kills: '31', deaths: '12', map: 'anzio', date: '2026-09-28', demo_type: 'pov',
      team_name: 'gskiLL', opponent: 'dicE', allies: 'dicE', axis: 'gskiLL', demo: 'a',
    });
  });

  it('keeps team1/team2 in one order whichever side each team played', () => {
    const half1 = demoValues(pov('a.dem'), emptyProjectTeams());
    const half2 = demoValues(pov('b.dem', { teams: [{ side: 'Allies', tag: 'gskiLL' }, { side: 'Axis', tag: 'dicE' }] }), emptyProjectTeams());
    expect([half1.team1, half1.team2]).toEqual(['dicE', 'gskiLL']);
    expect([half2.team1, half2.team2]).toEqual(['dicE', 'gskiLL']);
    expect([half2.allies, half2.axis]).toEqual(['gskiLL', 'dicE']);
  });

  it("uses the project's team names and merges, and British for the Allied side", () => {
    const teams = { names: { dicE: 'Dice Squad' }, merged: { gskill: 'gskiLL' } };
    const values = demoValues(pov('a.dem', {
      side: 'British', teams: [{ side: 'British', tag: 'dicE' }, { side: 'Axis', tag: 'gskill' }],
    }), teams);
    expect(values).toMatchObject({ team_name: 'Dice Squad', opponent: 'gskiLL', allies: 'Dice Squad' });
  });

  it('leaves an HLTV demo without a player, and an untagged side without a team', () => {
    const values = demoValues(hltv('h.dem', { teams: [{ side: 'Allies', tag: null }, { side: 'Axis', tag: 'gskiLL' }] }), emptyProjectTeams());
    expect(values).toMatchObject({ name: null, kills: null, team_name: null, opponent: null, allies: null, axis: 'gskiLL' });
    expect([values.team1, values.team2]).toEqual(['gskiLL', null]);
  });
});

describe('planRenames', () => {
  it('builds each type from its own template', () => {
    const rows = plan([pov('a.dem'), hltv('h.dem')]);
    expect(rows.map((r) => [r.status, r.to])).toEqual([
      ['rename', 'krod_31k_v_dicE_anzio.dem'],
      ['rename', 'dicE_v_gskiLL_anzio_2026-09-28.dem'],
    ]);
    expect(renamePairs(rows)).toEqual([
      { from: `${DIR}a.dem`, to: `${DIR}krod_31k_v_dicE_anzio.dem` },
      { from: `${DIR}h.dem`, to: `${DIR}dicE_v_gskiLL_anzio_2026-09-28.dem` },
    ]);
  });

  it('numbers clashes, and never takes a name already in the folder', () => {
    const rows = plan([pov('a.dem'), pov('b.dem'), pov('krod_31k_v_dicE_anzio_2.dem', { name: 'x' })], {
      selected: new Set([`${DIR}a.dem`, `${DIR}b.dem`]),
    });
    expect(rows.map((r) => [r.status, r.to, r.numbered])).toEqual([
      ['rename', 'krod_31k_v_dicE_anzio.dem', false],
      ['rename', 'krod_31k_v_dicE_anzio_3.dem', true],
      ['skipped', 'krod_31k_v_dicE_anzio_2.dem', false],
    ]);
  });

  it('keeps names apart per folder only', () => {
    const other = (f) => ({ ...pov(f), path: `C:\\other\\${f}` });
    const rows = plan([pov('a.dem'), other('a.dem')]);
    expect(rows.map((r) => r.to)).toEqual(['krod_31k_v_dicE_anzio.dem', 'krod_31k_v_dicE_anzio.dem']);
    expect(renamePairs(rows)[1].to).toBe('C:\\other\\krod_31k_v_dicE_anzio.dem');
  });

  it('a demo already named by the template is left alone; a change of case still goes through', () => {
    expect(plan([pov('krod_31k_v_dicE_anzio.dem')])[0].status).toBe('same');
    const row = plan([pov('KROD_31k_v_dicE_anzio.dem')])[0];
    expect([row.status, row.to, row.numbered]).toEqual(['rename', 'krod_31k_v_dicE_anzio.dem', false]);
  });

  it('skips unticked and unreadable demos, and a type whose template has errors', () => {
    const rows = plan([pov('a.dem'), pov('bad.dem', { error: 'no frames' }), hltv('h.dem')], {
      hltvTemplate: '{row}',
      selected: new Set([`${DIR}bad.dem`, `${DIR}h.dem`]),
    });
    expect(rows.map((r) => [r.status, r.to])).toEqual([
      ['skipped', 'a.dem'], ['unreadable', 'bad.dem'], ['template', 'h.dem'],
    ]);
    expect(renamePairs(rows)).toEqual([]);
  });

  it('gives a missing value its fallback word, else unknown', () => {
    const rows = plan([hltv('h.dem', { teams: [] })], { hltvTemplate: '{name|hltv}_{allies}_{map}' });
    expect(rows[0].to).toBe('hltv_unknown_anzio.dem');
  });
});
