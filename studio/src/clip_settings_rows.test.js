// The Clip settings rows (#1085, build plan step 8): every row has its text,
// every select its choices, every DoD Studio cvar exists in the hook, and
// every line of the built-in Clean clips has a row to show it.

import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { CLIP_SETTINGS_SECTIONS, CLIP_SETTINGS_ROWS, ROW_CVARS } from './clip_settings_rows.js';
import { DODSTUDIO_NAMES } from './console_commands_data.js';
import { parseSetting } from './clip_settings_logic.js';
import { STRINGS } from './strings.js';

const S = STRINGS.CLIP_SETTINGS;

/** Clean clips' lines, read from the Rust source that ships them. */
function cleanClipsLines() {
  const path = fileURLToPath(new URL('../src-tauri/src/builtin_profiles.rs', import.meta.url));
  const source = readFileSync(path, 'utf8');
  const body = source.slice(source.indexOf('const CLEAN_CLIPS: &[&str] = &['), source.indexOf('];', source.indexOf('const CLEAN_CLIPS: &[&str]')));
  return [...body.matchAll(/^\s*"([^"]+)",/gm)].map((m) => m[1]);
}

describe('CLIP_SETTINGS_ROWS', () => {
  it('gives every row and section its text', () => {
    for (const section of CLIP_SETTINGS_SECTIONS) expect(S.SECTIONS[section.id]).toBeTruthy();
    for (const row of CLIP_SETTINGS_ROWS) {
      expect(S.ROWS[row.id], row.id).toHaveLength(2);
      expect(S.ROWS[row.id][0], row.id).toBeTruthy();
    }
  });

  it('gives every select a label for each value', () => {
    for (const row of CLIP_SETTINGS_ROWS.filter((r) => r.kind === 'select')) {
      expect(S.CHOICES[row.id], row.id).toHaveLength(row.values.length);
    }
  });

  it('has unique ids and cvars', () => {
    const ids = CLIP_SETTINGS_ROWS.map((r) => r.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(new Set(ROW_CVARS).size).toBe(ROW_CVARS.length);
  });

  it('names only DoD Studio cvars the hook has', () => {
    const known = new Set(DODSTUDIO_NAMES.map(([name]) => name));
    for (const cvar of ROW_CVARS.filter((c) => c.startsWith('ds_'))) expect(known.has(cvar), cvar).toBe(true);
  });

  it('shows every line of Clean clips in a row, so picking it fills rows, not Your own commands', () => {
    const lines = cleanClipsLines();
    expect(lines.length).toBeGreaterThan(20);
    for (const line of lines) expect(ROW_CVARS.includes(parseSetting(line)?.name), line).toBe(true);
  });
});
