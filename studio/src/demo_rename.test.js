import { describe, it, expect } from 'vitest';
import {
  DEFAULT_POV_TEMPLATE, DEFAULT_HLTV_TEMPLATE, parseDemoTemplate, demoValues, planRenames, renamePairs,
  placeholdersFor, nameWithoutTag, DEMO_PLACEHOLDERS, DEMO_PLACEHOLDER_GROUPS, listProgressView,
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
    expect(values).toMatchObject({ faction: 'British', enemy_faction: 'Axis' });
  });

  it("gives the recorder's side, the other side, and the name without the side's tag", () => {
    const values = demoValues(pov('a.dem', { name: 'gskiLL | krod' }), emptyProjectTeams());
    expect(values).toMatchObject({ name: 'krod', full_name: 'gskiLL | krod', faction: 'Axis', enemy_faction: 'Allies' });
  });

  it('leaves an HLTV demo without a player, and an untagged side without a team', () => {
    const values = demoValues(hltv('h.dem', { teams: [{ side: 'Allies', tag: null }, { side: 'Axis', tag: 'gskiLL' }] }), emptyProjectTeams());
    expect(values).toMatchObject({
      name: null, kills: null, faction: null, team_name: null, opponent: null, allies: null, axis: 'gskiLL',
    });
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
    const rows = plan([hltv('h.dem', { teams: [] })], { hltvTemplate: '{team1|mix}_{allies}_{map}' });
    expect(rows[0].to).toBe('mix_unknown_anzio.dem');
  });

  it('makes every name plain: letters, digits, - and _ only', () => {
    const rows = plan([pov('a.dem', { name: 'Zoë [x] #1 :D' })], { povTemplate: '{name} {kills}k' });
    expect(rows[0].to).toBe('Zoe_x_1_D_31k.dem');
  });

  it('lower-cases the whole name when asked', () => {
    const rows = plan([pov('a.dem'), hltv('h.dem')], { lowercase: true });
    expect(rows.map((r) => r.to)).toEqual(['krod_31k_v_dice_anzio.dem', 'dice_v_gskill_anzio_2026-09-28.dem']);
  });

  it('refuses a POV-only placeholder in the HLTV template', () => {
    const rows = plan([hltv('h.dem')], { hltvTemplate: '{name}_{map}' });
    expect(rows[0].status).toBe('template');
  });
});

describe('POV-only placeholders', () => {
  it('are errors in an HLTV template and fine in a POV one', () => {
    expect(parseDemoTemplate('{faction}_{kills}k', 'hltv').errors).toHaveLength(2);
    expect(parseDemoTemplate('{faction}_{kills}k', 'pov').errors).toEqual([]);
  });

  it('are not usable in the HLTV template', () => {
    expect(placeholdersFor('hltv')).not.toContain('name');
    expect(placeholdersFor('hltv')).toContain('allies');
    expect(placeholdersFor('pov')).toContain('faction');
  });
});

describe('DEMO_PLACEHOLDER_GROUPS', () => {
  it('holds every placeholder exactly once', () => {
    const grouped = DEMO_PLACEHOLDER_GROUPS.flatMap((g) => g.names);
    expect([...grouped].sort()).toEqual([...DEMO_PLACEHOLDERS].sort());
    expect(new Set(grouped).size).toBe(grouped.length);
  });
});

describe(':first', () => {
  it('keeps the first word of a name, before it is made plain', () => {
    expect(parseDemoTemplate('{name:first}', 'pov').errors).toEqual([]);
    const facts = pov('a.dem', { name: 'm00cat :D', side: 'Allies' });
    expect(plan([facts], { povTemplate: '{name}_{map}' })[0].to).toBe('m00cat_D_anzio.dem');
    expect(plan([facts], { povTemplate: '{name:first}_{map}' })[0].to).toBe('m00cat_anzio.dem');
  });

  it('leaves a one-word value as it is', () => {
    expect(plan([pov('a.dem')], { povTemplate: '{name:first}_{map:first}' })[0].to).toBe('krod_anzio.dem');
  });
});

describe('nameWithoutTag', () => {
  it('drops the tag at either end and the punctuation around it', () => {
    expect(nameWithoutTag('dicE[: :]m00cat :D', 'dicE')).toBe('m00cat :D');
    expect(nameWithoutTag('[DICE] m00cat', 'dice')).toBe('m00cat');
    expect(nameWithoutTag('DICE | m00cat', 'dice')).toBe('m00cat');
    expect(nameWithoutTag('m00cat -gskiLL-', 'gskiLL')).toBe('m00cat');
    expect(nameWithoutTag('m00cat', 'dicE')).toBe('m00cat');
  });

  it('keeps the name when there is no tag, or nothing would be left', () => {
    expect(nameWithoutTag('m00cat', null)).toBe('m00cat');
    expect(nameWithoutTag('dicE', 'dicE')).toBe('dicE');
    expect(nameWithoutTag(null, 'dicE')).toBe(null);
  });
});

describe('listProgressView', () => {
  it('counts the cached demos, then the time left from the pace of the parses', () => {
    expect(listProgressView({ done: 0, total: 6, cached: 0, parsed: 0 })).toEqual({ pct: 0, text: 'Reading demos: 0 / 6' });
    expect(listProgressView({ done: 5, total: 6, cached: 5, parsed: 0 }).text)
      .toBe('Reading demos: 5 / 6 (5 from the analyzer cache)');
    // 2 parses in 2 s, 400 to go: about 400 s.
    expect(listProgressView({ done: 7, total: 407, cached: 5, parsed: 2 }, 2000))
      .toEqual({ pct: 2, text: 'Reading demos: 7 / 407 (5 from the analyzer cache) · about 7 min left' });
    expect(listProgressView({ done: 8, total: 10, cached: 0, parsed: 8 }, 8000).text)
      .toBe('Reading demos: 8 / 10 · about 2 s left');
  });

  it('measures the time left by bytes when the event has them', () => {
    // A 90 MB parse took 3 s; 10 MB is left of 100 MB: about 0.3 s, so 1 s.
    // By count (1 of 2 parses in 3 s) it would have said 3 s.
    expect(listProgressView({
      done: 6, total: 7, cached: 5, parsed: 1, bytes_to_parse: 100e6, bytes_parsed: 90e6,
    }, 3000).text).toBe('Reading demos: 6 / 7 (5 from the analyzer cache) · about 1 s left');
  });

  it('says no time left once nothing is left to parse', () => {
    expect(listProgressView({ done: 6, total: 6, cached: 5, parsed: 1 }, 1000).text)
      .toBe('Reading demos: 6 / 6 (5 from the analyzer cache)');
  });
});
