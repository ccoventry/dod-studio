import { describe, it, expect } from 'vitest';
import {
  copyNameProblem, suggestCopyName, copyBytes, copyFits, copyCheck, progressPercent, progressLine,
} from './movie_copy.js';

const GB = 1024 ** 3;
// About this PC's stock dod (#939): 15 GB without demos, 29 GB of demos.
const plan = (over = {}) => ({
  files: 4422, bytes: 15 * GB, demo_files: 424, demo_bytes: 29 * GB,
  free_bytes: 100 * GB, headroom_bytes: GB,
  existing: ['dod', 'valve', 'dod_addon', 'hl.exe'], default_name: 'dod_movies',
  ...over,
});

describe('copyNameProblem', () => {
  it('takes letters, digits and _', () => {
    expect(copyNameProblem('dod_movies', ['dod'])).toBe('');
    expect(copyNameProblem('Movies2', [])).toBe('');
  });

  it('refuses an empty name and anything else', () => {
    expect(copyNameProblem('', [])).toBe('Give the copy a name.');
    for (const bad of ['dod movies', 'dod-movies', '..', 'a\\b', 'a/b', 'dod.movies', 'filmé']) {
      expect(copyNameProblem(bad, []), bad).toBe('Letters, digits and _ only.');
    }
  });

  it("refuses Windows' device names", () => {
    expect(copyNameProblem('CON', [])).toMatch(/Windows doesn't allow/);
    expect(copyNameProblem('lpt3', [])).toMatch(/Windows doesn't allow/);
    expect(copyNameProblem('com10', [])).toBe('');
  });

  it('refuses a name already in the Half-Life folder, without case', () => {
    expect(copyNameProblem('DOD', ['dod', 'valve'])).toBe(
      "There's already a dod in your Half-Life folder: pick another name.",
    );
    expect(copyNameProblem('Valve', ['dod', 'valve'])).toMatch(/already a valve/);
  });
});

describe('suggestCopyName', () => {
  it('suggests dod_movies, or the next free number', () => {
    expect(suggestCopyName(['dod'])).toBe('dod_movies');
    expect(suggestCopyName(['dod', 'DOD_MOVIES'])).toBe('dod_movies2');
    expect(suggestCopyName(['dod_movies', 'dod_movies2'])).toBe('dod_movies3');
  });
});

describe('copyBytes and copyFits', () => {
  it('leaves the demos out unless asked', () => {
    expect(copyBytes(plan(), false)).toBe(15 * GB);
    expect(copyBytes(plan(), true)).toBe(44 * GB);
  });

  it('keeps the headroom free', () => {
    expect(copyFits(plan({ free_bytes: 16 * GB }), false)).toBe(true);
    expect(copyFits(plan({ free_bytes: 16 * GB - 1 }), false)).toBe(false);
    expect(copyFits(plan({ free_bytes: 30 * GB }), true)).toBe(false);
  });

  it("doesn't block when the free space is unknown", () => {
    expect(copyFits(plan({ free_bytes: null }), true)).toBe(true);
  });
});

describe('copyCheck', () => {
  it('shows the size, the demos it leaves out and the free space', () => {
    const c = copyCheck(plan(), 'dod_movies', false);
    expect(c.sizeLine).toBe('4422 files, 15 GB to copy.');
    expect(c.demosLine).toBe('Demos: 424 demos, 29 GB.');
    expect(c.spaceLine).toBe('100 GB free on this drive.');
    expect(c.canCopy).toBe(true);
  });

  it('counts the demos in when ticked', () => {
    expect(copyCheck(plan(), 'dod_movies', true).sizeLine).toBe('4846 files, 44 GB to copy.');
  });

  it("refuses a copy that doesn't fit, and says so", () => {
    const c = copyCheck(plan({ free_bytes: 40 * GB }), 'dod_movies', true);
    expect(c.fits).toBe(false);
    expect(c.canCopy).toBe(false);
    expect(c.spaceLine).toBe(
      'Not enough space: the copy needs 44 GB and this drive has 40 GB free (DoD Studio leaves 1 GB spare). Free some space, or leave the demos out.',
    );
  });

  it('refuses a bad or taken name', () => {
    expect(copyCheck(plan(), 'dod', false).canCopy).toBe(false);
    expect(copyCheck(plan(), 'dod movies', false).nameProblem).toBe('Letters, digits and _ only.');
  });

  it("can't copy before the folder is measured", () => {
    expect(copyCheck(null, 'dod_movies', false).canCopy).toBe(false);
  });

  it('says when the free space is unknown, and still copies', () => {
    const c = copyCheck(plan({ free_bytes: null }), 'dod_movies', false);
    expect(c.spaceLine).toBe("Couldn't tell how much space this drive has free.");
    expect(c.canCopy).toBe(true);
  });
});

describe('progress', () => {
  it('fills the bar by bytes and names the file', () => {
    const p = { done_bytes: 3 * GB, total_bytes: 12 * GB, done_files: 10, total_files: 100, file: 'maps\\dod_anzio.bsp' };
    expect(progressPercent(p)).toBe(25);
    expect(progressLine(p)).toBe('Copied 3.0 GB of 12 GB: maps\\dod_anzio.bsp');
  });

  it('is full for an empty copy that finished, empty before the first event', () => {
    expect(progressPercent({ done_bytes: 0, total_bytes: 0, done_files: 0, total_files: 0 })).toBe(100);
    expect(progressPercent(null)).toBe(0);
    expect(progressLine(null)).toBe('Starting the copy…');
  });
});
