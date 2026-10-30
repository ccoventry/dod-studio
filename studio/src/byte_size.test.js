import { describe, it, expect } from 'vitest';
import { BYTES_PER_MB, BYTES_PER_GB, gbOrMbText, renderSizeText } from './byte_size.js';

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
