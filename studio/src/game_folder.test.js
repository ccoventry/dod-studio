import { describe, it, expect } from 'vitest';
import {
  DEFAULT_GAME_FOLDER, gameFolderProblem, gameFolderOptions, languageFileNote, languageFileButton,
} from './game_folder.js';
import { STRINGS } from './strings.js';

describe('gameFolderProblem (#402)', () => {
  it('accepts plain folder names', () => {
    for (const name of ['dod', 'dod_movies', 'dod_3_1_beta', 'dod_1.2', 'valve']) {
      expect(gameFolderProblem(name), name).toBe('');
    }
  });

  // The same set native::shared::paths::validate_game_folder refuses: each
  // would be a path, or a launch line the engine reads another way.
  it('refuses paths, launch options and blanks', () => {
    for (const name of [
      '', '   ', ' dod', 'dod ', 'dod movies', '.', '..', '../dod', 'dod/maps',
      'dod\\maps', 'C:\\Games\\dod', 'C:dod', 'dod?', '-insecure', '+map', 'dod\u0000',
      undefined, null,
    ]) {
      expect(gameFolderProblem(name), JSON.stringify(name)).not.toBe('');
    }
  });
});

describe('gameFolderOptions (#402)', () => {
  it('lists the folders found and selects the saved one', () => {
    expect(gameFolderOptions(['dod', 'dod_movies'], 'dod_movies')).toEqual([
      { value: 'dod', missing: false, selected: false },
      { value: 'dod_movies', missing: false, selected: true },
    ]);
  });

  it('matches the saved folder without case, as Windows does', () => {
    const options = gameFolderOptions(['DOD', 'dod_movies'], 'dod');
    expect(options.find((o) => o.selected)).toEqual({ value: 'DOD', missing: false, selected: true });
    expect(options).toHaveLength(2);
  });

  it('keeps a saved folder that is not there, marked, rather than swapping it', () => {
    expect(gameFolderOptions(['dod'], 'dod_movies')).toEqual([
      { value: 'dod_movies', missing: true, selected: true },
      { value: 'dod', missing: false, selected: false },
    ]);
  });

  it('falls back to dod with nothing saved and nothing found', () => {
    expect(gameFolderOptions([], '')).toEqual([
      { value: DEFAULT_GAME_FOLDER, missing: true, selected: true },
    ]);
    expect(gameFolderOptions(undefined, undefined)[0].value).toBe('dod');
  });
});

describe('languageFileNote (#925)', () => {
  it('names the file to rename and its new name', () => {
    const note = languageFileNote({
      folder: 'dod_movies', language: 'english', needed: 'dod_movies_english.txt', rename_from: 'dod_english.txt',
    });
    expect(note).toContain('dod_movies has no resource');
    expect(note).toContain('dod_movies_english.txt');
    expect(note).toContain('rename dod_english.txt to dod_movies_english.txt');
    expect(note).toContain('#Dod_Spec_Allies_Score');
  });

  it('says where to copy it from when the folder has nothing to rename', () => {
    const note = languageFileNote({
      folder: 'movies', language: 'french', needed: 'movies_french.txt', rename_from: null,
    });
    expect(note).toContain('dod_french.txt');
    expect(note).toContain('name it movies_french.txt');
  });

  it('is empty when nothing is missing', () => {
    expect(languageFileNote(null)).toBe('');
    expect(languageFileNote(undefined)).toBe('');
  });
});

describe('languageFileButton (#925)', () => {
  it('shows the file when there is one to rename, else opens the folder', () => {
    expect(languageFileButton({ show: 'C:/x/dod_english.txt', rename_from: 'dod_english.txt' }))
      .toBe(STRINGS.CAPTURE_CONFIG.LANGUAGE_FILE_SHOW);
    expect(languageFileButton({ show: 'C:/x/resource', rename_from: null }))
      .toBe(STRINGS.CAPTURE_CONFIG.LANGUAGE_FILE_OPEN_FOLDER);
    expect(languageFileButton(null)).toBe('');
  });
});
