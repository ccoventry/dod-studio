import { describe, it, expect } from 'vitest';
import {
  shortenPath, isFolderPath, pathSeparator, parentDirOf, ancestorChain, isAncestorOf, demoDateISO,
} from './analyzer_browser_logic.js';

describe('shortenPath', () => {
  it('shows a short path whole, with forward slashes', () => {
    expect(shortenPath('C:\\Demos\\wsod')).toBe('C:/Demos/wsod');
  });

  it('keeps the drive and the last folder of a long path', () => {
    expect(shortenPath('C:\\Users\\someone\\Documents\\Day of Defeat\\DoD Demos')).toBe('C:\\...\\DoD Demos');
  });

  it('cuts the last folder from the left when even that is too long', () => {
    const out = shortenPath('D:\\x\\a-very-long-folder-name-that-will-not-fit-anywhere', 20);
    expect(out.startsWith('D:\\...\\…')).toBe(true);
    expect(out.endsWith('t-anywhere')).toBe(true);
  });

  it('is empty for no path', () => {
    expect(shortenPath('')).toBe('');
    expect(shortenPath(null)).toBe('');
  });
});

describe('isFolderPath', () => {
  it('treats a .dem file in the pinned list as not a folder, in any case', () => {
    expect(isFolderPath('C:\\Demos\\match.dem')).toBe(false);
    expect(isFolderPath('C:\\Demos\\MATCH.DEM')).toBe(false);
    expect(isFolderPath('C:\\Demos')).toBe(true);
  });
});

describe('parentDirOf', () => {
  it('is the folder a Windows or POSIX path is in', () => {
    expect(pathSeparator('C:\\Demos\\a.dem')).toBe('\\');
    expect(pathSeparator('/home/demos/a.dem')).toBe('/');
    expect(parentDirOf('C:\\Demos\\a.dem')).toBe('C:\\Demos');
    expect(parentDirOf('/home/demos/a.dem')).toBe('/home/demos');
  });

  it('is null with nothing above', () => {
    expect(parentDirOf('a.dem')).toBe(null);
    expect(parentDirOf('/a.dem')).toBe(null);
  });
});

describe('ancestorChain', () => {
  it('runs from the drive root down to the path on Windows', () => {
    expect(ancestorChain('C:\\Demos\\wsod')).toEqual(['C:\\', 'C:\\Demos', 'C:\\Demos\\wsod']);
  });

  it('ignores a trailing separator', () => {
    expect(ancestorChain('C:\\Demos\\')).toEqual(['C:\\', 'C:\\Demos']);
  });

  it('runs from / down on POSIX paths', () => {
    expect(ancestorChain('/home/demos')).toEqual(['/home', '/home/demos']);
  });
});

describe('isAncestorOf', () => {
  it('is true only for a folder strictly above', () => {
    expect(isAncestorOf('C:\\Demos', 'C:\\Demos\\wsod')).toBe(true);
    expect(isAncestorOf('C:\\', 'C:\\Demos\\wsod')).toBe(true);
    expect(isAncestorOf('C:\\Demos\\wsod', 'C:\\Demos\\wsod')).toBe(false);
    expect(isAncestorOf('C:\\Other', 'C:\\Demos\\wsod')).toBe(false);
    expect(isAncestorOf('C:\\Demos', null)).toBe(false);
  });
});

describe('demoDateISO', () => {
  it('is the UTC date of the modified time', () => {
    expect(demoDateISO({ modified_unix_secs: 1_700_000_000 })).toBe('2023-11-14');
  });

  it('is empty when the listing has no time', () => {
    expect(demoDateISO({})).toBe('');
    expect(demoDateISO({ modified_unix_secs: 0 })).toBe('');
  });
});
