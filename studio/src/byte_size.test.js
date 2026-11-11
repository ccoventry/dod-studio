import { describe, it, expect } from 'vitest';
import { BYTES_PER_MB, BYTES_PER_GB, gbOrMbText, gigabytes, renderSizeText } from './byte_size.js';

describe('gigabytes', () => {
  it('counts 1024-based gigabytes, as every other size in the app does (#1181)', () => {
    expect(gigabytes(BYTES_PER_GB)).toBe('1.00');
    // 50 decimal gigabytes: "50.00" in a 1e9 count, 46.57 in Explorer's.
    expect(gigabytes(50e9)).toBe('46.57');
  });

  it('rounds to two places', () => {
    expect(gigabytes(1.239 * BYTES_PER_GB)).toBe('1.24');
    expect(gigabytes(0)).toBe('0.00');
  });
});

describe('gbOrMbText', () => {
  it('shows gigabytes to two places from one gigabyte up', () => {
    expect(gbOrMbText(3 * BYTES_PER_GB)).toBe('3.00 GB');
    expect(gbOrMbText(BYTES_PER_GB)).toBe('1.00 GB');
  });

  it('shows megabytes to one place below a gigabyte', () => {
    expect(gbOrMbText(21_870_000)).toBe('20.9 MB');
    expect(gbOrMbText(0)).toBe('0.0 MB');
  });
});

describe('renderSizeText', () => {
  it('shows gigabytes to the places asked for', () => {
    expect(renderSizeText(2.5 * BYTES_PER_GB, 2)).toBe('2.50 GB');
    expect(renderSizeText(2.5 * BYTES_PER_GB, 1)).toBe('2.5 GB');
  });

  it('shows whole megabytes below a gigabyte', () => {
    expect(renderSizeText(512.4 * BYTES_PER_MB, 2)).toBe('512 MB');
  });

  it('counts anything that is not a number as nothing', () => {
    expect(renderSizeText(undefined, 1)).toBe('0 MB');
    expect(renderSizeText('x', 2)).toBe('0 MB');
  });
});
