import { describe, it, expect } from 'vitest';
import {
  shownPath, addFolders, auditFolders, canStartAudit, allDrivesLabel, auditProgressView,
  deleteProgressView, startsTicked, withoutDeleted, auditTotals, wastedSizeText, fileStatus,
} from './audit_duplicates.js';

describe('audit_duplicates (#844)', () => {
  it('shows paths without the verbatim prefix', () => {
    expect(shownPath('\\\\?\\C:\\demos\\a.dem')).toBe('C:\\demos\\a.dem');
    expect(shownPath('\\\\?\\UNC\\nas\\d\\a.dem')).toBe('\\\\nas\\d\\a.dem');
    expect(shownPath('C:\\a.dem')).toBe('C:\\a.dem');
  });

  it('adds picked folders once, ignoring case, slashes and blanks', () => {
    expect(addFolders(['C:\\Demos'], ['c:\\demos\\', 'F:\\', ' ', null, 'F:/'])).toEqual(['C:\\Demos', 'F:\\']);
    expect(addFolders([], 'D:\\one')).toEqual(['D:\\one']);
    expect(addFolders(['D:\\one'], null)).toEqual(['D:\\one']);
  });

  it('puts the Target Folder first and can start with folders or All drives', () => {
    expect(auditFolders(' C:\\t ', ['F:\\x', 'c:\\t'])).toEqual(['C:\\t', 'F:\\x']);
    expect(auditFolders('', ['F:\\x'])).toEqual(['F:\\x']);
    expect(canStartAudit('', [], false)).toBe(false);
    expect(canStartAudit('', [], true)).toBe(true);
    expect(canStartAudit('', ['F:\\x'], false)).toBe(true);
  });

  it('names the drives All drives will check', () => {
    expect(allDrivesLabel(['C:\\', 'F:\\'])).toBe('All drives (C:, F:)');
    expect(allDrivesLabel([])).toBe('All drives');
    expect(allDrivesLabel(undefined)).toBe('All drives');
  });

  it('says which step the audit is on, with a bar once there is a total', () => {
    expect(auditProgressView(null)).toEqual({ pct: 0, text: 'Looking for demos: 0 folders read', current: '' });
    expect(auditProgressView({ folders: 1, total: 0, done: 0, current: '\\\\?\\C:\\x' }))
      .toEqual({ pct: 0, text: 'Looking for demos: 1 folder read', current: 'C:\\x' });
    expect(auditProgressView({ folders: 9, total: 8, done: 2, current: 'C:\\a.dem' }))
      .toEqual({ pct: 25, text: 'Comparing demos: 2 / 8', current: 'C:\\a.dem' });
    expect(auditProgressView({ folders: 9, total: 8, done: 12 }).pct).toBe(100);
  });

  it('words the delete progress by where the files go', () => {
    expect(deleteProgressView({ done: 1, total: 4 }, false).text).toBe('Moving to the Recycle Bin: 1 / 4');
    expect(deleteProgressView({ done: 4, total: 4 }, true)).toEqual({ pct: 100, text: 'Deleting permanently: 4 / 4', current: '' });
    expect(deleteProgressView({ done: 0, total: 0 }, false).pct).toBe(0);
  });

  it('never ticks the kept copy or a copy the game or the project uses', () => {
    expect(startsTicked({ path: 'a', in_use: null }, 0)).toBe(false);
    expect(startsTicked({ path: 'b', in_use: null }, 1)).toBe(true);
    expect(startsTicked({ path: 'c', in_use: 'queue' }, 2)).toBe(false);
  });

  it('drops deleted copies and groups left with one file', () => {
    const groups = [
      { size: 10, header_hash: 1, files: [{ path: 'a1' }, { path: 'a2' }] },
      { size: 20, header_hash: 2, files: [{ path: 'b1' }, { path: 'b2' }, { path: 'b3' }] },
    ];
    expect(withoutDeleted(groups, ['a2', 'b3'])).toEqual([
      { size: 20, header_hash: 2, files: [{ path: 'b1' }, { path: 'b2' }] },
    ]);
    expect(auditTotals(groups)).toEqual({ copies: 3, wastedBytes: 50 });
  });

  it('labels each row kept, a copy, or a copy in use', () => {
    expect(fileStatus({ in_use: null }, 0)).toBe('Kept');
    expect(fileStatus({ in_use: 'game_folder' }, 0)).toBe('Kept (in the game folder)');
    expect(fileStatus({ in_use: null }, 1)).toBe('   ↳ Copy');
    expect(fileStatus({ in_use: 'queue' }, 1)).toBe("   ↳ Copy, in this project's queue");
  });

  it('says wasted space in MB under a gigabyte, GB from there', () => {
    expect(wastedSizeText(21_870_000)).toBe('20.9 MB');
    expect(wastedSizeText(3 * 1024 ** 3)).toBe('3.00 GB');
    expect(wastedSizeText(0)).toBe('0.0 MB');
  });
});
