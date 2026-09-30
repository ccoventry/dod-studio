import { describe, it, expect } from 'vitest';
import { projectFolders, pinnedFoldersOnly } from './project_paths.js';

describe('projectFolders', () => {
  it('lists each demo folder once, in first-seen order', () => {
    const demos = [
      { path: 'C:\\dod\\test stuff\\a.dem' },
      { path: 'D:\\Demos\\b.dem' },
      { path: 'c:/dod/TEST STUFF/c.dem' },
      { path: 'C:\\dod\\test stuff\\sub\\d.dem' },
      {},
    ];
    expect(projectFolders(demos)).toEqual([
      'C:\\dod\\test stuff',
      'D:\\Demos',
      'C:\\dod\\test stuff\\sub',
    ]);
  });

  it('is empty for no demos', () => {
    expect(projectFolders([])).toEqual([]);
    expect(projectFolders(undefined)).toEqual([]);
  });
});

describe('pinnedFoldersOnly', () => {
  it('drops single demo files and repeats, and keeps folders', () => {
    expect(pinnedFoldersOnly([
      'C:\\dod',
      'C:\\dod\\a.dem',
      'H:\\DoD Demos',
      'c:/dod/',
      'D:\\x\\B.DEM',
      '',
    ])).toEqual(['C:\\dod', 'H:\\DoD Demos']);
  });
});
