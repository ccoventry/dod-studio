import { describe, it, expect } from 'vitest';
import { hookDllRowShown } from './hook_dll_row.js';

describe('GoldSrc Hooks DLL row', () => {
  it('shows in a local or debug build, set or not', () => {
    expect(hookDllRowShown(true, '')).toBe(true);
    expect(hookDllRowShown(true, 'C:\\dev\\dodstudio_goldsrc_hooks.dll')).toBe(true);
  });

  it('shows in a release build only while a path is set, so it can be cleared', () => {
    expect(hookDllRowShown(false, '')).toBe(false);
    expect(hookDllRowShown(false, '   ')).toBe(false);
    expect(hookDllRowShown(false, 'C:\\dev\\dodstudio_goldsrc_hooks.dll')).toBe(true);
  });
});
