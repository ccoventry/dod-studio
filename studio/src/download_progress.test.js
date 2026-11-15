import { describe, it, expect } from 'vitest';
import { megabytes, bytesText, updateDownloadLine, mapDownloadLine } from './download_progress.js';

const MB = 1024 * 1024;

describe('megabytes', () => {
  it('keeps one decimal under 10 MB and drops a trailing .0', () => {
    expect(megabytes(2.1 * MB)).toBe('2.1');
    expect(megabytes(6 * MB)).toBe('6');
    expect(megabytes(0)).toBe('0');
    expect(megabytes(undefined)).toBe('0');
  });

  it('rounds to whole MB from 10 up', () => {
    expect(megabytes(12.4 * MB)).toBe('12');
    expect(megabytes(44.6 * MB)).toBe('45');
  });
});

describe('bytesText', () => {
  it('says how much of how much', () => {
    expect(bytesText(12 * MB, 45 * MB)).toBe('12 of 45 MB');
  });

  it('gives just the count when the server stated no size', () => {
    expect(bytesText(3 * MB, null)).toBe('3 MB');
  });
});

describe('updateDownloadLine', () => {
  it('names the update and its MB', () => {
    expect(updateDownloadLine({ downloaded: 12 * MB, total: 45 * MB })).toBe('Downloading the update: 12 of 45 MB');
  });

  it('reads plainly before the first byte', () => {
    expect(updateDownloadLine({ downloaded: 0, total: 45 * MB })).toBe('Downloading the update…');
    expect(updateDownloadLine(null)).toBe('Downloading the update…');
  });
});

describe('mapDownloadLine', () => {
  it('counts the maps and their MB', () => {
    expect(mapDownloadLine('dod_x', 1, 3, { step: 'downloading', downloaded: 2.1 * MB, total: 6 * MB }))
      .toBe('Downloading dod_x (1 of 3): 2.1 of 6 MB');
  });

  it('leaves the count off for one map, and the MB off before the first byte', () => {
    expect(mapDownloadLine('dod_x', 1, 1, null)).toBe('Downloading dod_x…');
    expect(mapDownloadLine('dod_x', 2, 3, null)).toBe('Downloading dod_x (2 of 3)…');
  });

  it('says when it checks the map', () => {
    expect(mapDownloadLine('dod_x', 1, 3, { step: 'checking', downloaded: 6 * MB, total: 6 * MB })).toBe('Checking dod_x…');
  });
});
