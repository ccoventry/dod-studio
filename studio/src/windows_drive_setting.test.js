import { describe, it, expect, vi, afterEach } from 'vitest';
import {
  MAX_KEEP_FREE_GB, DEFAULT_KEEP_FREE_GB, keepFreeGbFrom, readWindowsDriveKeepFree, applyWindowsDriveKeepFree,
  usableCaptureBytes, windowsDriveShortWarning,
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

describe("Start Capture Batch's check (#1430 live test)", () => {
  const GIB = 1024 ** 3;

  it('counts the usable space, or the free space when that is unknown', () => {
    expect(usableCaptureBytes(2 * GIB, { free_bytes: 2 * GIB, usable_bytes: 0 })).toBe(0);
    expect(usableCaptureBytes(70 * GIB, { free_bytes: 70 * GIB, usable_bytes: 40 * GIB })).toBe(40 * GIB);
    expect(usableCaptureBytes(5 * GIB, undefined)).toBe(5 * GIB);
    expect(usableCaptureBytes(5 * GIB, null)).toBe(5 * GIB);
  });

  it('a Windows drive below the setting is short of room, not "no free space", and the banner says why', () => {
    const text = windowsDriveShortWarning({ requiredBytes: 10 * GIB, freeBytes: 2 * GIB, usableBytes: 0, keepFreeGb: 50 });
    expect(text).toContain('2.00 GB, below the 50 GB');
    expect(text).toContain('"Keep free on the Windows drive"');
    expect(text).toContain('Configuration → Destinations');
    expect(text).toContain('Lower that setting');
    expect(text).toContain('add a Capture Output folder on another drive');
  });

  it('names what can be used when the setting holds back only part of the room', () => {
    const text = windowsDriveShortWarning({ requiredBytes: 60 * GIB, freeBytes: 70 * GIB, usableBytes: 40 * GIB, keepFreeGb: 50 });
    expect(text).toContain('70.00 GB free, but only 40.00 GB of it can be used');
    expect(text).toContain('keeps 50 GB free on the Windows drive');
  });

  it('says nothing when the setting holds nothing back or the capture fits anyway', () => {
    expect(windowsDriveShortWarning({ requiredBytes: 10 * GIB, freeBytes: 5 * GIB, usableBytes: 5 * GIB, keepFreeGb: 50 })).toBe(null);
    expect(windowsDriveShortWarning({ requiredBytes: 10 * GIB, freeBytes: 70 * GIB, usableBytes: 20 * GIB, keepFreeGb: 50 })).toBe(null);
  });

  it('quotes the default before settings load', () => {
    const text = windowsDriveShortWarning({ requiredBytes: 10 * GIB, freeBytes: 2 * GIB, usableBytes: 0, keepFreeGb: undefined });
    expect(text).toContain(`below the ${DEFAULT_KEEP_FREE_GB} GB`);
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
