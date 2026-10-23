import { describe, it, expect, vi } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ listHookDlls: vi.fn() }));

import { pickerOptions, selectedValue, shortDate, resolveTyped, filterOptions } from './hook_dll_picker.js';
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

describe('resolveTyped', () => {
  const options = [
    { value: '', text: 'Bundled (none in a local dev build)' },
    { value: 'a', text: '910 · Oct 22 17:54' },
    { value: 'b', text: '910b · Oct 22 18:01' },
    { value: 'c', text: '889 · Oct 22 17:54' },
    { value: 'd', text: 'capture-wsod25d · Oct 18 02:10' },
  ];
  const pick = (typed) => resolveTyped(options, typed)?.value ?? null;

  it('takes the picked row, its label alone, or the one row that contains it', () => {
    expect(pick('910 · Oct 22 17:54')).toBe('a');
    expect(pick('910')).toBe('a');
    expect(pick('910B')).toBe('b');
    expect(pick('wsod')).toBe('d');
    expect(pick('bundled')).toBe('');
  });

  it('is null for nothing typed, no match, or several', () => {
    expect(pick('')).toBeNull();
    expect(pick('   ')).toBeNull();
    expect(pick('999')).toBeNull();
    expect(pick('91')).toBeNull();
    expect(pick('Oct 22')).toBeNull();
  });
});

describe('filterOptions', () => {
  const options = [
    { value: '', text: 'Bundled (none in a local dev build)' },
    { value: 'a', text: '910 · Oct 22 17:54' },
    { value: 'b', text: '918 · Oct 22 01:10' },
    { value: 'c', text: '889 · Oct 22 17:54' },
  ];
  const values = (typed, current = '') => filterOptions(options, typed, current).map((o) => o.value);

  it('keeps the rows that contain what is typed, in any case', () => {
    expect(values('91')).toEqual(['', 'a', 'b']);
    expect(values('BUNDLED')).toEqual(['']);
    expect(values('')).toEqual(['', 'a', 'b', 'c']);
  });

  it('always keeps the saved pick, so the dropdown still shows it', () => {
    expect(values('91', 'c')).toEqual(['a', 'b', 'c']);
    expect(values('nothing', 'c')).toEqual(['c']);
  });
});
