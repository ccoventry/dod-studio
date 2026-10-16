import { describe, it, expect } from 'vitest';
import { DEFAULT_GAME_FOLDER, gameFolderProblem, gameFolderOptions } from './game_folder.js';

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
