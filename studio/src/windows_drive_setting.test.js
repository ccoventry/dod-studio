import { describe, it, expect, vi, afterEach } from 'vitest';
import {
  MAX_KEEP_FREE_GB, keepFreeGbFrom, readWindowsDriveKeepFree, applyWindowsDriveKeepFree,
} from './windows_drive_setting.js';

describe('Keep free on the Windows drive (#1430)', () => {
  it('takes a whole number of GB', () => {
    expect(keepFreeGbFrom('50', null)).toBe(50);
    expect(keepFreeGbFrom(' 120 ', null)).toBe(120);
    expect(keepFreeGbFrom('20.6', null)).toBe(21);
    expect(keepFreeGbFrom(75, null)).toBe(75);
  });

  it('takes 0, which turns the setting off', () => {
    expect(keepFreeGbFrom('0', 50)).toBe(0);
  });

  it('clamps to 0 and the largest value the backend takes', () => {
    expect(keepFreeGbFrom('-5', 50)).toBe(0);
    expect(keepFreeGbFrom('999999', 50)).toBe(MAX_KEEP_FREE_GB);
  });

  it('keeps the fallback for a blank or garbled field', () => {
    expect(keepFreeGbFrom('', 50)).toBe(50);
    expect(keepFreeGbFrom('lots', 30)).toBe(30);
    expect(keepFreeGbFrom(undefined, 40)).toBe(40);
    expect(keepFreeGbFrom(null, null)).toBe(null);
  });
});

describe('the field', () => {
  afterEach(() => vi.unstubAllGlobals());

  function stubField(value) {
    const input = { value };
    vi.stubGlobal('document', { querySelector: (sel) => (sel === '#config-windows-drive-keep-free' ? input : null) });
    return input;
  }

  it('shows the loaded value and saves what is typed', () => {
    const input = stubField('');
    applyWindowsDriveKeepFree(80);
    expect(input.value).toBe('80');
    input.value = '25';
    expect(readWindowsDriveKeepFree()).toBe(25);
  });

  it('saves the loaded value when the field is cleared', () => {
    const input = stubField('');
    applyWindowsDriveKeepFree(60);
    input.value = '';
    expect(readWindowsDriveKeepFree()).toBe(60);
  });
});
