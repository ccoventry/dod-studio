import { describe, it, expect } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  suggestions, typedName, acceptSuggestion, tierNote, droppedFromDemo,
  OWNED_BY_STUDIO, GAME_QUITS_OVER, SCHEDULED_BANNED, MID_DEMO_HAZARDS, NOOP_IN_INIT,
  DEMO_FILTER_NAME_CONTAINS, DEMO_FILTER_NAME_STARTS_WITH, DEMO_FILTER_LINE_STARTS_WITH,
  DEMO_FILTER_LINE_CONTAINS, DEMO_FILTER_WORD_STARTS_WITH,
} from './command_suggest.js';
import { DODSTUDIO_NAMES } from './console_commands_data.js';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

describe('suggestions (#215)', () => {
  it('matches the start of a name, an exact match first, any case', () => {
    const names = suggestions('mirv_movie_f').map((s) => s.name);
    expect(names).toEqual(expect.arrayContaining(['mirv_movie_ffmpeg', 'mirv_movie_filename', 'mirv_movie_fps']));
    expect(names.every((n) => n.startsWith('mirv_movie_f'))).toBe(true);
    expect(suggestions('FPS_MAX')[0].name).toBe('fps_max');
    expect(suggestions('sensitivity')[0].name).toBe('sensitivity');
    expect(suggestions('')).toEqual([]);
    expect(suggestions('zzzz_not_a_name')).toEqual([]);
  });

  it('knows the game, HLAE and DoD Studio names', () => {
    const one = (p) => suggestions(p, { limit: 1 })[0];
    expect(one('cl_xhair_style')).toMatchObject({ source: 'game', kind: 'cvar' });
    expect(one('viewdemo')).toMatchObject({ source: 'game', kind: 'cmd' });
    expect(one('mirv_fov')).toMatchObject({ source: 'hlae' });
    expect(one('dodstudio_spec_lock')).toMatchObject({ source: 'dodstudio' });
    expect(one('gl_use_shaders')).toMatchObject({ builds: 'post' });
  });

  it('stops at the limit', () => {
    expect(suggestions('cl_', { limit: 5 })).toHaveLength(5);
  });

  it('says what Studio does with a name, per list', () => {
    expect(tierNote('host_framerate', { scheduled: false }).level).toBe('refused');
    expect(tierNote('cl_lw', { scheduled: true }).level).toBe('refused');
    expect(tierNote('r_decals', { scheduled: true }).level).toBe('refused');
    expect(tierNote('r_decals', { scheduled: false }).level).toBe('warned');
    expect(tierNote('mirv_movie_filename', { scheduled: false }).level).toBe('noop');
    expect(tierNote('exec', { scheduled: true }).level).toBe('noop');
    expect(tierNote('sensitivity', { scheduled: true })).toBeNull();
  });

  it("says when the game drops a name from a demo (#679), in either list", () => {
    for (const scheduled of [false, true]) {
      const note = tierNote('mirv_matte_setcolor', { scheduled });
      expect(note.level).toBe('noop');
      expect(note.text).toContain("'_set'");
    }
    expect(tierNote('kill', { scheduled: true }).text).toContain("'kill'");
  });
});

describe('droppedFromDemo (#679)', () => {
  it('finds a filtered piece anywhere in the name, any case', () => {
    expect(droppedFromDemo('mirv_matte_setcolor')).toBe('_set');
    expect(droppedFromDemo('MIRV_Draw_SV_Hitboxes_SetUColor')).toBe('_set');
    expect(droppedFromDemo('cl_killsound')).toBe('kill');
    expect(droppedFromDemo('unbindall')).toBe('bind');
    expect(droppedFromDemo('writecfg')).toBe('writecfg');
  });

  it('drops a name starting with connect, but not reconnect', () => {
    expect(droppedFromDemo('connect')).toBe('connect');
    expect(droppedFromDemo('reconnect')).toBeNull();
  });

  it('drops alias and anything with exec in it', () => {
    expect(droppedFromDemo('alias')).toBe('alias');
    expect(droppedFromDemo('aliases')).toBeNull();
    expect(droppedFromDemo('exec')).toBe('exec');
  });

  it('lets ordinary names through', () => {
    for (const name of ['echo', 'sensitivity', 'mirv_fov', 'spec_autodirector', 'dodstudio_spec_lock']) {
      expect(droppedFromDemo(name)).toBeNull();
    }
  });

  it('never fires for a name another tier already covers', () => {
    for (const name of [...OWNED_BY_STUDIO, ...GAME_QUITS_OVER, ...SCHEDULED_BANNED, ...MID_DEMO_HAZARDS, ...NOOP_IN_INIT]) {
      expect(droppedFromDemo(name)).toBeNull();
    }
  });
});

describe('typing', () => {
  it('suggests only while the cursor is in the first word', () => {
    expect(typedName('mirv_fo')).toBe('mirv_fo');
    expect(typedName('  r_dec')).toBe('r_dec');
    expect(typedName('mirv_fov 90')).toBeNull();
    expect(typedName('mirv_fov 90', 4)).toBe('mirv');
  });

  it('a taken name replaces the first word and keeps the rest', () => {
    expect(acceptSuggestion('mirv_fo', 'mirv_fov')).toBe('mirv_fov ');
    expect(acceptSuggestion('mirv 90', 'mirv_fov')).toBe('mirv_fov 90');
  });
});

describe('kept in step with the code', () => {
  const rustList = (source, name) => {
    const body = source.match(new RegExp(`pub const ${name}: &\\[&str\\] = &\\[([^\\]]*)\\]`))[1];
    return [...body.matchAll(/"([^"]+)"/g)].map((m) => m[1]).sort();
  };
  const cfgScan = fs.readFileSync(path.join(repo, 'native', 'src', 'patch', 'cfg_scan.rs'), 'utf8');

  it("matches native::patch::cfg_scan's tiers", () => {
    expect([...OWNED_BY_STUDIO, ...GAME_QUITS_OVER].sort()).toEqual(rustList(cfgScan, 'BANNED_COMMANDS'));
    expect([...SCHEDULED_BANNED].sort()).toEqual(rustList(cfgScan, 'SCHEDULED_BANNED_COMMANDS'));
    expect([...MID_DEMO_HAZARDS].sort()).toEqual(rustList(cfgScan, 'MID_DEMO_HAZARDS'));
    expect([...DEMO_FILTER_NAME_CONTAINS].sort()).toEqual(rustList(cfgScan, 'DEMO_FILTER_NAME_CONTAINS'));
    expect([...DEMO_FILTER_NAME_STARTS_WITH].sort()).toEqual(rustList(cfgScan, 'DEMO_FILTER_NAME_STARTS_WITH'));
    expect([...DEMO_FILTER_LINE_STARTS_WITH].sort()).toEqual(rustList(cfgScan, 'DEMO_FILTER_LINE_STARTS_WITH'));
    expect([...DEMO_FILTER_LINE_CONTAINS].sort()).toEqual(rustList(cfgScan, 'DEMO_FILTER_LINE_CONTAINS'));
    expect([...DEMO_FILTER_WORD_STARTS_WITH].sort()).toEqual(rustList(cfgScan, 'DEMO_FILTER_WORD_STARTS_WITH'));
    expect([...NOOP_IN_INIT].sort()).toEqual(rustList(cfgScan, 'NOOP_IN_INIT_COMMANDS'));
  });

  it("has every console_name! in goldsrc-hooks (re-run goldsrc-hooks/tools/console_names.py when this fails)", () => {
    const names = new Set();
    const walk = (dir) => fs.readdirSync(dir, { withFileTypes: true }).forEach((e) => {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (p.endsWith('.rs')) {
        // Code only, as console_names.py reads it: names.rs's doc comment shows the macro with made-up names.
        const code = fs.readFileSync(p, 'utf8').split('\n').filter((l) => !l.trimStart().startsWith('//')).join('\n');
        for (const m of code.matchAll(/console_name!\("([a-z0-9_]+)"\)/g)) names.add(`dodstudio_${m[1]}`);
      }
    });
    walk(path.join(repo, 'goldsrc-hooks', 'src'));
    const listed = new Set(DODSTUDIO_NAMES.map(([n]) => n));
    expect([...names].filter((n) => !listed.has(n))).toEqual([]);
  });
});
