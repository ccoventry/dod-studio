import { describe, it, expect } from 'vitest';
import {
  PROJECT_EXTENSION, projectSaveDialogOptions, PROJECT_OPEN_FILTERS, isProjectFile, isDemoFile,
} from './project_file_type.js';

// Project files are `.dodstudio`, and `.json` projects keep loading (#528).

describe('the first save', () => {
  it('defaults to a .dodstudio file in the projects folder', () => {
    const options = projectSaveDialogOptions('C:\\Users\\x\\Documents\\dod-studio\\projects');
    expect(PROJECT_EXTENSION).toBe('dodstudio');
    expect(options.defaultPath).toBe('C:\\Users\\x\\Documents\\dod-studio\\projects\\dod_project.dodstudio');
    expect(options.filters[0].extensions).toEqual(['dodstudio']);
  });

  it('still offers .json, second', () => {
    expect(projectSaveDialogOptions(null).filters.map((f) => f.extensions)).toEqual([['dodstudio'], ['json']]);
  });

  it('names just the file when there is no projects folder', () => {
    expect(projectSaveDialogOptions(null).defaultPath).toBe('dod_project.dodstudio');
  });
});

describe('Load Project', () => {
  it('lists both kinds of project file under one filter', () => {
    expect(PROJECT_OPEN_FILTERS).toHaveLength(1);
    expect(PROJECT_OPEN_FILTERS[0].extensions).toEqual(['dodstudio', 'json']);
  });
});

describe('file kinds', () => {
  it('a project is .dodstudio or .json, any case', () => {
    expect(isProjectFile('D:\\Movies\\WSOD25.dodstudio')).toBe(true);
    expect(isProjectFile('D:/Movies/old.JSON')).toBe(true);
    expect(isProjectFile('D:/Movies/match.dem')).toBe(false);
    expect(isProjectFile('D:/Movies.dodstudio/readme')).toBe(false);
    expect(isProjectFile('')).toBe(false);
  });

  it('a demo is .dem', () => {
    expect(isDemoFile('D:\\demos\\match.DEM')).toBe(true);
    expect(isDemoFile('D:\\demos\\match.dem.json')).toBe(false);
    expect(isDemoFile('D:\\demos\\.dem')).toBe(false);
  });
});
