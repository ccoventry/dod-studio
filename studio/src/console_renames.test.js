import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { OLD_NAMES, currentName } from './console_renames.js';
import { DODSTUDIO_NAMES } from './console_commands_data.js';

describe('console_renames (#555)', () => {
  it('matches the hook\'s old_names.rs, the table the Rust side uses', () => {
    const rust = readFileSync(
      fileURLToPath(new URL('../../goldsrc-hooks/src/old_names.rs', import.meta.url)),
      'utf8',
    );
    const pairs = Object.fromEntries(
      [...rust.matchAll(/\("(dodstudio_[a-z0-9_]+)", "(ds_[a-z0-9_]+)"\)/g)].map((m) => [m[1], m[2]]),
    );
    expect(Object.keys(pairs).length).toBeGreaterThan(0);
    expect({ ...OLD_NAMES }).toEqual(pairs);
  });

  it('maps every old name to a name the hook registers', () => {
    const listed = new Set(DODSTUDIO_NAMES.map(([n]) => n));
    for (const [old, name] of Object.entries(OLD_NAMES)) {
      expect(listed.has(name), `${old} -> ${name}`).toBe(true);
    }
  });

  it('reads an old name in any case, and nothing else', () => {
    expect(currentName('DodStudio_Pov_Live_Bob')).toBe('ds_view_bob');
    expect(currentName('dodstudio_hd')).toBeNull();
    expect(currentName('ds_view_bob')).toBeNull();
  });
});
