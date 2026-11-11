import { describe, it, expect } from 'vitest';
import { readFileSync, readdirSync } from 'node:fs';

// Every call into the Rust backend goes through ipc_bridge.js, where each one
// carries its `.catch()` (CLAUDE.md, Tauri IPC). A module that imports
// `invoke` itself skips that wrapper, and its error handling is whatever the
// call site remembered (#1107 found five).
describe('invoke()', () => {
  it('is called only from ipc_bridge.js', () => {
    const dir = new URL('./', import.meta.url);
    const offenders = readdirSync(dir)
      .filter((name) => name.endsWith('.js') && !name.endsWith('.test.js') && name !== 'ipc_bridge.js')
      .filter((name) => /\binvoke\s*\(/.test(readFileSync(new URL(name, dir), 'utf8')));
    expect(offenders).toEqual([]);
  });
});
