import { describe, it, expect, vi } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ listHookDlls: vi.fn() }));

import { pickerOptions, selectedValue, shortDate } from './hook_dll_picker.js';
import { STRINGS } from './strings.js';

const S = STRINGS.CAPTURE_CONFIG;
const list = {
  folder: 'C:\\repo\\local\\hook-dlls',
  checkout_build: 'C:\\repo\\target\\i686-pc-windows-msvc\\release\\dodstudio_goldsrc_hooks.dll',
  dlls: [
    { label: '889', path: 'C:\\repo\\local\\test-889\\dodstudio_goldsrc_hooks.dll', modified_unix_secs: null },
    { label: '910', path: 'C:\\repo\\local\\hook-dlls\\dodstudio_goldsrc_hooks_910.dll', modified_unix_secs: null },
  ],
};

describe('pickerOptions', () => {
  it('lists the bundled DLL, this checkout\'s build, then each test DLL by its label', () => {
    const options = pickerOptions(list, '');
    expect(options.map((o) => o.text)).toEqual([S.HOOK_DLL_BUNDLED, S.HOOK_DLL_CHECKOUT, '889', '910']);
    expect(options[0].value).toBe('');
    expect(options[3].title).toBe(list.dlls[1].path);
  });

  it('keeps a saved path that is none of them, by its folder and name', () => {
    const options = pickerOptions(list, 'D:/elsewhere/mine/dodstudio_goldsrc_hooks.dll');
    expect(options.at(-1).text).toBe(S.hookDllOther('mine\\dodstudio_goldsrc_hooks.dll'));
    expect(options.at(-1).value).toBe('D:/elsewhere/mine/dodstudio_goldsrc_hooks.dll');
  });

  it('matches the saved path whatever its slashes or case', () => {
    const saved = 'c:/repo/LOCAL/test-889/dodstudio_goldsrc_hooks.dll';
    const options = pickerOptions(list, saved);
    expect(options).toHaveLength(4);
    expect(selectedValue(options, saved)).toBe(list.dlls[0].path);
    expect(selectedValue(options, '')).toBe('');
  });

  it('works with no checkout and no DLLs', () => {
    expect(pickerOptions({ folder: null, checkout_build: null, dlls: [] }, '').map((o) => o.value)).toEqual(['']);
    expect(pickerOptions(null, '').map((o) => o.value)).toEqual(['']);
  });

  it('adds the file\'s date after the label', () => {
    const secs = new Date(2026, 9, 22, 17, 54).getTime() / 1000;
    const options = pickerOptions({ dlls: [{ label: '924', path: 'x.dll', modified_unix_secs: secs }] }, '');
    expect(options[1].text).toBe('924 · Oct 22 17:54');
    expect(shortDate(null)).toBe('');
  });
});

describe('pickerOptions order', () => {
  it('puts the newest test DLL first, after the bundled and checkout rows', () => {
    const dll = (label, secs) => ({ label, path: `${label}.dll`, modified_unix_secs: secs });
    const options = pickerOptions({
      checkout_build: 'target.dll',
      dlls: [dll('402b', 100), dll('555', 300), dll('889', null), dll('910', 200)],
    }, '');
    expect(options.map((o) => o.value)).toEqual(['', 'target.dll', '555.dll', '910.dll', '402b.dll', '889.dll']);
  });
});
