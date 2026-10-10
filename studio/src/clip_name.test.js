import { describe, it, expect } from 'vitest';
import {
  DEFAULT_TEMPLATE, parseTemplate, buildName, highlightValues, clipNameFor,
  automaticClipName, uniqueNames, clipNamesForTakes, maxNameLength, timeWord, highlightRow,
} from './clip_name.js';
import { streakUid } from './take_index.js';

function streak(overrides = {}) {
  return {
    player_index: 3,
    target_player: 'krod',
    start_tick: 1000,
    end_tick: 1600,
    kill_count: 4,
    kills: [[1000, 754.2, 'Garand'], [1200, 756, 'Garand'], [1400, 758, 'MP40'], [1600, 760, 'Garand']],
    viewdemo_times: [754.2, 756, 758, 760],
    start_index: 0,
    end_index: 3,
    faction: 'Allies',
    victims: ['dicE: Hub', 'bajko', 'dicE: Hub', 'element'],
    victim_factions: ['Axis', 'Axis', 'Axis', 'Axis'],
    ...overrides,
  };
}

function demo(streaks, overrides = {}) {
  return {
    path: 'C:/d/scrim-anzio_h1.dem',
    name: 'scrim-anzio_h1.dem',
    map_name: 'dod_anzio',
    local_player_index: 3,
    modified_unix_secs: Date.UTC(2026, 8, 20, 12) / 1000,
    streaks,
    ...overrides,
  };
}

describe('parseTemplate', () => {
  it('accepts the default template and every placeholder with modifiers', () => {
    expect(parseTemplate(DEFAULT_TEMPLATE).errors).toEqual([]);
    expect(parseTemplate('{faction:lower}_{map:upper}_{opponent:lower|mix}_{row}').errors).toEqual([]);
  });

  it('reads placeholder and modifier names in any case, and keeps the fallback as typed', () => {
    expect(parseTemplate('{MAP}_{Map:UPPER}_{Opponent|MiX}_{row}').errors).toEqual([]);
    const s = streak();
    const v = highlightValues(demo([s]), s);
    expect(buildName(parseTemplate('{MAP}_{row}'), v).name).toBe(buildName(parseTemplate('{map}_{row}'), v).name);
    expect(buildName(parseTemplate('{Team_Name|MiX}_{row}'), v).name).toBe('MiX_01');
  });

  it('flags unknown placeholders and modifiers, braces and bad characters', () => {
    expect(parseTemplate('{oponent}_{row}').errors[0]).toContain('{oponent}');
    expect(parseTemplate('{map:title}_{row}').errors[0]).toContain('{map:title}');
    expect(parseTemplate('{map_{row}').errors[0]).toContain('never closed');
    expect(parseTemplate('map}_{row}').errors[0]).toContain('no "{"');
    expect(parseTemplate('{map}?{row}').errors[0]).toContain("Windows doesn't allow");
    expect(parseTemplate('{map|x}_{row}').errors[0]).toContain('only {team_name} and {opponent}');
    expect(parseTemplate('   ').errors[0]).toContain('empty');
  });

  it('warns, without refusing, when nothing tells highlights apart', () => {
    const parsed = parseTemplate('{map}_{player}');
    expect(parsed.errors).toEqual([]);
    expect(parsed.warnings).toHaveLength(1);
  });
});

describe('highlightValues', () => {
  it('reads every placeholder over the chosen kill range', () => {
    const s = streak({ start_index: 1, end_index: 2 });
    const v = highlightValues(demo([s]), s);
    expect(v).toMatchObject({
      player: 'krod', faction: 'Allies', enemy_faction: 'Axis', map: 'anzio', kills: '2',
      weapons: 'garand-mp40', first_weapon: 'garand', victims: 'bajko-dicE: Hub',
      first_victim: 'bajko', row: '01', time: '12m36s', demo: 'scrim-anzio_h1',
      date: '2026-09-20', team_name: null, opponent: null,
    });
  });

  it('numbers rows among the recording player\'s highlights only', () => {
    const other = streak({ player_index: 5 });
    const second = streak({ start_tick: 5000 });
    const d = demo([streak(), other, second]);
    expect(highlightRow(d, second)).toBe(2);
  });
});

describe('buildName', () => {
  it('cleans values, applies modifiers and falls back for missing teams', () => {
    const s = streak();
    const v = highlightValues(demo([s]), s);
    const { name } = buildName(parseTemplate('{player}_v_{opponent:lower|mix}_{victims}_{faction:upper}'), v);
    expect(name).toBe('krod_v_mix_dicE_ Hub-bajko-element_ALLIES');
    expect(buildName(parseTemplate('{team_name}_{row}'), v).name).toBe('unknown_01');
  });

  it(':first keeps the first word, read before the value is cleaned', () => {
    const s = streak();
    const v = { ...highlightValues(demo([s]), s), player: 'm00cat :D' };
    expect(buildName(parseTemplate('{player:first}_{row}'), v).name).toBe('m00cat_01');
    // A one-word value is kept whole; a missing one still falls back.
    expect(buildName(parseTemplate('{map:first}_{team_name:first}_{row}'), v).name).toMatch(/^[a-z0-9]+_unknown_01$/);
  });

  it('cuts victims, demo, then weapons to fit, never row or time', () => {
    const s = streak({ victims: ['a'.repeat(40), 'b'.repeat(40), 'c', 'd'] });
    const v = highlightValues(demo([s]), s);
    const parsed = parseTemplate('{victims}_{demo}_{weapons}_{row}_{time}');
    const full = buildName(parsed, v).name;
    const { name, trimmed } = buildName(parsed, v, { maxLength: 40 });
    expect(trimmed).toBe(true);
    expect(name.length).toBeLessThanOrEqual(40);
    expect(name.endsWith('_01_12m34s')).toBe(true);
    expect(full.length).toBeGreaterThan(40);
  });

  it('shows an unknown placeholder as typed', () => {
    const s = streak();
    expect(buildName(parseTemplate('{oops}_{row}'), highlightValues(demo([s]), s)).name).toBe('{oops}_01');
  });
});

describe('clipNameFor', () => {
  it('a typed name wins, cleaned; clearing it goes back to the template', () => {
    const s = streak({ clipName: 'anzio: wallbang?' });
    const d = demo([s]);
    expect(clipNameFor(d, s, DEFAULT_TEMPLATE)).toMatchObject({ name: 'anzio_ wallbang_', typed: true });
    delete s.clipName;
    expect(clipNameFor(d, s, DEFAULT_TEMPLATE).name).toBe('anzio_krod_4k_garand-mp40_12m34s');
  });

  it('a template with errors gives the default template\'s name', () => {
    const s = streak();
    expect(automaticClipName(demo([s]), s, '{oops}').name).toBe('anzio_krod_4k_garand-mp40_12m34s');
  });

  it('a project saved before #441 still gets a name', () => {
    const s = streak({ faction: undefined, victims: undefined, victim_factions: undefined });
    const d = demo([s], { map_name: undefined, modified_unix_secs: undefined });
    expect(clipNameFor(d, s, '{map}_{faction}_{player}_{row}').name).toBe('unknown_unknown_krod_01');
  });
});

describe('names for takes', () => {
  it('names each take after its first highlight, uniquely', () => {
    const a = streak();
    const b = streak({ start_tick: 900, kills: [[900, 700, 'Garand']], viewdemo_times: [700], start_index: 0, end_index: 0 });
    const c = streak({ start_tick: 3000, kills: [[3000, 900, 'Garand']], viewdemo_times: [900], end_index: 0 });
    const d = demo([a, b, c]);
    const takeIndex = {
      'sess/dodstudio_chain_01_b0': [streakUid(d.path, a), streakUid(d.path, b)],
      'sess/dodstudio_chain_01_b1': [streakUid(d.path, c)],
      'sess/dodstudio_chain_02_b0': [streakUid(d.path, c)],
      'sess/gone': ['nothing'],
    };
    expect(clipNamesForTakes(takeIndex, [d], '{player}_{kills}k')).toEqual({
      'sess/dodstudio_chain_01_b0': 'krod_1k',
      'sess/dodstudio_chain_01_b1': 'krod_1k_2',
      'sess/dodstudio_chain_02_b0': 'krod_1k_3',
    });
  });

  it('uniqueNames ignores case', () => {
    expect(uniqueNames(['a', 'A', 'a', 'b'])).toEqual(['a', 'A_2', 'a_3', 'b']);
  });

  it('leaves room for the longest export folder', () => {
    expect(maxNameLength(['D:\\Clips\\', 'E:\\Some\\Much\\Longer\\Folder'])).toBe(250 - 26 - 1 - 20);
    expect(timeWord(754.9)).toBe('12m34s');
  });
});
